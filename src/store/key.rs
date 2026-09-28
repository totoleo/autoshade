//! The photo key: the stem and the hash, case folding read off the volume, path identity, alias notes, key resolution, and orphan adoption.

use super::*;

/// Stable per-photo key: `<stem>-<16 hex>` from the photo's IDENTITY
/// spelling — the canonical on-disk form for local volumes (C1/F10 rework,
/// user-decided 2026-08-10), so a symlink/junction/8.3/case alias of one
/// photo yields ONE key and one develop dir (and one develop LOCK — the
/// concurrency cluster's exclusion was void across aliases). Network and
/// device paths, and spellings that cannot be resolved, keep the LEXICAL
/// key (see [`identity_of`]); canonical identity does not see through
/// hardlinks or two mount points of one volume. The stem prefix keeps the
/// store browsable; the hash disambiguates same-named photos in different
/// folders. Windows paths are case-insensitive
/// (NTFS), so BOTH halves fold case there — one file must never produce two
/// keys just because it was opened as `D:\DSC001.ARW` once and
/// `d:\dsc001.arw` once. (The hash folded from the start; the stem prefix
/// did not, so the promise held only because NTFS resolves the two spellings
/// to one directory anyway. That same resolution keeps every pre-fold store
/// dir reachable under the folded key on the default root. A deliberately
/// case-SENSITIVE data dir — an opt-in fsutil/WSL configuration — never had
/// the one-key guarantee to begin with, and keeps any old mixed-case dirs
/// as orphans; the store's stem-cased artifact names assume a
/// case-insensitive root on Windows throughout.)
///
/// OFF Windows the fold does NOT run — a compile-time constant, deliberately,
/// even though [`volume_folds_case`] can now answer the same question per
/// volume and every path COMPARISON in this app asks it.
///
/// A key is not a comparison. This one names a DIRECTORY that has to be the
/// same one next week: `<stem>-<hash>` is where every recipe, snapshot, mask
/// raster and lock for that photo lives. The probe reads the filesystem, and a
/// filesystem read can fail for reasons that have nothing to do with case — a
/// disconnected network volume, a permission, an antivirus holding the
/// directory — at which point it answers [`DEFAULT_CASE_FOLD`] instead, the key
/// changes, and the develop orphans. Stability outranks precision here, and the
/// asymmetry decides the direction: folding on a case-sensitive volume MERGES
/// two genuinely different photos into one develop dir and one develop lock, so
/// each save overwrites the other's recipe, while not folding on a
/// case-insensitive one costs at most a second develop dir, both readable and
/// neither destroyed. So the fold runs exactly where the platform guarantees it
/// for every local volume. Aliases off Windows are handled where they can be
/// handled truthfully instead: [`identity_of`] resolves the spelling through
/// `fs::canonicalize`, which collapses symlink and `..` aliases on every
/// platform (whether a given libc's `realpath` ALSO normalises case is not
/// relied on here — nothing in this module reads case out of it).
pub fn photo_key(src: &Path) -> String {
    key_from_spelling(&identity_of(src))
}

/// Yesterday's key, byte-identical: the spelling handed in, made absolute
/// LEXICALLY (no link resolution). Still real, not legacy-only: it is the
/// fallback identity for network/device paths and unresolvable spellings,
/// and [`resolve_key_in`] consults it to adopt develops saved by pre-C1
/// builds.
pub(crate) fn photo_key_lexical(src: &Path) -> String {
    key_from_spelling(&std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf()))
}

fn key_from_spelling(abs: &Path) -> String {
    // The STORE KEY's case rule, and the one place it is decided. Deliberately
    // NOT [`volume_folds_case`]: see `photo_key` for why a key may not depend
    // on a probe that can fail.
    key_from_spelling_folded(abs, cfg!(windows))
}

/// The case rule to assume when a volume cannot be probed.
///
/// Windows folds on every local volume by default, macOS ships APFS
/// case-insensitive and formats case-SENSITIVE only on request, and every
/// ordinary Linux filesystem is case-sensitive.
pub(super) const DEFAULT_CASE_FOLD: bool = cfg!(windows) || cfg!(target_os = "macos");

/// Does the volume `path` sits on resolve names case-INSENSITIVELY?
///
/// Case sensitivity is a property of the VOLUME, not of the operating system,
/// and one machine can hold both kinds: a Linux box with an exFAT card mounted,
/// a Mac formatted case-sensitive, a Windows directory opted out of folding by
/// `fsutil` or created from WSL. Every path comparison that asked
/// `cfg!(windows)` instead was therefore wrong on some real setup — it let a
/// case-flipped `-o` past the overwrite gate on macOS and on a Linux-mounted
/// card, and conflated two genuinely distinct files inside a case-sensitive
/// Windows directory. This is the ONE helper they all ask now.
///
/// Empirical and read-only: it flips the ASCII case of an existing entry's name
/// and asks whether the flipped spelling names the SAME OBJECT. Same object,
/// not merely "something exists" — a directory holding both `Photos` and
/// `pHOTOS` holds two of them and is case-SENSITIVE, and answering "folds"
/// there is the one error that costs data.
///
/// Probed once per directory per process, at the deepest EXISTING ancestor of
/// `path` (the leaf commonly does not exist yet). A directory that cannot be
/// probed — unreadable, or empty of any name carrying an ASCII letter — answers
/// [`DEFAULT_CASE_FOLD`].
pub fn volume_folds_case(path: &Path) -> bool {
    let Some(dir) = path.ancestors().skip(1).find(|p| p.is_dir()) else {
        return DEFAULT_CASE_FOLD;
    };
    static MEMO: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>,
    > = std::sync::OnceLock::new();
    let memo = MEMO.get_or_init(Default::default);
    // A poisoned mutex must not change the ANSWER: probe again rather than
    // panic. The memo is a cache of a pure question about the filesystem, so
    // a lost entry costs one extra `read_dir` and nothing else.
    if let Ok(m) = memo.lock()
        && let Some(hit) = m.get(dir)
    {
        return *hit;
    }
    let folds = dir_folds_case(dir).unwrap_or(DEFAULT_CASE_FOLD);
    if let Ok(mut m) = memo.lock() {
        // One entry per directory touched, and a session touches a bounded
        // number of them; the store's own key memo is the larger of the two.
        m.insert(dir.to_path_buf(), folds);
    }
    folds
}

/// [`volume_folds_case`]'s probe, on one directory, with no memo.
///
/// `None` when the directory answers nothing: unreadable, or holding no name
/// with an ASCII letter to flip. The scan stops at the first usable name, and
/// at 64 entries either way so a photo library's root costs a bounded read.
pub(super) fn dir_folds_case(dir: &Path) -> Option<bool> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten().take(64) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().any(|b| b.is_ascii_alphabetic()) {
            continue;
        }
        let flipped: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        return Some(same_object(&dir.join(name), &dir.join(&flipped)));
    }
    None
}

/// Do these two spellings name ONE object on disk?
#[cfg(unix)]
fn same_object(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // Device + inode is the platform's own answer to this question, and the
    // only one that survives a case-INSENSITIVE volume: `realpath` returns the
    // spelling it was handed there, so two spellings of one file canonicalize
    // to two different strings.
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// Do these two spellings name ONE object on disk?
#[cfg(not(unix))]
fn same_object(a: &Path, b: &Path) -> bool {
    // Windows exposes no stable inode on stable Rust (`file_index` is behind
    // an unstable feature). `canonicalize` opens the object and asks the
    // filesystem for its FINAL name, which comes back in the TRUE on-disk
    // casing, so two spellings of one file canonicalize to one string.
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// [`key_from_spelling`] with that case rule handed IN, so BOTH halves of it
/// are exercised on every host the suite runs on: a Linux runner proves the
/// folding branch and a Windows runner proves the case-sensitive one (see
/// `the_stem_fold_never_invents_a_name_ntfs_cannot_resolve`). Nothing but
/// that test passes anything other than `cfg!(windows)`.
pub(super) fn key_from_spelling_folded(abs: &Path, fold_case: bool) -> String {
    let mut s = abs.to_string_lossy().into_owned();
    // BOTH halves normalise, from the SAME string. The hash was taken from
    // `abs` while the stem was taken from the raw `src`, so a spelling that
    // `absolute()` rewrites produced one hash and two directory names: Windows
    // drops a trailing dot when opening, so `…\DSC001.NEF` and `…\DSC001.NEF.`
    // are one file, yet `file_stem()` answered "DSC001" for the first and
    // "DSC001.NEF" for the second. The develop saved under one spelling was
    // then invisible under the other, and the next save built a fresh empty
    // develop beside it. For every ordinary path the two agree, so this
    // re-keys nothing that exists.
    let mut stem = crate::pipeline::stem(abs).to_string();
    if fold_case {
        s = s.to_lowercase();
        // ASCII-only for the DIRECTORY NAME half. Rust's full Unicode
        // lowercase and NTFS's $UpCase table disagree — and where they do,
        // the folded name is a directory that does not exist: measured on
        // this machine, "İMG_001" folds to "i\u{307}mg_001" (one char becomes
        // two), "ΣΑΣ" to "σας" (final sigma) and "ẞ" to "ß", and NONE of the
        // three resolve back to the pre-fold directory. The develop dir would
        // silently orphan — recipe, XMP, every version snapshot, every mask
        // raster and the master link — and the next save would create a fresh
        // empty one beside it. ASCII folding is the subset NTFS always agrees
        // with, and it is the case the fold was added for.
        //
        // No migration is needed for the full-Unicode spelling this replaces:
        // it existed only between two unreleased commits of this same fix
        // wave, so no build that ever shipped could have created a directory
        // under it.
        stem = stem.to_ascii_lowercase();
    }
    let suffix = format!("-{:016x}", fnv1a64(s.as_bytes()));
    // Leave headroom for filesystem metadata and keep the same key for every
    // existing ordinary stem. Enforce both byte-oriented Unix limits and
    // UTF-16-oriented Windows limits without splitting a scalar value.
    const MAX_COMPONENT_UNITS: usize = 240;
    let mut prefix = String::new();
    let mut bytes = 0usize;
    let mut wide = 0usize;
    for ch in stem.chars() {
        let next_bytes = bytes + ch.len_utf8();
        let next_wide = wide + ch.len_utf16();
        if next_bytes + suffix.len() > MAX_COMPONENT_UNITS
            || next_wide + suffix.encode_utf16().count() > MAX_COMPONENT_UNITS
        {
            break;
        }
        prefix.push(ch);
        bytes = next_bytes;
        wide = next_wide;
    }
    format!("{prefix}{suffix}")
}


/// The photo's IDENTITY spelling, memoized for the process lifetime. The
/// memo is LOAD-BEARING for correctness, not a cache: `develop_dir` must
/// answer the SAME directory for a given input path for the whole session
/// (raster re-anchoring via `parent() == dir`, the commit staging base, and
/// the develop-lock path all assume it), and canonicalize is a per-call
/// filesystem probe whose answer can flip transiently (AV holding the file,
/// a link retargeted mid-session). Consequence, documented: a link
/// retargeted while AutoShade runs keeps this session on the identity it
/// opened with.
fn identity_of(src: &Path) -> PathBuf {
    use std::sync::{Mutex, OnceLock};
    let abs = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    // LEXICAL-FIRST (the F4 rule, user-decided 2026-08-10): a network or
    // device path's identity is its spelling. Canonicalizing one costs a
    // network round trip per photo — a dead mapping would hang every key
    // derivation — and the store has twice decided not to reach off this
    // machine. Note this predicate has a second duty here, distinct from
    // the F4 refusals ("a develop pack may not reach off this machine"):
    // it also means "do not spend a network round trip on identity".
    if remote_identity(&abs) {
        return abs;
    }
    static MEMO: OnceLock<Mutex<std::collections::HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(hit) = memo.lock().unwrap().get(&abs) {
        return hit.clone();
    }
    let (id, hard_fallback) = identity_spelling(&abs);
    if hard_fallback {
        // HARD fallback only (no ancestor resolved — e.g. a disconnected
        // drive): a merely-absent leaf still resolves through its parent
        // and is not disclosed. Once per photo per process via the memo.
        eprintln!(
            "⚠ {} could not be resolved to its on-disk form — its develop is keyed by the \
             path spelling, so a link or short-name alias of this photo would get a \
             separate develop",
            abs.display()
        );
    }
    let mut m = memo.lock().unwrap();
    // Bounded (the memory-boundary idiom). NOT cleared on overflow: existing
    // entries are the stability promise, so they stay; entries past the cap
    // are simply recomputed per call (deterministic for a given disk state).
    const MEMO_CAP: usize = 50_000;
    if m.len() < MEMO_CAP {
        m.entry(abs).or_insert_with(|| id.clone());
    }
    id
}

/// Resolve `abs` to its canonical on-disk spelling: canonicalize the deepest
/// EXISTING ancestor and re-attach the not-yet-created tail (the
/// `pipeline::resolve_existing_pub` shape — an absent photo still keys by
/// the folder that holds it). Returns `(spelling, hard_fallback)`; a hard
/// fallback means NOTHING resolved and the lexical spelling stands.
fn identity_spelling(abs: &Path) -> (PathBuf, bool) {
    let mut cur = abs;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(mut c) = std::fs::canonicalize(cur) {
            for t in tail.iter().rev() {
                c.push(t);
            }
            return (strip_verbatim(&c), false);
        }
        match (cur.parent(), cur.file_name()) {
            (Some(par), Some(name)) if !par.as_os_str().is_empty() => {
                tail.push(name);
                cur = par;
            }
            _ => return (abs.to_path_buf(), true),
        }
    }
}

/// Undo the `\\?\` verbatim prefix `fs::canonicalize` returns on Windows,
/// so the canonical key of a photo already spelled with its true casing on
/// a plain local drive is BYTE-IDENTICAL to its lexical key — the property
/// that keeps the overwhelming majority of existing develop dirs un-rekeyed.
/// Only explicitly-understood prefixes are rewritten; everything else is
/// left UNCHANGED — inventing a DOS spelling for e.g. `\\?\Volume{GUID}\`
/// could collide with a different photo's key, the exact cross-photo
/// clobber this module exists to prevent.
pub(super) fn strip_verbatim(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut comps = p.components();
        let Some(Component::Prefix(pre)) = comps.next() else { return p.to_path_buf() };
        let root: PathBuf = match pre.kind() {
            Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:\\", letter as char)),
            Prefix::VerbatimUNC(server, share) => {
                let mut s = std::ffi::OsString::from(r"\\");
                s.push(server);
                s.push(r"\");
                s.push(share);
                s.push(r"\");
                PathBuf::from(s)
            }
            _ => return p.to_path_buf(),
        };
        let mut out = root;
        for c in comps {
            match c {
                Component::RootDir => {}
                other => out.push(other),
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

/// Network/device identity gate for [`identity_of`]: the F4 lexical
/// prefixes PLUS mapped network drive letters — `GetDriveTypeW` is a local
/// mount-table lookup, so a `Z:` pointing at a NAS is caught without any
/// network I/O.
fn remote_identity(p: &Path) -> bool {
    remote_or_device_path(p) || remote_drive_letter(p)
}

#[cfg(windows)]
fn remote_drive_letter(p: &Path) -> bool {
    use std::path::{Component, Prefix};
    let letter = match p.components().next() {
        Some(Component::Prefix(pre)) => match pre.kind() {
            Prefix::Disk(l) | Prefix::VerbatimDisk(l) => l,
            _ => return false,
        },
        _ => return false,
    };
    const DRIVE_REMOTE: u32 = 4;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetDriveTypeW"]
        fn get_drive_type_w(root: *const u16) -> u32;
    }
    let root: [u16; 4] = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
    // SAFETY: NUL-terminated buffer, live across the synchronous call.
    (unsafe { get_drive_type_w(root.as_ptr()) }) == DRIVE_REMOTE
}

#[cfg(not(windows))]
fn remote_drive_letter(_: &Path) -> bool {
    false
}

/// What one resolved photo told the user about its aliased past — typed, so
/// the GUI renders it in the session language at consumption time (the
/// worker-closure i18n lesson), never a pre-formatted string.
pub enum AliasNote {
    /// Edits saved under an older (lexical) spelling were adopted into the
    /// canonical develop dir.
    Adopted { from: PathBuf },
    /// BOTH spellings hold a real develop: the canonical one is in use, the
    /// other was left untouched at this path (user-decided 2026-08-10:
    /// disclose, never merge — a wrong guess silently destroys a develop).
    SecondDevelop { at: PathBuf },
}

fn alias_notes() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, AliasNote>> {
    static NOTES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, AliasNote>>> =
        std::sync::OnceLock::new();
    NOTES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Pop the alias disclosure for this photo, if its first key resolution this
/// session produced one. The GUI's open path consumes it into a toast.
pub fn take_alias_note(src: &Path) -> Option<AliasNote> {
    let abs = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    alias_notes().lock().unwrap().remove(&abs)
}

fn stash_alias_note(src_abs: &Path, note: AliasNote) {
    alias_notes().lock().unwrap().insert(src_abs.to_path_buf(), note);
}

/// Per-process memo of resolved develop keys — (root, absolute photo path) →
/// key. The stability contract of [`identity_of`]: an entry drops only when
/// a locked touch finds its dir superseded by a concurrent adoption
/// ([`with_develop_lock_in`]).
#[allow(clippy::type_complexity)]
pub(super) fn resolved_keys() -> &'static std::sync::Mutex<std::collections::HashMap<(PathBuf, PathBuf), String>>
{
    static MEMO: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(PathBuf, PathBuf), String>>,
    > = std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub(super) fn forget_resolved_key(root: &Path, src: &Path) {
    let abs = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    resolved_keys().lock().unwrap().remove(&(root.to_path_buf(), abs));
}

/// Which key this photo's develop lives under IN THIS ROOT — the canonical
/// key, after a one-time adoption of anything a pre-canonical build saved
/// under the lexical key. Memoized per (root, photo) for the process
/// lifetime (the same stability contract as [`identity_of`]).
pub(super) fn resolve_key_in(root: &Path, src: &Path) -> String {
    let ck = photo_key(src);
    let lk = photo_key_lexical(src);
    if ck == lk {
        // The common case (plain local path, true casing) — but an ALIAS
        // session's crashed adoption may have left this very dir half-copied
        // behind its marker, and only the ck≠lk path below would resume it.
        // One memoized probe per (root, key) keeps the steady state at a
        // single exists() per photo per process.
        resume_orphan_adoption_once(root, &ck);
        return ck;
    }
    let abs = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    if let Some(hit) = resolved_keys().lock().unwrap().get(&(root.to_path_buf(), abs.clone())) {
        return hit.clone();
    }
    match adopt_or_choose(root, &abs, &ck, &lk) {
        Some(key) => {
            let mut m = resolved_keys().lock().unwrap();
            const MEMO_CAP: usize = 50_000;
            if m.len() < MEMO_CAP {
                m.entry((root.to_path_buf(), abs)).or_insert_with(|| key.clone());
            }
            key
        }
        // Undecidable RIGHT NOW (another process holds a lock): fall back
        // to the lexical key for THIS call, unmemoized, so the next touch
        // retries the resolution instead of freezing the fallback.
        None => lk,
    }
}

/// The fast-path half of crash resumption: a canonical-spelling session
/// finds `adopting-from.txt` left by an ALIAS session that crashed
/// mid-adoption, and finishes the copy from the source the marker records —
/// without this the half-copied dir is served as if it were complete.
/// WouldBlock is not memoized (the next touch retries); any other failure is
/// disclosed and memoized — a stable, disclosed degradation beats re-probing
/// a broken source on every touch. The memo is an OPTIMIZATION only (review
/// R12-01): the develop lock re-checks the fence on every locked touch, so a
/// "no marker" memoized before a concurrent adoption started cannot grant
/// the fenced dir authority.
fn resume_orphan_adoption_once(root: &Path, ck: &str) {
    use std::sync::{Mutex, OnceLock};
    static CHECKED: OnceLock<Mutex<std::collections::HashSet<(PathBuf, String)>>> =
        OnceLock::new();
    let checked = CHECKED.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    if checked.lock().unwrap().contains(&(root.to_path_buf(), ck.to_string())) {
        return;
    }
    let cd = root.join("develops").join(ck);
    if !cd.join("adopting-from.txt").exists() {
        checked.lock().unwrap().insert((root.to_path_buf(), ck.to_string()));
        return;
    }
    match resume_marked_adoption(&cd, DevelopLockMode::NoWait) {
        Ok(done) => {
            // Only the process that copied says so: one beaten to it by a
            // concurrent resume claimed the other's work until 2026-09-24.
            if done {
                eprintln!(
                    "⚠ finished a crashed adoption into {} (left by an aliased path spelling)",
                    cd.display()
                );
            }
            checked.lock().unwrap().insert((root.to_path_buf(), ck.to_string()));
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(e) => {
            eprintln!(
                "⚠ a crashed adoption into {} could not be finished ({e}) — files may be \
                 missing there until its adopting-from.txt source becomes readable",
                cd.display()
            );
            checked.lock().unwrap().insert((root.to_path_buf(), ck.to_string()));
        }
    }
}

/// The adoption marker holds ONE path and a newline, nothing else — so it gets
/// its own, far tighter ceiling rather than [`MAX_STORE_JSON`]'s 16 MiB.
/// Windows' extended path limit is 32,767 UTF-16 units, which cannot exceed
/// 128 KiB of UTF-8; 64 KiB is past any spelling a filesystem will actually
/// hand back and still refuses a planted gigabyte. Bounded for the same reason
/// every other read in this module is (R28 2a, adjudication F7's sibling): the
/// `<temp>/autoshade` fallback root is world-writable, so the file this reads
/// is not always one this app wrote.
const MAX_ADOPTION_MARKER: u64 = 64 * 1024;

/// Finish a marker-fenced adoption whose source is read from the marker
/// itself — a resume must copy from the dir the CRASHED attempt was copying,
/// never from whatever spelling the current session happens to hold: with
/// three spellings in play, mixing the two merges two generations into one
/// dir.
///
/// `mode` is the CALLER's: the locked-touch gate hands down its own, so a
/// Wait surface (CLI, server, worker thread) queues behind a resume another
/// process is running instead of failing its touch with a WouldBlock, while
/// the badge-fill probe stays NoWait (a held lock postpones, never hangs).
/// The marker is re-read UNDER both locks (the `adopt_or_choose` rule — the
/// pre-lock read raced every other process): a resume that finds it gone
/// was beaten to the finish and copies NOTHING. `adopt_files` SYNCHRONIZES
/// the destination to the source, so a second run over a completed adoption
/// would replace a save that landed in the meantime with the superseded
/// source's bytes. Answers whether THIS call did the copying — `false` when
/// a concurrent resume finished first — so a caller reports its own work.
pub(super) fn resume_marked_adoption(cd: &Path, mode: DevelopLockMode) -> std::io::Result<bool> {
    let marker = cd.join("adopting-from.txt");
    let recorded = read_text_capped(&marker, MAX_ADOPTION_MARKER)?;
    let source = PathBuf::from(recorded.trim());
    if source.as_os_str().is_empty() {
        return Err(std::io::Error::other("adopting-from.txt names no source"));
    }
    with_path_lock(source.join(".develop.lock"), mode, || {
        with_path_lock(cd.join(".develop.lock"), mode, || {
            match read_text_capped(&marker, MAX_ADOPTION_MARKER) {
                Ok(now) if source == Path::new(now.trim()) => adopt_files(cd, &source).map(|()| true),
                // Finished by a concurrent resume while this one waited for
                // the locks: the dir is whole and unfenced — nothing to copy.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                // A newer attempt (from yet another spelling) crashed in the
                // window: its source is not the one these locks cover.
                Ok(_) => Err(std::io::Error::other(
                    "adopting-from.txt names a different source than it did before the locks \
                     were taken — the resume is retried on the next touch",
                )),
                Err(e) => Err(e),
            }
        })
    })
}

/// Names that are per-dir machinery, never a develop's content — excluded
/// from adoption copies and from the "is there anything here" probes.
pub(super) fn adoption_skips(name: &str) -> bool {
    name == ".develop.lock"
        || name == "superseded-by.txt"
        || name == "adopting-from.txt"
        || name == "adopted-from.txt"
        || name.contains(".tmp.")
}

/// The one-time adoption decision for a photo whose canonical and lexical
/// keys differ. Returns the key to use, or `None` when a lock could not be
/// taken without waiting (retry next call). All-or-nothing: two dirs that
/// BOTH hold a real develop are never merged file-by-file — a no-clobber
/// union of two generations is a franken-develop.
pub(super) fn adopt_or_choose(root: &Path, abs: &Path, ck: &str, lk: &str) -> Option<String> {
    let cd = root.join("develops").join(ck);
    let ld = root.join("develops").join(lk);
    // NotFound is the one honest "empty"; any other enumeration error must
    // not make an EXISTING develop look empty (review R12-02) — pre-lock
    // callers key lexically and retry, the in-lock recheck propagates.
    let contentful = |d: &Path| -> std::io::Result<bool> {
        let it = match std::fs::read_dir(d) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        for e in it {
            if !adoption_skips(&e?.file_name().to_string_lossy()) {
                return Ok(true);
            }
        }
        Ok(false)
    };
    // Already superseded by an earlier session's adoption — nothing to redo.
    if ld.join("superseded-by.txt").exists() {
        return Some(ck.to_string());
    }
    let resume = cd.join("adopting-from.txt").exists();
    // A resumed adoption copies from the source the MARKER records, never
    // from this session's spelling: with three spellings in play, copying
    // from the current one would mix two generations into one dir.
    let source = if resume {
        // Capped like the resume path above; an over-cap marker takes the
        // same "cannot be read" branch a corrupt one already took.
        match read_text_capped(&cd.join("adopting-from.txt"), MAX_ADOPTION_MARKER) {
            Ok(s) if !s.trim().is_empty() => PathBuf::from(s.trim()),
            _ => {
                eprintln!(
                    "⚠ {} holds an adoption marker whose source cannot be read — this session \
                     keys the photo by its path spelling and the resume retries later",
                    cd.display()
                );
                return Some(lk.to_string());
            }
        }
    } else {
        ld.clone()
    };
    if !resume {
        let (ld_full, cd_full) = match (contentful(&ld), contentful(&cd)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!(
                    "⚠ probing the develops for {} failed ({e}) — this session keys the photo \
                     by its path spelling and the adoption retries later",
                    abs.display()
                );
                return Some(lk.to_string());
            }
        };
        if !ld_full {
            // Fresh photo (or only lock litter): the canonical key, no copy.
            return Some(ck.to_string());
        }
        if cd_full {
            // GENUINE collision: both spellings hold a develop and no
            // adoption was in flight. Canonical wins, the alias stays on
            // disk untouched, and the fact is durable + surfaced (once).
            let marker = cd.join("aliased-develops.txt");
            if !marker.exists() {
                let _ = durable_write(
                    &marker,
                    format!("a second develop for this photo exists at:\n{}\n", ld.display())
                        .as_bytes(),
                );
                eprintln!(
                    "⚠ {} has a second saved develop at {} from an older path spelling — it was NOT merged",
                    abs.display(),
                    ld.display()
                );
                stash_alias_note(abs, AliasNote::SecondDevelop { at: ld.clone() });
            }
            return Some(ck.to_string());
        }
        // A pending transaction in the alias dir means its true content is
        // not settled — and the recovery helpers all derive their paths from
        // the PHOTO, which now resolves canonically, so they cannot be
        // pointed at the alias dir. Defer: this session keys lexically (the
        // pre-upgrade behaviour), the residue settles in place through
        // normal use, and the NEXT session adopts. Self-healing across two
        // sessions instead of a half-adopted transaction.
        let residue = ld.join("clear.pending").exists()
            || ld.join(".commit").exists()
            || std::fs::read_dir(&ld).ok().is_some_and(|it| {
                it.flatten().any(|e| {
                    e.file_name().to_string_lossy().starts_with(".deleting.v")
                })
            })
            || ["recipe.json", "pixels.json", "variants.json"].iter().any(|n| {
                !ld.join(n).exists() && ld.join(format!("{n}.bak")).exists()
            });
        if residue {
            eprintln!(
                "⚠ {} has unsettled develop state under its older path spelling ({}) — adoption \
                 into the canonical develop is postponed until it settles",
                abs.display(),
                ld.display()
            );
            return Some(lk.to_string());
        }
    }
    // Adopt (or resume a crashed adoption), under BOTH dir locks in a fixed
    // source→canonical order (total and process-independent, so two
    // processes cannot deadlock; concurrent adopters are fully serialized by
    // the two locks and re-validate before copying). with_path_lock
    // directly — with_develop_lock would re-derive the key and recurse into
    // this very function. NoWait on both: this can run on the UI thread
    // (badge fill), and a held lock postpones, never hangs.
    enum AdoptOutcome {
        Copied,
        AlreadyAdopted,
        Collision,
    }
    let adopted: std::io::Result<AdoptOutcome> = (|| {
        std::fs::create_dir_all(&cd)?; // the lock file needs its dir
        with_path_lock(source.join(".develop.lock"), DevelopLockMode::NoWait, || {
            with_path_lock(cd.join(".develop.lock"), DevelopLockMode::NoWait, || {
                // The pre-lock probes raced every other process: re-validate
                // the decision they fed now that both locks are held, and
                // only then copy.
                if source.join("superseded-by.txt").exists() {
                    // A concurrent adopter finished first — converge on its
                    // result instead of copying over it.
                    return Ok(AdoptOutcome::AlreadyAdopted);
                }
                if !cd.join("adopting-from.txt").exists() && contentful(&cd)? {
                    // cd gained real content between probe and lock: a
                    // GENUINE collision now — copying would franken-merge
                    // two develops.
                    return Ok(AdoptOutcome::Collision);
                }
                adopt_files(&cd, &source)?;
                Ok(AdoptOutcome::Copied)
            })
        })
    })();
    match adopted {
        Ok(AdoptOutcome::Copied) => {
            stash_alias_note(abs, AliasNote::Adopted { from: source.clone() });
            eprintln!(
                "⚠ adopted the develop saved under {} into {} (the photo resolves there)",
                source.display(),
                cd.display()
            );
            Some(ck.to_string())
        }
        Ok(AdoptOutcome::AlreadyAdopted) => Some(ck.to_string()),
        Ok(AdoptOutcome::Collision) => {
            let marker = cd.join("aliased-develops.txt");
            if !marker.exists() {
                let _ = durable_write(
                    &marker,
                    format!("a second develop for this photo exists at:\n{}\n", source.display())
                        .as_bytes(),
                );
                eprintln!(
                    "⚠ {} has a second saved develop at {} from an older path spelling — it was NOT merged",
                    abs.display(),
                    source.display()
                );
                stash_alias_note(abs, AliasNote::SecondDevelop { at: source.clone() });
            }
            Some(ck.to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(e) => {
            // Memoized by the caller ON PURPOSE: a stable lexical session
            // beats a key that flaps between spellings, and the one harmful
            // case — this lexical dir later superseded by a concurrent
            // adopter — is caught inside the develop lock, which probes the
            // marker and re-resolves.
            eprintln!(
                "⚠ adopting the develop at {} into {} failed ({e}) — this session keys the \
                 photo by its path spelling and the adoption retries later",
                source.display(),
                cd.display()
            );
            Some(lk.to_string())
        }
    }
}

/// The copy half of adoption, marker-fenced like every other multi-file
/// mutation in this store: `adopting-from.txt` lands durably FIRST, the
/// files copy, the completion breadcrumbs land, and only then is the
/// in-flight marker consumed. While the marker fences the destination the
/// destination has NO authority, so copies are SOURCE-WINS: a crashed
/// earlier attempt may have frozen older source bytes there, and no-clobber
/// would keep those over edits the session made under the lexical key after
/// the failure. Concurrent adopters run serialized under both dir locks and
/// copy the same source bytes, so replacement still converges. The source
/// dir is left INTACT as a frozen backup with a `superseded-by.txt` pointer
/// — adoption copies, it never deletes.
fn adopt_files(cd: &Path, source: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cd)?;
    durable_write(
        &cd.join("adopting-from.txt"),
        format!("{}\n", source.display()).as_bytes(),
    )?;
    let copy_dir = |from: &Path, to: &Path| -> std::io::Result<()> {
        for e in std::fs::read_dir(from)? {
            // A swallowed enumeration error is a file that never adopts: the
            // completion breadcrumbs + supersede marker would freeze it
            // invisible in the frozen backup forever — so it fails the
            // attempt whole and lands in the caller's retry arm instead.
            let e = e?;
            let name = e.file_name();
            let name_s = name.to_string_lossy();
            if adoption_skips(&name_s) {
                continue;
            }
            let src = from.join(&name);
            if src.is_dir() {
                if name_s == ".legacy-suppressed" {
                    std::fs::create_dir_all(to.join(&name))?;
                    for c in std::fs::read_dir(&src)? {
                        let c = c?;
                        copy_file_replace(&c.path(), &to.join(&name).join(c.file_name()))?;
                    }
                } else {
                    eprintln!(
                        "⚠ adoption skipped unknown directory {} — it stays under the old spelling",
                        src.display()
                    );
                }
                continue;
            }
            copy_file_replace(&src, &to.join(&name))?;
        }
        Ok(())
    };
    copy_dir(source, cd)?;
    // SYNCHRONIZE, not overlay (review R12-02): while the marker fences the
    // destination it has NO authority, and that includes members the source
    // no longer has — a clear made under the still-authoritative alias
    // between attempts must not be resurrected by a stale destination copy
    // that the supersede below would then freeze as truth.
    for e in std::fs::read_dir(cd)? {
        let e = e?;
        let name = e.file_name();
        let name_s = name.to_string_lossy();
        if adoption_skips(&name_s) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            continue; // unknown dirs stay under disclosure, as on the copy side
        }
        if !source.join(&name).is_file() {
            std::fs::remove_file(&p)?;
        }
    }
    durable_write(&cd.join("adopted-from.txt"), format!("{}\n", source.display()).as_bytes())?;
    durable_write(
        &source.join("superseded-by.txt"),
        format!("{}\n", cd.display()).as_bytes(),
    )?;
    // The fence must CONSUME or FAIL (the clear-marker rule, review
    // R12-01): reporting success with the marker still on disk leaves the
    // dir fenced — every later locked touch re-runs the (idempotent) resume
    // and the session believes the adoption completed.
    match std::fs::remove_file(cd.join("adopting-from.txt")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(std::io::Error::new(
                e.kind(),
                format!("the finished adoption could not consume its marker ({e})"),
            ));
        }
    }
    settle_consumed_marker(&cd.join("adopting-from.txt"));
    Ok(())
}

/// Stage-and-replace copy for adoption members (see [`adopt_files`] — the
/// destination is marker-fenced and has no authority while this runs).
fn copy_file_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let tmp = sibling_tmp(to);
    if let Err(e) = std::fs::copy(from, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    durable_replace(&tmp, to).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
