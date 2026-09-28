//! Exemplars: the style and look records, embedding distances, the staged frame, the intermediates sweep, and description embeddings.

use super::*;

#[derive(Serialize, Deserialize, Clone)]
pub struct StyleExemplar {
    pub stem: String,
    pub feat: Vec<f32>,
    pub tag: String,
    pub settings: BTreeMap<String, f32>,
    /// The user's master tone-curve shape `[black_lift, s_strength]` (0..255
    /// scale), if they drew one — the curve "habit" the flat sliders can't carry.
    /// `#[serde(default)]` keeps v1 index files loadable.
    #[serde(default)]
    pub curve: Option<[f32; 2]>,
    /// Absolute source path — self-exclusion identity (two rolls can hold two
    /// DSC00001.ARW, and stem-based exclusion dropped BOTH whenever either
    /// was being edited). `#[serde(default)]`: older indexes lack it and fall
    /// back to stem exclusion, over-exclusion being the safe direction.
    #[serde(default)]
    pub path: Option<String>,
    /// How hard this user pushes the colour FAMILIES the flat `settings` map
    /// cannot carry — the HSL mixer, the grade wheels, the per-channel curves
    /// (R23-1, feedback #12: the reference block was blind to all three, so
    /// the AI had no signal about a photographer who shapes colour per band).
    /// Summary statistics, not the 38 keys — see [`crate::eval::FamilySummary`].
    ///
    /// `#[serde(default)]` keeps every pre-R23 index loadable, and it is an
    /// ADDED field only: no existing key changes meaning, so the index version
    /// deliberately does NOT bump (a bump forces every user to rebuild an hour-
    /// long index for a field that degrades to "no summary line").
    #[serde(default)]
    pub families: Option<crate::eval::FamilySummary>,
    /// SigLIP 2 image embedding — 768 dims, L2-normalised, produced by
    /// `python/embed.py` (R27 Batch-5, index v5).
    ///
    /// **Beside the 14-dim feature, never fused into it.** The hand feature is
    /// EXIF + histogram: focal length, hour of day, clipping, warmth. The
    /// embedding is what the frame LOOKS like. They answer different questions
    /// and they are on incomparable scales, so the distance keeps them as two
    /// blocks with their own weights ([`embed_distance`]) rather than
    /// concatenating them into one vector where `WEIGHTS` would stop meaning
    /// anything.
    ///
    /// Deliberately NOT z-scored, unlike [`ZSCORE_DIMS`]: a per-dim mean and
    /// σ over ~150 exemplars is a degenerate estimate at 768 dims, and
    /// `normalize`'s `(v−mean)/σ` would amplify whichever dims happened to be
    /// flat. A cosine over unit vectors needs no per-dim statistics at all,
    /// which is the second reason the two blocks stay separate.
    ///
    /// `#[serde(default)]` and `Option`, so this is optional in BOTH
    /// directions: a v4 index has none (and the cosine term contributes
    /// nothing), and a v5 index built without the sidecar has none either.
    /// Retrieval must tolerate an index built either way — that is the
    /// contract, not a fallback.
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub vocab_scores: Option<Vec<f32>>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub desc_embed: Option<Vec<f32>>,
    /// What this photographer's LOCAL work looks like on this shot — how many
    /// masks, put to which use, and the amount-weighted mean of eight sliders
    /// per use (S3). Summary statistics, never geometry: see
    /// [`crate::mask_habit`] for why no coordinate is ever averaged.
    ///
    /// `Option`, optional in BOTH directions like [`StyleExemplar::families`]
    /// before it: a pre-S3 index has none, and the
    /// reference block then renders exactly as it did
    /// (`reference_local_work_note_is_absent_when_no_neighbour_carries_masks`).
    /// `None` means NOT MEASURED, which is a different fact from a measured
    /// `count: 0` — see [`crate::mask_habit::MaskHabit`].
    ///
    /// KNOWN BOUNDARY, inherited rather than introduced: a sidecar whose `crs`
    /// prefix is bound to a foreign namespace imports as FULLY NEUTRAL
    /// (`xmp::xmp_to_recipe_clamped_impl` refuses it outright), so such a
    /// document lands here as a measured `count: 0`. It lands in
    /// [`StyleExemplar::settings`] as an EMPTY map for exactly the same reason
    /// — `read_settings` reads through the same hijacked prefix — so the two
    /// halves of the exemplar agree, and the reference block shows a neighbour
    /// with no sliders at all rather than a plausible one.
    ///
    /// The index version deliberately does NOT bump for this, on
    /// [`StyleExemplar::families`]' own precedent and for its reason: the
    /// version gate exists for a change in what the FOURTEEN FEATURES mean or
    /// in how candidates are RANKED (`CURRENT_INDEX_VERSION`), and this field
    /// touches neither — nothing here is read by `score_candidates`
    /// (`retrieval_does_not_read_mask_habits`). A bump would force every user
    /// to rebuild an hour-long index for a field that degrades to "no
    /// local-work line".
    ///
    /// SINCE BATCH 2 it is also read by [`style_targets`], which distils a
    /// mask's SLIDER AMPLITUDES toward this photographer's per-use habit. That
    /// does not disturb the paragraph above — the ranking is still blind to it
    /// — and an index without the field still degrades to "no target", but it
    /// does mean the old claim that `blend_toward` never reads a habit is no
    /// longer true, and a comment that says so would be the kind of lie this
    /// batch was opened to remove.
    ///
    /// The `#[serde(default)]` below is CONSISTENCY with the eleven optional
    /// fields above it, not the load-bearing part: serde reads a missing
    /// `Option` field as `None` with or without it (measured — M-S3-M removed
    /// the attribute and `a_pre_s3_index_reads_with_no_mask_habit` stayed
    /// green). What keeps the two states apart is that nothing ever writes a
    /// default INTO the field: `load` clamps a habit that is there and
    /// fabricates none that is not.
    #[serde(default)]
    pub masks: Option<crate::mask_habit::MaskHabit>,
    /// Did the photographer convert this frame to BLACK AND WHITE?
    ///
    /// Read from the sidecar's treatment ([`read_monochrome`]) and not from
    /// any slider, because there is no slider: a grayscale conversion leaves
    /// `crs:Saturation` and every `crs:SaturationAdjustment*` where they were,
    /// so an index that reads only sliders cannot tell a black-and-white
    /// photograph from a colour one that happens to have an untouched mixer.
    /// The consequence is [`style_targets`]' — a zero counted as agreement
    /// drags a band target toward nothing — which is why the fact is stored
    /// rather than guessed at distillation time.
    ///
    /// `#[serde(default)]` on the eleven optional fields above's precedent: a
    /// pre-v6 index carries none, every exemplar in it reads `false`, and the
    /// distillation is exactly the one it was. The next build fills it in.
    #[serde(default)]
    pub mono: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LookExemplar {
    pub stem: String,
    pub path: String,
    pub embed: Vec<f32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub vocab_scores: Option<Vec<f32>>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub desc_embed: Option<Vec<f32>>,
}

/// The embedding half of the retrieval distance: `W_EMB · (1 − cos(q, e))`,
/// or `0.0` when either side has no vector.
///
/// Both sides are L2-normalised by the sidecar (and re-checked at the door by
/// [`crate::embed::parse_vector`] / [`exemplar_is_finite`]), so the cosine is a
/// plain dot product. Accumulated in f64 for the same reason the 14-dim block
/// is: the ranking must be deterministic, and `total_cmp` can only order keys
/// that were computed the same way every time.
///
/// A width mismatch answers 0 rather than truncating: two vectors of different
/// widths are not comparable, and silently comparing their common prefix is
/// how an index that mixed models would produce a plausible, wrong ranking.
pub(super) fn embed_distance(q: Option<&[f32]>, e: Option<&[f32]>, w: f64) -> f64 {
    cosine_gap(q, e).map_or(0.0, |gap| w * gap)
}

/// `1 − cos(q, e)`, or `None` when the pair is NOT COMPARABLE — either side
/// missing, or two different widths (see [`embed_distance`]).
///
/// The `None` is the load-bearing part: it is what lets the standardisation
/// above distinguish "this candidate scored badly" from "this candidate was
/// never measured", which a bare 0.0 cannot.
pub(super) fn cosine_gap(q: Option<&[f32]>, e: Option<&[f32]>) -> Option<f64> {
    let (Some(q), Some(e)) = (q, e) else { return None };
    if q.len() != e.len() || q.is_empty() {
        return None;
    }
    let dot: f64 = q.iter().zip(e).map(|(&a, &b)| a as f64 * b as f64).sum();
    // Unit vectors put the dot in [-1, 1]; the clamp is against the ~1e-7
    // round-trip slack the JSON text carries, not against a real value.
    Some(1.0 - dot.clamp(-1.0, 1.0))
}

/// One photo's embedding frame, staged on disk and OWNED: dropping it removes
/// both temp files.
///
/// It exists so the ~181 MB preview and the multi-second sidecar call stop
/// overlapping (R28 Batch-4 4b). `embed_preview` did both in one call, which
/// meant the caller's decode-sized buffer — and, in `StyleIndex::build`, its
/// `DecodePermit` — stayed alive for the whole model load. Staging is a
/// 512x512 PNG; once it exists, the frame the sidecar needs is 200 KB on disk
/// rather than 181 MB in RAM.
///
/// RAII rather than the two `remove_file` calls it replaces: the staged files
/// are intermediates, and every early return out of the embed path used to be
/// a chance to leak one into the user's temp directory.
pub struct StagedFrame {
    pub(super) img: PathBuf,
    pub(super) json: PathBuf,
}

impl StagedFrame {
    /// The staged PNG a sidecar reads. Exposed since S2, when the model calls
    /// moved OUT of the per-photo loop: a batch door takes a manifest of
    /// paths, so the frame has to outlive the worker that staged it.
    pub fn image(&self) -> &Path {
        &self.img
    }

    /// …and the same path as the string a JSONL manifest carries. One
    /// spelling, because the answer is mapped back by this exact text.
    pub fn image_path(&self) -> String {
        self.img.display().to_string()
    }
}

impl Drop for StagedFrame {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.img);
        let _ = std::fs::remove_file(&self.json);
    }
}

/// Reduce a camera preview to THE embedding frame and write it out.
///
/// The reduction is the reason both the index build and the develop-time query
/// go through one function: the sidecar squashes whatever it is given to
/// 384x384, so the only way a photo's stored vector and its query vector could
/// differ is if the two paths handed it different pixels.
pub fn stage_embed_frame(
    preview: &image::DynamicImage,
    dir: &Path,
    tag: &str,
) -> Result<StagedFrame> {
    // `Triangle` is bilinear, NAMED rather than defaulted for the same reason
    // the sidecar names PIL's resample: a filter that changed under us would
    // move every vector without a line of this repo changing.
    let small = preview.resize(EMBED_FRAME_EDGE, EMBED_FRAME_EDGE, image::imageops::FilterType::Triangle);
    // pid + seq + tag: two workers (or two processes) must never share one
    // name. The pid+tag pair was NOT enough — `pipeline::produce_recipe` passes
    // the constant tag "query" for every photo, so the moment two develops ran
    // concurrently in one process (the batch pool, which has shipped at three
    // workers since R26; the web server's request threads) they staged into the
    // same `autoshade-embed-<pid>-query.png` and the same `.json`: one worker
    // embedded the other's frame and got a style vector for the wrong
    // photograph, and either one's cleanup deleted the other's file mid-run.
    // The seq belongs HERE rather than at the call site — that is the fix that
    // holds for every caller, including the next one (`StyleIndex::build`
    // already passed a unique `idx-{i}` and was never affected).
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem = format!(
        "autoshade-embed-{}-{}-{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        tag
    );
    // Constructed BEFORE the write, so a failed `save` still cleans up the
    // partial file it may have created.
    let staged =
        StagedFrame { img: dir.join(format!("{stem}.png")), json: dir.join(format!("{stem}.json")) };
    small
        .to_rgb8()
        .save(&staged.img)
        .with_context(|| format!("stage embedding input {}", staged.img.display()))?;
    Ok(staged)
}

/// The scratch-file prefixes a build writes beside the index — every one of
/// them an INTERMEDIATE that exists only while the build runs.
///
/// `autoshade-embed-` covers both the staged frames and the description-text
/// manifests (`autoshade-embed-desc-…`), which is why the list is prefixes and
/// not names.
pub(super) const INTERMEDIATE_PREFIXES: [&str; 3] =
    ["autoshade-embed-", "autoshade-describe-", "autoshade-look-vocab-"];

/// How long an intermediate has to have been sitting there before a build
/// treats it as ABANDONED rather than as another build's live working file.
///
/// A day. The longest a staged frame can legitimately live is one build — it
/// is written in stage 1 and dropped after stage 4 — and the biggest library
/// this app admits is 5,000 RAWs, which is hours rather than a day even with
/// both sidecars. A shorter window would risk deleting a frame out from under
/// a slow build on someone else's machine, and the cost of waiting is a few
/// hundred KB of dead PNGs for one extra day.
pub(super) const STALE_INTERMEDIATE_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Delete the intermediates an EARLIER build left behind, and say how many.
///
/// [`StagedFrame`] removes its own two files on drop, which covers every path
/// out of a build that RETURNS — including the error paths, which is what the
/// RAII was for. What it cannot cover is a process that never unwinds: a
/// power cut, a `Stop-Process` on a build the user got tired of, the GUI being
/// killed mid-index. Those frames were then immortal, because nothing else in
/// the tree ever looked at the store for them — 200 KB per photograph of a
/// library, accumulating for the life of the installation.
///
/// Never OUR pid, whatever the age: the names carry the writing process's id
/// precisely so that two concurrent builds (the web server's request threads,
/// the GUI beside a CLI run) cannot mistake each other's files for rubbish,
/// and a sweep that skipped that test would be the accumulation defect
/// rewritten as a data race.
pub(super) fn sweep_stale_intermediates(dir: &Path, older_than: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mine = format!("-{}-", std::process::id());
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !INTERMEDIATE_PREFIXES.iter().any(|p| name.starts_with(p)) || name.contains(&mine) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|age| age >= older_than).unwrap_or(false))
            .unwrap_or(false);
        if old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// [`sweep_stale_intermediates`] at the shipped age, with the one line of
/// disclosure a build owes for deleting a file it did not write this run.
pub(super) fn sweep_intermediates_and_say(dir: &Path, what: &str) {
    let removed = sweep_stale_intermediates(dir, STALE_INTERMEDIATE_AGE);
    if removed > 0 {
        println!("  {what}: removed {removed} staged file(s) left by an interrupted build");
    }
}

/// The name one LIBRARY goes by in the description cache.
///
/// One spelling for both builders, because the RAW library and the look
/// library share one cache file and its cap is shared out between them by this
/// key (`crate::describe::DescriptionCache::retain`). The folder is resolved
/// the way [`StyleIndex::source_dir`] and [`StyleIndex::looks_dir`] already
/// resolve it, and then digested — a path that cannot be made absolute is used
/// as given, because a relative root is still a root and two of them that
/// differ only by the working directory cost a re-description, never a wrong
/// description.
pub(super) fn library_key(dir: &Path) -> String {
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    crate::describe::library_key(&abs)
}

/// The CONTENT key of every staged frame in a build, computed ONCE.
///
/// Three consumers key on it — the exemplar cache
/// ([`crate::style_cache`]), the description cache and the description
/// stage's hit/miss split — and it used to be computed inside the third,
/// which is why the first two could not exist. A frame that cannot be hashed
/// gets `None`: it is measured again next build, and says so.
pub(super) fn frame_digests(frames: &[Option<StagedFrame>], what: &str) -> Vec<Option<String>> {
    frames
        .iter()
        .map(|f| {
            let f = f.as_ref()?;
            match crate::describe::frame_digest(f.image()) {
                Ok(d) => Some(d),
                Err(e) => {
                    eprintln!(
                        "  {what}: one frame could not be hashed ({e:#}) — it is measured again \
                         next build"
                    );
                    None
                }
            }
        })
        .collect()
}

pub fn embed_staged_record(opts: &crate::embed::EmbedOpts, staged: &StagedFrame) -> Result<crate::embed::EmbedRecord> {
    crate::embed::embed_file_record(opts, &staged.img, &staged.json)
}

/// The phrase-list scratch file for ONE builder in ONE process.
///
/// Named by `who` as well as the pid because both builders wrote
/// `autoshade-look-vocab-<pid>.txt` and both DELETED it when finished: two
/// builds in one process (the web server's request threads) shared a path, and
/// whichever finished first took the other's vocabulary out from under it. A
/// sequence number covers the same builder running twice.
pub(super) fn vocab_scratch_path(dir: &Path, who: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    dir.join(format!(
        "autoshade-look-vocab-{who}-{}-{}.txt",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

/// The phrase list on disk for the length of ONE builder, removed when that
/// builder returns — by RAII, for the reason [`StagedFrame`] is: it used to be
/// removed by a trailing `remove_file`, and every `?` between the write and
/// that line (a finished photo that would not decode, a frame that would not
/// stage, a sidecar that refused) left the list behind in the user's store.
pub(super) struct VocabScratch(PathBuf);

impl VocabScratch {
    /// Write [`LOOK_VOCAB`] to [`vocab_scratch_path`] under `dir`. Constructed
    /// BEFORE the write, so a failed write still cleans up what it started.
    pub(super) fn write(dir: &Path, who: &str) -> Result<Self> {
        let scratch = VocabScratch(vocab_scratch_path(dir, who));
        std::fs::write(&scratch.0, LOOK_VOCAB.join("\n"))?;
        Ok(scratch)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for VocabScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The text whose vector becomes [`StyleExemplar::desc_embed`]: the record's
/// own description when it has one, otherwise its tag string.
///
/// ONE rule, used by both builders, so a RAW exemplar and a look record can
/// never end up describing themselves in two different vocabularies. `None`
/// means there is nothing to embed — a record with no description and no tags
/// keeps `desc_embed: None`, and the W_DESC term is simply absent for it
/// (`embed_distance`'s existing rule).
pub(super) fn desc_text(desc: Option<&str>, tags: &[String]) -> Option<String> {
    match desc.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => Some(d.chars().take(MAX_DESC_CHARS).collect()),
        None if !tags.is_empty() => Some(tags.join(", ")),
        None => None,
    }
}

/// One text vector per record, from ONE sidecar process.
///
/// This is the whole of F-12. The look build used to call the sidecar once PER
/// PHOTOGRAPH to embed that photograph's own tag string — 1.5 GB of weights
/// re-loaded per record — and the RAW build did not compute the vector at all,
/// so every one of its exemplars carried `desc_embed: None` and the W_DESC
/// term was dead. Here the whole build's texts go out as one JSONL manifest
/// and come back in order.
///
/// `texts[i] == None` contributes no line and gets no vector back; the answer
/// is mapped onto the records by position, and a batch whose length does not
/// line up is refused by [`crate::embed::parse_text_vectors`] rather than
/// zipped onto the wrong records.
fn embed_desc_texts(
    opts: &crate::embed::EmbedOpts,
    dir: &Path,
    texts: &[Option<String>],
) -> Result<Vec<Option<Vec<f32>>>> {
    let live: Vec<usize> = texts.iter().enumerate().filter(|(_, t)| t.is_some()).map(|(i, _)| i).collect();
    let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    if live.is_empty() {
        return Ok(out);
    }
    std::fs::create_dir_all(dir)?;
    static TEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem = format!(
        "autoshade-embed-desc-{}-{}",
        std::process::id(),
        TEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let manifest = dir.join(format!("{stem}.jsonl"));
    let scratch = dir.join(format!("{stem}.json"));
    let body: String = live
        .iter()
        .map(|&i| serde_json::json!({ "text": texts[i].as_deref().unwrap_or_default() }).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&manifest, body + "\n")
        .with_context(|| format!("write description text manifest {}", manifest.display()))?;
    let vectors = crate::embed::embed_text_batch(opts, &manifest, &scratch, live.len());
    let _ = std::fs::remove_file(&manifest);
    for (slot, v) in live.into_iter().zip(vectors?) {
        out[slot] = Some(v);
    }
    Ok(out)
}

/// The two record shapes that carry a description vector, behind one door — so
/// the RAW builder and the look builder cannot drift into two rules about what
/// `desc_embed` holds.
pub(super) trait DescribedRecord {
    fn desc(&self) -> Option<&str>;
    fn tags(&self) -> &[String];
    fn set_tags(&mut self, tags: Vec<String>);
    fn vocab_scores(&self) -> Option<&[f32]>;
    fn has_desc_embed(&self) -> bool;
    fn set_desc_embed(&mut self, v: Option<Vec<f32>>);
}

impl DescribedRecord for StyleExemplar {
    fn desc(&self) -> Option<&str> { self.desc.as_deref() }
    fn tags(&self) -> &[String] { &self.tags }
    fn set_tags(&mut self, tags: Vec<String>) { self.tags = tags; }
    fn vocab_scores(&self) -> Option<&[f32]> { self.vocab_scores.as_deref() }
    fn has_desc_embed(&self) -> bool { self.desc_embed.is_some() }
    fn set_desc_embed(&mut self, v: Option<Vec<f32>>) { self.desc_embed = v; }
}

impl DescribedRecord for LookExemplar {
    fn desc(&self) -> Option<&str> { self.desc.as_deref() }
    fn tags(&self) -> &[String] { &self.tags }
    fn set_tags(&mut self, tags: Vec<String>) { self.tags = tags; }
    fn vocab_scores(&self) -> Option<&[f32]> { self.vocab_scores.as_deref() }
    fn has_desc_embed(&self) -> bool { self.desc_embed.is_some() }
    fn set_desc_embed(&mut self, v: Option<Vec<f32>>) { self.desc_embed = v; }
}

/// Fill `desc_embed` for a whole build in ONE sidecar call.
///
/// DEGRADES, never fails: a text batch that could not run leaves the vectors
/// absent, the W_DESC term inert for this index, and one line on stderr saying
/// so — the same contract the image arm has kept since R27 Batch-5. An
/// hour-long index build must not be lost to the text half of it.
pub(super) fn attach_desc_embeddings<R: DescribedRecord>(
    opts: &crate::embed::EmbedOpts,
    dir: &Path,
    records: &mut [R],
    what: &str,
) {
    if records.is_empty() {
        return;
    }
    // A record that ALREADY carries a vector contributes no text: the cache
    // answered it (`crate::style_cache`), and re-embedding the same sentence
    // would be the sidecar call this batch exists to avoid.
    let texts: Vec<Option<String>> = records
        .iter()
        .map(|r| (!r.has_desc_embed()).then(|| desc_text(r.desc(), r.tags())).flatten())
        .collect();
    let live = texts.iter().filter(|t| t.is_some()).count();
    match embed_desc_texts(opts, dir, &texts) {
        Ok(vectors) => {
            // Only WRITE a vector that was produced: a `None` here is either a
            // record that had nothing to embed or one that already had its
            // vector, and overwriting the second with `None` would throw the
            // cached answer away at the last step.
            for (record, vector) in records.iter_mut().zip(vectors) {
                if vector.is_some() {
                    record.set_desc_embed(vector);
                }
            }
            if live > 0 {
                println!("  {what}: {live} description vector(s) in one text call");
            }
        }
        Err(e) => eprintln!(
            "  {what}: description text embedding unavailable ({e:#}) — the description \
             term stays inert for this index"
        ),
    }
}
