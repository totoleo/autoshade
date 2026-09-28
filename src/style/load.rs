//! Loading: the effective index, the index info and state, and neighbour stems.

use super::*;

/// Which index file answered, and whether one could be used at all.
///
/// ONE loader for every surface (R23-2): the CLI, the web handler, the GUI's
/// status line and `pipeline::produce_recipe` used to each spell the
/// central-then-legacy walk themselves, and the pipeline's copy additionally
/// decided "is this worth telling the user about?" from `central.exists()` —
/// which is why a fresh install got a Style slider that silently did nothing.
/// The three states here are exactly the three answers a surface needs.
pub enum EffectiveIndex {
    /// A usable index, and the file it came from (absolute).
    Loaded(StyleIndex, std::path::PathBuf),
    /// No index file anywhere — nothing has been built yet.
    Absent,
    /// A file EXISTS but cannot be used (version gate, corruption, size cap,
    /// permissions). `err` is the loader's own message chain.
    Unusable { path: std::path::PathBuf, err: String },
}

/// The user's style index: the central store first, the legacy cwd-relative
/// file as a fallback (see [`LEGACY_INDEX_PATH`]).
pub fn load_effective() -> EffectiveIndex {
    load_effective_at(&crate::store::style_index_path(), Path::new(LEGACY_INDEX_PATH))
}

/// [`load_effective`] against explicit paths — the seam the tests drive (the
/// real one reads a per-user store location that no test may depend on).
pub fn load_effective_at(central: &Path, legacy: &Path) -> EffectiveIndex {
    let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    // Both loads are attempted before anything is CLASSIFIED: a corrupt
    // central file with a good legacy one beside it must still serve the
    // legacy index, exactly as it did before this function existed.
    let central_err = match StyleIndex::load(central) {
        Ok(ix) => return EffectiveIndex::Loaded(ix, abs(central)),
        Err(e) => e,
    };
    let legacy_err = match StyleIndex::load(legacy) {
        Ok(ix) => return EffectiveIndex::Loaded(ix, abs(legacy)),
        Err(e) => e,
    };
    // Nothing answered. "A file exists but cannot be used" and "no library at
    // all" are DIFFERENT facts: the first deserves the loader's error (it
    // carries the version-gate rebuild instruction), the second is a fresh
    // install that needs an entry point, not an error message.
    if central.exists() {
        EffectiveIndex::Unusable { path: abs(central), err: format!("{central_err:#}") }
    } else if legacy.exists() {
        EffectiveIndex::Unusable { path: abs(legacy), err: format!("{legacy_err:#}") }
    } else {
        EffectiveIndex::Absent
    }
}

/// Everything a UI shows ABOUT the style library, as typed facts — the
/// shared read behind the web's `/api/style-info` and the GUI's status line
/// (R23-2: the GUI had no production-side entry at all, and the display logic
/// existed only inside the web handler).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleIndexInfo {
    /// The file that answered — or, when none did, the central path a build
    /// would write.
    pub path: std::path::PathBuf,
    pub state: StyleIndexState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StyleIndexState {
    Built {
        /// How many of the user's own edits it holds.
        total: usize,
        version: u32,
        /// The folder it was built FROM (`None` for indexes written before
        /// the field existed).
        source_dir: Option<String>,
        /// The most common scene tags, most-frequent first, at most 6.
        scenes: Vec<(String, usize)>,
        /// Number of RAW exemplars carrying a valid SigLIP vector.
        with_embedding: usize,
        /// Number of finished-photo look records in the separate library.
        looks: usize,
        looks_dir: Option<String>,
        /// How long ago the file was written, measured at read time. A
        /// RELATIVE age on purpose: this tree carries no calendar library, and
        /// "built 3 days ago" is the question a photographer is asking anyway.
        age: Option<std::time::Duration>,
    },
    /// Nothing built yet.
    Absent,
    /// A file exists but cannot be used — the loader's message.
    Unusable { err: String },
}

/// Read the style library's status (see [`StyleIndexInfo`]).
pub fn index_info() -> StyleIndexInfo {
    index_info_at(&crate::store::style_index_path(), Path::new(LEGACY_INDEX_PATH))
}

/// [`index_info`] against explicit paths — the tested seam.
pub fn index_info_at(central: &Path, legacy: &Path) -> StyleIndexInfo {
    match load_effective_at(central, legacy) {
        EffectiveIndex::Loaded(ix, path) => {
            let mut tags: BTreeMap<&str, usize> = BTreeMap::new();
            for e in &ix.exemplars {
                *tags.entry(e.tag.as_str()).or_default() += 1;
            }
            let mut scenes: Vec<(String, usize)> =
                tags.into_iter().map(|(t, n)| (t.to_string(), n)).collect();
            // Count first, then the tag itself: a pure count sort left ties in
            // an order that could differ between two reads of the same file.
            scenes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            scenes.truncate(6);
            let age = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok());
            let state = StyleIndexState::Built {
                total: ix.exemplars.len(),
                version: ix.version,
                source_dir: ix.source_dir.clone(),
                scenes,
                age,
                with_embedding: ix.exemplars.iter().filter(|e| e.embed.is_some()).count(),
                looks: ix.looks.len(),
                looks_dir: ix.looks_dir.clone(),
            };
            StyleIndexInfo { path, state }
        }
        EffectiveIndex::Absent => StyleIndexInfo {
            path: std::path::absolute(central).unwrap_or_else(|_| central.to_path_buf()),
            state: StyleIndexState::Absent,
        },
        EffectiveIndex::Unusable { path, err } => {
            StyleIndexInfo { path, state: StyleIndexState::Unusable { err } }
        }
    }
}

/// The one FOLDER name that tells two same-named photographs apart, or `None`
/// when this exemplar has no path to read one from.
///
/// The immediate parent and nothing above it. A camera counter wraps at 9999
/// and starts again, so two rolls hold two `00001234` and the folder they sit
/// in is the shortest thing that distinguishes them — while the folder's
/// PARENT is the layout of the user's disk, which a persisted, displayed
/// rationale has no business carrying.
fn parent_hint(path: Option<&str>) -> Option<String> {
    let name = Path::new(path?).parent()?.file_name()?.to_string_lossy().into_owned();
    (!name.is_empty()).then_some(name)
}

/// The file names of the exemplars ONE retrieval actually used — the answer
/// to "which library is it referencing, and which shots?" (feedback #6, the
/// user's stated top pain: the reference was invisible).
///
/// Stems only, never full paths: the rationale is persisted and shown in
/// three UIs, and the folder layout is not the point. Bounded on both axes
/// ([`MAX_DISCLOSED_NEIGHBOURS`], [`MAX_STEM_CHARS`]) so a long-named library
/// cannot crowd out the rest of the rationale.
///
/// A stem SHARED by two of the disclosed neighbours gets its folder in front
/// of it (A27). Every keyed surface already tells those two apart by path —
/// self-exclusion, the exemplar cache, the neighbour ranking — and this
/// sentence was the one place that did not: the user's own 169-exemplar
/// library holds four eight-digit camera counters carried by two photographs
/// each, and "the 4 most similar" then named the same shot twice with no way
/// to tell which two frames answered. The hint is added ONLY to the names it
/// disambiguates, so a library with no collision reads exactly as it did.
pub fn neighbour_stems(ex: &[&StyleExemplar]) -> Vec<String> {
    let shown: Vec<&&StyleExemplar> = ex.iter().take(MAX_DISCLOSED_NEIGHBOURS).collect();
    shown
        .iter()
        .map(|e| {
            let shared = shown.iter().filter(|o| o.stem == e.stem).count() > 1;
            let name = match shared.then(|| parent_hint(e.path.as_deref())).flatten() {
                Some(folder) => format!("{folder}/{}", e.stem),
                None => e.stem.clone(),
            };
            // The cut is the one this disclosure has always made: the cap in
            // CHARACTERS, with an ellipsis after it, so a reader can tell a
            // cut name from one that merely stops.
            let mut s: String = name.chars().take(MAX_STEM_CHARS).collect();
            if s.chars().count() < name.chars().count() {
                s.push('…');
            }
            s
        })
        .collect()
}
