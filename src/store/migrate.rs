//! Migrating legacy stores.

use super::*;

/// One-time, per-photo migration of legacy ./out sidecars into the central
/// develop dir. Idempotent and best-effort per file: a file that cannot move
/// stays where it was and the legacy read fallbacks keep serving it (nothing
/// is ever deleted without its copy landing first). Returns true when at
/// least one file was migrated.
///
/// Recipe files are parsed so their raster references can move along and be
/// rewritten to bare names; an UNPARSABLE recipe is moved byte-for-byte —
/// the loud `Unreadable` handling at read time stays intact.
///
/// Concurrency: two processes migrating the same photo at once (GUI + `serve`)
/// race benignly — both derive from the same legacy bytes, so whichever write
/// lands carries the same content, and the per-file existence checks plus
/// resumability finish any remainder on the next touch. No lock: the store is
/// single-user by construction, and identical-content races cannot corrupt.
pub fn migrate_legacy(src: &Path) -> bool {
    match with_develop_lock(src, DevelopLockMode::Wait, || {
        Ok::<_, std::io::Error>(migrate_legacy_unlocked(src))
    }) {
        Ok(moved) => moved,
        Err(e) => {
            eprintln!(
                "⚠ legacy develops for {} could not be migrated under the develop lock ({e})",
                src.display()
            );
            false
        }
    }
}

fn migrate_legacy_unlocked(src: &Path) -> bool {
    // Ahead of the memo: this is the "touching this photo" hook every reader
    // (GUI read_saved_develop, web api_recipe) already calls, and a crashed
    // publish must be repaired on EVERY touch, not once per process. A failed
    // recovery is reported by the helper and re-decided by the backup gate,
    // which refuses to overwrite what it could not snapshot.
    let _ = recover_orphan_baks(src);
    // AFTER the recovery, never before the lock (L03): every explicit clear
    // writes the tombstone, so the suppressed early-return used to skip the
    // recovery hook for exactly the photos a killed clear leaves behind —
    // and the web's api_recipe reaches recovery only through this call.
    if legacy_suppressed(src) {
        return false;
    }
    // Process-wide memo: a photo can only need migrating once per process, and
    // this runs on every photo open (UI thread) — without the memo a library
    // whose ./out holds thousands of exports pays a full directory enumeration
    // on every reopen for nothing.
    use std::sync::{Mutex, OnceLock};
    static DONE: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let key = photo_key(src);
    {
        let mut done = DONE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !done.insert(key) {
            return false;
        }
    }
    let mut moved = false;
    let mut failed = false;
    for legacy_out in legacy_out_roots() {
        let (m, f) = migrate_legacy_in(&store_root(), &legacy_out, src);
        moved |= m;
        failed |= f;
    }
    if failed {
        // A transient failure (AV lock, network volume) must stay RETRYABLE:
        // leaving the memo key in place silenced every later open this
        // process, hiding legacy sidecars for the whole session.
        let mut done = DONE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        done.remove(&photo_key(src));
    }
    moved
}

/// Gallery-wide variant of [`migrate_legacy`] (whose per-photo body is
/// `migrate_legacy_in`): the legacy folder is scanned ONCE for version
/// snapshots and the result shared across photos — the per-photo call re-ran
/// that `read_dir` for every photo, making a big import O(photos × directory
/// entries). Returns how many photos had anything migrated.
pub fn migrate_legacy_from_many(legacy_out: &Path, photos: &[PathBuf]) -> usize {
    if !legacy_out.is_dir() {
        return 0;
    }
    // stem → its legacy "v<N>" snapshot files. A snapshot is named
    // "<stem>.v<digits>.recipe.json"; stems may contain dots, so split at the
    // LAST ".v<digits>" tail and require the head to be an actual gallery
    // stem (same answer the old per-stem prefix scan produced).
    let stems: std::collections::HashSet<&str> =
        photos.iter().map(|p| crate::pipeline::stem(p)).collect();
    let mut versions: std::collections::HashMap<String, Vec<(PathBuf, String)>> =
        std::collections::HashMap::new();
    if let Ok(dir) = std::fs::read_dir(legacy_out) {
        for e in dir.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name.strip_suffix(".recipe.json") else { continue };
            let Some((head, nums)) = rest.rsplit_once(".v") else { continue };
            if nums.parse::<u32>().is_ok() && stems.contains(head) {
                versions
                    .entry(head.to_string())
                    .or_default()
                    .push((e.path(), format!("v{nums}.recipe.json")));
            }
        }
    }
    let root = store_root();
    photos
        .iter()
        .filter(|p| {
            let vjobs = versions
                .get(crate::pipeline::stem(p))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            with_develop_lock_in(&root, p, DevelopLockMode::Wait, || {
                // The crashed-publish survivor FIRST, as the per-photo path
                // (`migrate_legacy_unlocked`) does: a `.bak` whose live file
                // never landed is the NEWEST save, and the no-clobber publish
                // below would otherwise fill the empty slot with the OLDER
                // legacy bytes — after which the recovery sees a live file
                // and leaves the survivor retired for good. A failed recovery
                // is reported by the helper; the .bak stays and the backup
                // gate re-decides on the next touch.
                let _ = recover_orphan_baks_unlocked(p);
                // The explicit-clear tombstone, honoured here as the per-photo
                // path honours it (`migrate_legacy_unlocked`): an "Import
                // legacy" over a develop the user had cleared re-imported the
                // files that clear retired, until 2026-09-24.
                if legacy_suppressed_in(&root, p) {
                    return Ok::<_, std::io::Error>(false);
                }
                Ok::<_, std::io::Error>(migrate_legacy_jobs(&root, legacy_out, p, vjobs).0)
            })
            .unwrap_or(false)
        })
        .count()
}

/// Returns `(moved_anything, any_attempt_failed)` — the second flag lets the
/// memoized caller keep a FAILED photo retryable instead of silencing it for
/// the process lifetime.
pub(super) fn migrate_legacy_in(root: &Path, legacy_out: &Path, src: &Path) -> (bool, bool) {
    // Cheap gate — most calls find nothing legacy at all. Triage instead of
    // `is_dir()`: that folded an INACCESSIBLE directory (AV lock, offline
    // volume) into "nothing legacy", and the caller's process memo then
    // silenced the photo for the whole session.
    match std::fs::metadata(legacy_out) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return (false, false), // exists but is not a directory
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (false, false),
        Err(_) => return (false, true), // inaccessible — retryable failure
    }
    let stem = crate::pipeline::stem(src);
    let vprefix = format!("{stem}.v");
    let mut vjobs: Vec<(PathBuf, String)> = Vec::new();
    // A failed enumeration is a FAILED attempt, not an empty scan: reporting
    // success here let the process memo mark the photo done and never retry
    // the undiscovered legacy versions this session.
    let mut scan_failed = false;
    match std::fs::read_dir(legacy_out) {
        Ok(dir) => {
            for e in dir {
                let Ok(e) = e else {
                    scan_failed = true;
                    continue;
                };
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(rest) = name.strip_prefix(&vprefix)
                    && let Some(nums) = rest.strip_suffix(".recipe.json")
                    && nums.parse::<u32>().is_ok()
                {
                    vjobs.push((e.path(), format!("v{nums}.recipe.json")));
                }
            }
        }
        Err(_) => scan_failed = true,
    }
    let (moved, failed) = migrate_legacy_jobs(root, legacy_out, src, &vjobs);
    (moved, failed || scan_failed)
}

/// The per-photo migration body, with the version-snapshot scan factored OUT
/// so gallery imports can share one directory listing (`version_jobs` =
/// `(legacy file, central v<N> name)` pairs for this photo's stem).
fn migrate_legacy_jobs(
    root: &Path,
    legacy_out: &Path,
    src: &Path,
    version_jobs: &[(PathBuf, String)],
) -> (bool, bool) {
    let stem = crate::pipeline::stem(src);
    let dev = develop_dir_in(root, src);

    // Collect (legacy file, new name) pairs.
    let mut jobs: Vec<(PathBuf, String, bool)> = Vec::new(); // (from, to-name, is_recipe)
    let lr = legacy_out.join(format!("{stem}.recipe.json"));
    if lr.exists() {
        jobs.push((lr, "recipe.json".into(), true));
    }
    let lx = legacy_out.join(format!("{stem}.xmp"));
    if lx.exists() {
        jobs.push((lx, format!("{stem}.xmp"), false));
    }
    for (from, to_name) in version_jobs {
        jobs.push((from.clone(), to_name.clone(), true));
    }
    if jobs.is_empty() {
        return (false, false);
    }
    if std::fs::create_dir_all(&dev).is_err() {
        return (false, true);
    }
    let abs_src = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    let marker = dev.join("source.txt");
    if !marker.exists() {
        let _ = durable_write(&marker, format!("{}\n", abs_src.display()).as_bytes());
    }

    // Version snapshots the user DELETED are not re-migrated (adversarial
    // review HIGH-1 — the third resurrection arm): legacy bytes are
    // RETAINED by design, so "central v<N> missing" is ambiguous between
    // "never migrated" and "deleted after migration", and the exists-check
    // below re-published a deleted version on every open. The registry
    // disambiguates; unreadable → migrate as before (fail-open — the claim
    // and delete paths report that corruption loudly).
    let burned = read_deleted_versions(&dev).unwrap_or_default();
    let mut moved = false;
    let mut failed = false;
    for (from, to_name, is_recipe) in jobs {
        let to = dev.join(&to_name);
        if let Some(n) = to_name
            .strip_prefix("v")
            .and_then(|rest| rest.strip_suffix(".recipe.json"))
            .and_then(|digits| digits.parse::<u32>().ok())
            && (burned.deleted.iter().any(|e| e.n == n)
                || dev.join(format!(".deleting.v{n}")).exists())
        {
            continue;
        }
        if to.exists() {
            // The central copy is the post-migration truth — never clobber it
            // with an older legacy file. The legacy file stays for the read
            // fallbacks (deleting user data it never copied would be worse).
            // (The publishes below re-check ATOMICALLY via create_new claims;
            // this early check just skips obvious work.)
            continue;
        }
        let ok = if is_recipe {
            migrate_one_recipe(&from, &to, stem, &dev, legacy_out)
        } else {
            // Ok(false) = destination owned by a newer writer — the desired
            // outcome, not a failure. (The previous `|| to.exists()` also
            // blessed OUR OWN claim residue after a move+cleanup double
            // failure — an empty file then shadowed the intact legacy
            // artifact as a "successfully migrated" one.)
            move_file_no_clobber(&from, &to).is_ok()
        };
        moved |= ok;
        failed |= !ok;
    }
    (moved, failed)
}

/// Move one legacy recipe file, carrying its `./out/<stem>.<kind>.png` raster
/// references into the develop dir (rewritten to bare `<kind>.png` names).
/// Rasters shared by several recipe files move once; later files just rewrite.
///
/// Failure-ordering contract (a migration must never make things WORSE than
/// not migrating): rasters are STAGED as copies first, the rewritten recipe is
/// published next, and the legacy originals are never touched — their
/// stem-only identity cannot prove they belong to this photo, so they are
/// RETAINED for the read fallbacks and older builds (see the tail below). Any
/// failure leaves every legacy file byte-identical — the read fallbacks keep
/// serving it. Staged central copies are NOT rolled back: they are
/// identical-content derivations a concurrent migration may already
/// reference (rolling them back deleted the winner's bitmap).
fn migrate_one_recipe(from: &Path, to: &Path, stem: &str, dev: &Path, legacy_out: &Path) -> bool {
    let Ok(text) = read_text_capped(from, MAX_STORE_JSON) else { return false };
    let Ok(mut r) = serde_json::from_str::<EditRecipe>(&text) else {
        // Unparsable (interrupted write / newer schema): move byte-for-byte so
        // the read path can keep reporting it loudly as Unreadable. Ok(false)
        // = a central owner exists — success, not failure (see the
        // migrate_legacy_jobs caller for why `|| to.exists()` was wrong).
        return move_file_no_clobber(from, to).is_ok();
    };
    let raster_prefix = format!("{stem}.");
    for m in &mut r.masks {
        for path in m.bitmap_paths_mut() {
            let mut p = PathBuf::from(path.as_str());
            if p.is_absolute() {
                continue; // foreign reference — not ours to move
            }
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
            let Some(bare) = name.strip_prefix(&raster_prefix).map(str::to_string) else {
                continue;
            };
            // Legacy refs are relative to the OLD launch cwd ("out/<stem>.<kind>.png").
            // PREFER the raster inside the root being migrated: a same-named
            // file under TODAY'S cwd can belong to a different context (same
            // stem, another photo), while the one beside the legacy recipe is
            // the raster it was written against. The recipe's own
            // cwd-relative reading stays as the fallback for a launch from
            // the original directory (where the two spellings coincide).
            let cand = legacy_out.join(name);
            if cand.exists() {
                p = cand;
            }
            let dest = dev.join(&bare);
            if dest.exists() {
                *path = bare; // already migrated by an earlier file / process
                continue;
            }
            if !p.exists() {
                // Keep the old reference — the engine's missing-raster
                // contract reports it.
                continue;
            }
            // Stage a private full copy, then hard-link-publish (see
            // publish_no_clobber): the old create_new claim exposed a 0-byte
            // dest between claim and copy, and its rollback could delete a
            // raster a concurrent migration had already adopted.
            let tmp = sibling_tmp(&dest);
            if std::fs::copy(&p, &tmp).is_err() {
                let _ = std::fs::remove_file(&tmp);
                continue; // keep the old reference
            }
            // On Err the old reference is kept — the engine's missing-raster
            // contract reports it. Published OR adopted (an identical-content
            // copy landed meanwhile): the central raster is in place either
            // way, and the legacy source stays where it was — retained like
            // every legacy byte (see the fn docs).
            if publish_no_clobber(&tmp, &dest).is_ok() {
                *path = bare;
            }
        }
    }
    // Publish the rewritten recipe via the hard-link no-clobber publisher: a
    // central recipe created meanwhile (a concurrent save) must never be
    // replaced by older legacy bytes. The previous create_new claim + rename
    // still overwrote a save landing inside the claim→rename window, and its
    // failure cleanup deleted the claim blindly even after a save had
    // replaced it.
    let published = (|| -> Option<()> {
        let json = serde_json::to_string_pretty(&r).ok()?;
        let tmp = sibling_tmp(to);
        if write_staged(&tmp, json.as_bytes()).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        matches!(publish_no_clobber(&tmp, to), Ok(true)).then_some(())
    })();
    if published.is_none() {
        // Deliberately NO raster rollback (the fn's failure-ordering contract): every
        // legacy file is still byte-identical and keeps serving via the read
        // fallbacks, while the staged central rasters stay as harmless
        // identical-content copies a concurrent migration may already
        // reference.
        return false;
    }
    // The central copies are now authoritative for this photo, but neither the
    // stem-keyed recipe nor its rasters can be proven to belong to this path.
    // Retain all legacy bytes for older builds and same-stem photos.
    true
}
