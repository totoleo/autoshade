//! Version metadata: entries and origins, the edit states, naming, deletion and recovery.

use super::*;

/// The version NAME + PROVENANCE sidecar (R24-2): one `.version-meta.json`
/// per develop dir recording, per snapshot number, the user's own name for
/// it and where it came from — `{name, from_kind, from_id, origin}` with
/// `origin` = "user" (an explicit 「＋ Save as version」) or "auto" (the
/// backup gate preserving a save it was about to overwrite).
///
/// Purely ADVISORY. Nothing renders, restores, exports or dedups
/// differently because of it, so every failure path answers "no metadata"
/// instead of blocking the operation it decorates — the opposite stance to
/// the [`DeletedVersions`] registry next door, which fails LOUD because
/// losing an entry there resurrects content the user deleted. Losing a
/// LABEL is cosmetic; losing a delete record is not.
///
/// It follows that registry's NON-GENERATIONAL discipline exactly, for the
/// same reasons:
///
/// * published with [`durable_write`], never `publish_json_sidecar` — no
///   `.bak`, and therefore deliberately NO entry in [`recover_orphan_baks`]'s
///   pair list (a retired copy of an advisory file is a recovery route to
///   nowhere, and republishing one would resurrect names for numbers the
///   live file has since dropped);
/// * never a [`CommitMember`]: it is not part of the develop the
///   single-generation commit publishes as a unit, and a rename must not be
///   able to fail a save;
/// * absent from `clear_sweep`'s explicit file list, exactly like the
///   versions it names — a clear that took the names while the snapshots
///   themselves stand would leave every surviving version anonymous;
/// * absent from [`adoption_skips`], so adoption COPIES it along with the
///   versions (that list is an EXCLUSION list — putting the file in it is
///   what would strand the names behind an adopted develop).
///
/// A name is bound to a NUMBER, and a deleted number is never re-issued
/// (`DeletedVersions::hwm`), so [`delete_version`] drops the entry and no
/// later snapshot can ever inherit it.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VersionMetaEntry {
    pub n: u32,
    /// The user's own name for this snapshot. Capped like every other stored
    /// name ([`MAX_STORE_NAME`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `store_str` spelling of the variant this snapshot was taken from
    /// ("original" | "generated" | "fitted" | "edited" | "denoised"), when the taker knew it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_kind: Option<String>,
    /// That variant's opaque id, so the attribution survives a card the user
    /// later renames or re-kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_id: Option<String>,
    /// [`VERSION_ORIGIN_USER`] or [`VERSION_ORIGIN_AUTO`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// An explicit 「＋ Save as version」.
pub const VERSION_ORIGIN_USER: &str = "user";
/// The backup gate preserving a save a programmatic write was about to
/// replace (analyze persist, reverse-fit, paste, CLI, serve, migration).
pub const VERSION_ORIGIN_AUTO: &str = "auto";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct VersionMeta {
    #[serde(default)]
    versions: Vec<VersionMetaEntry>,
}

pub(super) fn version_meta_path(dev: &Path) -> PathBuf {
    dev.join(".version-meta.json")
}

/// Advisory read: a missing file is the normal case, and an unreadable or
/// corrupt one degrades to "no names" WITH a warning — never an `Err`. A
/// caller that could not open the file still has to be able to save, load
/// and delete versions.
fn read_version_meta_unlocked(dev: &Path) -> VersionMeta {
    match read_text_capped(&version_meta_path(dev), MAX_STORE_JSON) {
        Ok(t) => match serde_json::from_str::<VersionMeta>(&t) {
            Ok(mut m) => {
                for e in &mut m.versions {
                    e.name = e.name.as_deref().map(capped_name);
                }
                m
            }
            Err(e) => {
                eprintln!(
                    "⚠ {} is unreadable ({e}) — version names are not shown (snapshots themselves are unaffected)",
                    version_meta_path(dev).display()
                );
                VersionMeta::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => VersionMeta::default(),
        Err(e) => {
            eprintln!(
                "⚠ {} could not be read ({e}) — version names are not shown (snapshots themselves are unaffected)",
                version_meta_path(dev).display()
            );
            VersionMeta::default()
        }
    }
}

/// Every recorded version name / provenance for this photo, ascending. See
/// [`VersionMetaEntry`] — advisory, so this never fails.
pub fn read_version_meta(src: &Path) -> Vec<VersionMetaEntry> {
    read_version_meta_unlocked(&develop_dir(src)).versions
}

/// What ONE entry of a photo's edit-state list IS (R24-4). The two halves
/// live in different files under different lifecycle rules — `variants.json`
/// is a generation member `clear_develop` sweeps, the `v<n>.recipe.json`
/// family is kept — so this is a synthesized VIEW, never a stored one.
pub enum EditStateKind {
    /// A rendition card from `variants.json`.
    Variant {
        /// [`VariantEntry::kind`] spelling ("original" | "generated" |
        /// "fitted" | "edited" | "denoised"). The taxonomy's binary is
        /// `!= "generated"` (parametric vs pixel-state — the GUI's
        /// `VariantKind::is_source_based`, R24-1); a "denoised" card
        /// (2026-09-15) is parametric over its own denoised master.
        kind: String,
        /// The card `recipe.json` currently mirrors.
        active: bool,
    },
    /// A numbered snapshot (`v<n>.recipe.json`) plus its advisory metadata.
    Version {
        n: u32,
        /// Which card the snapshot was taken from (R24-2), when recorded.
        from_kind: Option<String>,
        from_id: Option<String>,
        /// [`VERSION_ORIGIN_USER`] or [`VERSION_ORIGIN_AUTO`].
        source: Option<String>,
    },
}

/// One edit state of a photo — a rendition card or a numbered snapshot.
pub struct EditState {
    /// The variant's own opaque id, or `v<n>` for a snapshot. An EMPTY
    /// string is a card whose record predates identities (R24-2).
    pub id: String,
    pub name: Option<String>,
    pub state: EditStateKind,
}

/// Every edit state of a photo in ONE query (R24-4): the strip's cards in
/// strip order, then its version snapshots ascending.
///
/// The composition is the point — three files (`variants.json`,
/// `pixels.json`, the `v<n>` family + `.version-meta.json`) answer "what
/// edits does this photo have" together, and every consumer used to join
/// them by hand. Version metadata is restricted to numbers that are actually
/// LISTED: a kill between a delete's sweep and its metadata drop can leave a
/// record for a burned number, and that must never surface as a phantom row.
///
/// The variant half is the LAST SAVED strip. A live editor's own card list
/// outranks it (unsaved pushes, deletes and renames are not here), which is
/// why the GUI consumes the version half of this call and keeps rendering
/// its in-memory strip for the other; for every non-GUI surface this IS the
/// list. The trivial one-card photo has no `variants.json` by design, so it
/// contributes no Variant entry — its single base negative is implicit.
pub fn list_edits(src: &Path) -> Vec<EditState> {
    let mut out: Vec<EditState> = Vec::new();
    if let Some(rec) = read_variants(src) {
        let mut cards: Vec<EditState> = rec
            .others
            .iter()
            .map(|e| EditState {
                id: e.id.clone().unwrap_or_default(),
                name: e.name.clone(),
                state: EditStateKind::Variant {
                    kind: e.kind.clone(),
                    active: false,
                },
            })
            .collect();
        // The active card is NOT in `others` (that is the record's shape):
        // it goes back at its recorded position, clamped — a hand-edited
        // `active_pos` past the end must not panic a read-only listing.
        let at = rec.active_pos.min(cards.len());
        cards.insert(
            at,
            EditState {
                id: rec.active_id.clone().unwrap_or_default(),
                name: rec.active_name.clone(),
                state: EditStateKind::Variant {
                    kind: rec.active_kind.clone(),
                    active: true,
                },
            },
        );
        out.extend(cards);
    }
    let meta = read_version_meta(src);
    for n in list_versions(src) {
        let m = meta.iter().find(|e| e.n == n);
        out.push(EditState {
            id: format!("v{n}"),
            name: m.and_then(|e| e.name.clone()),
            state: EditStateKind::Version {
                n,
                from_kind: m.and_then(|e| e.from_kind.clone()),
                from_id: m.and_then(|e| e.from_id.clone()),
                source: m.and_then(|e| e.origin.clone()),
            },
        });
    }
    out
}

fn write_version_meta_unlocked(dev: &Path, meta: &VersionMeta) -> std::io::Result<()> {
    if meta.versions.is_empty() {
        // Nothing left to say: take the file away rather than leave an empty
        // registry standing (a develop with no names looks exactly like one
        // that never had any — which is what it is).
        return match std::fs::remove_file(version_meta_path(dev)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
    }
    let json = serde_json::to_string_pretty(meta).map_err(std::io::Error::other)?;
    durable_write(&version_meta_path(dev), json.as_bytes())
}

/// Merge the `Some` fields of `entry` into v`entry.n`'s record (a `None`
/// leaves what is already there — a later rename must not erase the
/// provenance the save stamped, and vice versa). Creates the record when
/// the number has none.
pub(super) fn record_version_meta_unlocked(dev: &Path, entry: &VersionMetaEntry) -> std::io::Result<()> {
    let mut meta = read_version_meta_unlocked(dev);
    match meta.versions.iter_mut().find(|e| e.n == entry.n) {
        Some(cur) => {
            if entry.name.is_some() {
                cur.name = entry.name.as_deref().map(capped_name);
            }
            if entry.from_kind.is_some() {
                cur.from_kind = entry.from_kind.clone();
            }
            if entry.from_id.is_some() {
                cur.from_id = entry.from_id.clone();
            }
            if entry.origin.is_some() {
                cur.origin = entry.origin.clone();
            }
        }
        None => {
            let mut fresh = entry.clone();
            fresh.name = fresh.name.as_deref().map(capped_name);
            meta.versions.push(fresh);
        }
    }
    meta.versions.sort_by_key(|e| e.n);
    write_version_meta_unlocked(dev, &meta)
}

/// Record (merge) advisory metadata for one version snapshot.
pub fn record_version_meta(src: &Path, entry: &VersionMetaEntry) -> std::io::Result<()> {
    with_develop_lock(src, DevelopLockMode::Wait, || {
        record_version_meta_unlocked(&develop_dir(src), entry)
    })
}

/// Name v`n` — `None` (or an all-whitespace name) CLEARS the name and
/// leaves the provenance standing. Unlike [`record_version_meta`]'s merge,
/// this one is authoritative for the name field: the rename UI has to be
/// able to take a name back off.
pub fn set_version_name(src: &Path, n: u32, name: Option<&str>) -> std::io::Result<()> {
    let name = name.map(str::trim).filter(|s| !s.is_empty()).map(capped_name);
    with_develop_lock(src, DevelopLockMode::Wait, || {
        let dev = develop_dir(src);
        std::fs::create_dir_all(&dev)?;
        let mut meta = read_version_meta_unlocked(&dev);
        match meta.versions.iter_mut().find(|e| e.n == n) {
            Some(cur) => cur.name = name.clone(),
            None => meta.versions.push(VersionMetaEntry {
                n,
                name: name.clone(),
                ..Default::default()
            }),
        }
        // A record that now says nothing at all is noise: drop it, so a name
        // typed and taken back off leaves the dir as it was.
        meta.versions.retain(|e| {
            e.name.is_some() || e.from_kind.is_some() || e.from_id.is_some() || e.origin.is_some()
        });
        meta.versions.sort_by_key(|e| e.n);
        write_version_meta_unlocked(&dev, &meta)
    })
}

/// Drop v`n`'s advisory record — the delete half of "a name dies with its
/// number". Best-effort by contract: the caller ignores the result, because
/// a surviving name can never be re-attached (the number is burned in
/// [`DeletedVersions`] and never re-issued) while a failed delete over a
/// LABEL would be absurd.
fn forget_version_meta_unlocked(dev: &Path, n: u32) -> std::io::Result<()> {
    let mut meta = read_version_meta_unlocked(dev);
    let before = meta.versions.len();
    meta.versions.retain(|e| e.n != n);
    if meta.versions.len() == before {
        return Ok(());
    }
    write_version_meta_unlocked(dev, &meta)
}

pub(super) fn delete_version_unlocked(src: &Path, n: u32) -> std::io::Result<()> {
    // The stale-list probe comes BEFORE anything durable: a 🗑 on a version
    // that is already gone (deleted by another surface since the list was
    // drawn — or a number that never was one) answers NotFound and leaves
    // NO trace. The marker below used to be written first, survived the
    // NotFound that `register_deleted_version(.., true)` raised right after
    // it, and the next recovery then burned that number fingerprint-less
    // and lifted the high-water mark to it — a registry entry, and a skipped
    // claim number, for a version that never existed. Only ABSENCE
    // short-circuits: an unreadable snapshot stays deletable (the registry's
    // fingerprint-less rule).
    if let Err(e) = std::fs::metadata(version_target(src, n))
        && e.kind() == std::io::ErrorKind::NotFound
    {
        return Err(e);
    }
    // TRANSACTION MARKER before anything is destroyed (L03): a kill mid-sweep
    // used to leave a half-version — recipe alive with rasters gone (listed,
    // loadable, rendering dead masks the next save persists as dangling
    // paths) or rasters alive with the recipe gone (orphan blobs forever, and
    // the number silently recyclable). The marker records the intent durably;
    // the sweep resumes at the next claim or locked recovery touch.
    durable_write(&deleting_marker(src, n), format!("deleting v{n}\n").as_bytes())?;
    // REGISTRY SECOND, while the fingerprint sources still exist — the
    // sweep destroys the very bytes the gate needs to keep recognising as
    // "explicitly discarded". A failure leaves the marker: the version
    // stays unlisted and recovery retries the whole tail.
    register_deleted_version(src, n, true)?;
    sweep_version_unlocked(src, n, true)?;
    // The NAME dies with the number (R24-2), and it dies LAST: the record is
    // advisory, so a failure here must not fail the delete, and a crash in
    // this window leaves a name attached to a number that is already both
    // unlisted (swept) and permanently unclaimable (burned in the registry
    // above) — unreachable, never re-attachable to a later snapshot.
    let _ = forget_version_meta_unlocked(&develop_dir(src), n);
    let _ = std::fs::remove_file(deleting_marker(src, n));
    settle_consumed_marker(&deleting_marker(src, n));
    Ok(())
}

/// The sweep half of [`delete_version_unlocked`]: rasters first, recipe
/// last. `must_exist` keeps a fresh delete's NotFound error for a
/// stale-list 🗑 while a RESUME tolerates the recipe already being gone.
fn sweep_version_unlocked(src: &Path, n: u32, must_exist: bool) -> std::io::Result<()> {
    // Sweep the frozen rasters FIRST, recipe LAST: with the old order a
    // raster-removal failure was silently discarded after the recipe was
    // already gone, and a retry returned early on the missing recipe —
    // stranding potentially large frozen bitmaps forever. Now a failed
    // raster removal is reported and the version stays retryable.
    let prefix = format!("v{n}.");
    let recipe_name = version_target(src, n)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut first_err: Option<std::io::Error> = None;
    match std::fs::read_dir(develop_dir(src)) {
        Ok(dir) => {
            for e in dir {
                // A per-ENTRY enumeration error is an enumeration failure
                // too: swallowing it (the old `.flatten()`) let the recipe
                // deletion below proceed past rasters the sweep never saw.
                let e = match e {
                    Ok(e) => e,
                    Err(err) => {
                        if first_err.is_none() {
                            first_err = Some(err);
                        }
                        continue;
                    }
                };
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                // The dot terminator keeps "v3." from matching "v30.recipe.json".
                if name.starts_with(&prefix)
                    && name != recipe_name
                    && let Err(err) = std::fs::remove_file(e.path())
                    && first_err.is_none()
                {
                    first_err = Some(err);
                }
            }
        }
        // A missing dir has nothing to strand; any OTHER enumeration failure
        // must abort BEFORE the recipe deletion, or rasters hidden behind it
        // lose their version entry and become unreachable forever.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    if let Some(err) = first_err {
        return Err(err);
    }
    match std::fs::remove_file(version_target(src, n)) {
        Ok(()) => Ok(()),
        Err(e) if !must_exist && e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Resume every KILLED version delete whose marker survives. Best-effort
/// per version — a locked raster keeps its marker (the version stays
/// unlisted and its number unclaimable) and the next touch retries.
pub(super) fn recover_pending_version_deletes_unlocked(src: &Path) -> std::io::Result<()> {
    let mut pending = Vec::new();
    match std::fs::read_dir(develop_dir(src)) {
        Ok(dir) => {
            for e in dir.flatten() {
                if let Some(n) = e
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix(".deleting.v"))
                    .and_then(|rest| rest.parse::<u32>().ok())
                {
                    pending.push(n);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    let mut failure: Option<std::io::Error> = None;
    for n in pending {
        // Burn the number BEFORE finishing the sweep (idempotent; a resume
        // whose recipe already fell registers fingerprint-less — the number
        // stays burned even with the content gone, and a pre-registry
        // marker gets its entry here too).
        match register_deleted_version(src, n, false)
            .and_then(|()| sweep_version_unlocked(src, n, false))
        {
            Ok(()) => {
                let _ = std::fs::remove_file(deleting_marker(src, n));
                settle_consumed_marker(&deleting_marker(src, n));
            }
            Err(e) => {
                failure.get_or_insert(e);
            }
        }
    }
    match failure {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

/// Best-effort breadcrumb so a human browsing the hashed store can tell which
/// photo a develop dir belongs to. Never fails the caller.
pub fn note_source(src: &Path) {
    let dir = develop_dir(src);
    let marker = dir.join("source.txt");
    if marker.exists() {
        return;
    }
    let abs = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = durable_write(&marker, format!("{}\n", abs.display()).as_bytes());
    }
}
