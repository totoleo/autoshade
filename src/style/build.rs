//! Building the index: the sidecars and stages, the exemplar cache, frame embedding, descriptions, and the self check.

use super::*;

/// The three things an index build needs from OUTSIDE itself: the two model
/// sidecars, and the directory it may write in.
///
/// A struct rather than three parameters because the SCRATCH DIRECTORY is the
/// load-bearing one and it used to be read from a global. `cargo test` runs
/// with `AUTOSHADE_DATA_DIR` pointing at a real store (and, without it, at
/// `%LOCALAPPDATA%/autoshade`), so a build driven by a test wrote its staged
/// frames — and, once S2 added one, its DESCRIPTION CACHE — into the user's
/// own store, where a later live build would have served the stub sentences
/// back. Observed, not theorised: a test run on 2026-08-30 left 16 entries of
/// `"a stubbed grade sentence"` in `%LOCALAPPDATA%/autoshade/style-descriptions.json`.
pub struct BuildSidecars {
    pub embed: crate::embed::EmbedOpts,
    pub describe: crate::describe::DescribeOpts,
    /// Where staged frames and the description cache live. Production passes
    /// [`crate::store::store_root`]; a test passes its own directory.
    pub scratch: PathBuf,
}

impl BuildSidecars {
    /// What the production callers use: both sidecars from [`Config`], and the
    /// per-user store as the scratch root.
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        BuildSidecars {
            embed: crate::embed::EmbedOpts::from_config(cfg),
            describe: crate::describe::DescribeOpts::from_config(cfg),
            scratch: crate::store::store_root(),
        }
    }
}

/// Which phase of an index build a [`BuildProgress`] belongs to.
///
/// The build used to be one loop with one counter, because every model call
/// happened inside it. Since S2 it is four phases over the WHOLE library, and
/// only the first of them has per-record granularity — the other three are one
/// sidecar process each, and this process genuinely cannot see inside them. A
/// bare `(done, total)` would have had to either lie about that or reset to
/// zero three times with nothing to say why; the stage is what makes the pair
/// readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStage {
    /// Decode every source and stage its 512-px frame — the only phase this
    /// process runs itself, and the only one that reports per record.
    Frames,
    /// ONE SigLIP call over every staged frame (image vector + vocabulary
    /// scores).
    Embed,
    /// ONE Qwen call over the frames this machine has not already described.
    /// `done` at entry is the number the cache already answered.
    Describe,
    /// ONE SigLIP text call over every description-or-tag string.
    Text,
}

impl BuildStage {
    /// The stage's name for a UI, in English — the GUI puts it through `tr()`
    /// like every other string it shows.
    pub fn label(self) -> &'static str {
        match self {
            BuildStage::Frames => "decoding",
            BuildStage::Embed => "embedding",
            BuildStage::Describe => "describing",
            BuildStage::Text => "text vectors",
        }
    }
}

/// One progress report from an index build: which phase, and how far into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildProgress {
    pub stage: BuildStage,
    pub done: usize,
    pub total: usize,
}

/// The bands the EXEMPLAR CACHE admits an entry inside — the index door's own
/// numbers ([`exemplar_is_finite`]), handed to the cache rather than restated
/// there.
///
/// A cache is a file on disk, so it gets the index's threat model ("invariants
/// at the door"), not "we wrote it, so it is fine": a bit-rotted vector served
/// out of it would land in a published index that then refuses to LOAD,
/// turning a cache defect into a destroyed library.
pub(super) const CACHE_BANDS: crate::style_cache::Bands = crate::style_cache::Bands {
    ndim: NDIM,
    feature_abs: MAX_FEATURE_ABS,
    embed_dim: crate::embed::EMBED_DIM,
    vocab: LOOK_VOCAB.len(),
    desc_chars: MAX_DESC_CHARS,
    version: CURRENT_INDEX_VERSION,
};

/// One photograph's result from the decode pool.
pub(super) struct BuiltRecord {
    pub(super) ex: StyleExemplar,
    /// The staged embedding frame, when this build had to produce one. `None`
    /// means the cache answered this photograph whole and nothing was decoded.
    pub(super) frame: Option<StagedFrame>,
    /// The content key, when it is already known — i.e. it came from the
    /// cache. A decoded record's key is hashed from its frame after the pool.
    pub(super) digest: Option<String>,
    /// The identity of the file this was measured from, for the cache's fast
    /// path on the NEXT build.
    pub(super) stamp: Option<crate::style_cache::SourceStamp>,
}

/// Read one photograph's SIDECAR and assemble its exemplar, with `feat`
/// already measured — by a decode, or by the cache.
///
/// This half is paid on EVERY build, deliberately: the sliders, the curve, the
/// family summary and the mask habit are properties of the `.xmp`, not of the
/// pixels, so a photograph whose vectors came out of the cache still re-reads
/// the edit the photographer may have changed since. It is a small file read
/// beside a ~1 s decode.
///
/// `fill` is where a cache entry's own answers are applied — before the
/// finiteness door below, so anything the cache hands back is checked by the
/// same rule a freshly measured exemplar is.
pub(super) fn read_exemplar(
    raw: &Path,
    sidecar: &Path,
    feat: [f32; NDIM],
    loss_counts: &[std::sync::atomic::AtomicU32],
    fill: impl FnOnce(&mut StyleExemplar),
) -> Option<StyleExemplar> {
    use std::sync::atomic::Ordering;
    // An unreadable sidecar must SKIP the photo, not produce a settings-free
    // exemplar that dilutes retrieval (the pair scan guaranteed the .xmp
    // exists — a read failure here is a real error).
    let Some(xmp) = crate::store::read_sidecar(sidecar) else {
        eprintln!("  skip {}: xmp unreadable or over the sidecar size cap", pipeline::stem(raw));
        return None;
    };
    // The photographer's LOCAL work, read through the SAME importer the
    // develop chain uses (S3). Path-aware, so a brush group whose strokes live
    // in a sibling `.acr` is readable; SILENT, because a 169-photo build
    // discloses what it could not read ONCE, in aggregate, after the pool —
    // not 169 times inside it.
    let losses = crate::xmp::import_losses_for_photo(&xmp, raw);
    // A Range Mask this engine cannot honour is DROPPED by the importer, so
    // the surviving recipe under-reports the photographer's refinements — the
    // loss channel is where that fact still exists (see
    // `MaskHabit::of_with_refused_ranges`).
    let refused_ranges = losses
        .iter()
        .filter(|l| l.reason.same_kind(crate::xmp::MaskImportReason::ForeignRangeMask))
        .count();
    let habit = crate::mask_habit::MaskHabit::of_with_refused_ranges(
        &crate::xmp::xmp_to_recipe_for_photo(&xmp, raw).masks,
        refused_ranges,
    );
    for l in &losses {
        if let Some(k) =
            crate::xmp::MaskImportReason::ALL.iter().position(|r| r.same_kind(l.reason))
        {
            loss_counts[k].fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut ex = StyleExemplar {
        stem: pipeline::stem(raw).to_string(),
        path: std::path::absolute(raw).ok().map(|p| p.display().to_string()),
        tag: derive_tag(&feat),
        feat: feat.to_vec(),
        settings: read_settings(&xmp),
        curve: crate::eval::user_curve_shape(&xmp).map(|(b, s)| [b, s]),
        families: crate::eval::user_family_summary(&xmp),
        embed: None,
        tags: Vec::new(),
        vocab_scores: None,
        desc: None,
        desc_embed: None,
        masks: Some(habit),
        mono: read_monochrome(&xmp),
    };
    fill(&mut ex);
    if exemplar_is_finite(&ex) {
        Some(ex)
    } else {
        eprintln!(
            "  skip {}: non-finite or out-of-band metadata/settings (would corrupt the index)",
            pipeline::stem(raw)
        );
        None
    }
}

/// May this build skip the DECODE for a photograph the cache already holds?
///
/// All-or-nothing, and that is the point: the frame digest is a function of
/// the staged frame, so a photograph that skipped the decode has no frame —
/// and a record with no frame can never afterwards be embedded or described.
/// A cache entry therefore either answers everything this build asked for, or
/// the photograph decodes.
pub(super) fn cache_answers_everything(
    entry: &crate::style_cache::CachedExemplar,
    want_embed: bool,
    want_desc: bool,
    provenance: &str,
) -> bool {
    let embed_ok =
        !want_embed || (entry.embed.is_some() && entry.embedding_is_current(provenance));
    let desc_ok = !want_desc || entry.current_desc().is_some();
    embed_ok && desc_ok
}

/// Serve one cache entry's MEASUREMENTS into an exemplar this build just read.
///
/// Gated on what the build ASKED for: a `style-index` without `--embed` must
/// not silently acquire vectors, and one without `--describe` must not
/// silently acquire prose — either would make the index claim work the user
/// did not request this run.
///
/// …and, for the vectors, on PROVENANCE: `embed` and `vocab_scores` are served
/// only while [`crate::style_cache::CachedExemplar::embedding_is_current`]
/// says they came out of this build's checkpoint, tokenizer and phrase list.
/// The warm path asks that before it skips a decode
/// ([`cache_answers_everything`]); this is the same question at the other
/// door. A digest-rescued entry from another checkpoint used to hand its
/// vector straight to the record, the embedding stage then skipped the record
/// (it "had" a vector), and [`cache_entry`] re-stamped the old vector with
/// THIS build's provenance — a stale vector laundered into a current one every
/// time the checkpoint, the tokenizer or the vocabulary moved.
///
/// The TAGS are not served here and neither is the description VECTOR, and
/// that is the v6 shape rather than an omission: both are functions of the
/// whole population's scores (`retag`), which nothing inside the decode pool
/// can know. [`adopt_cached_desc_embed`] serves the vector once they are
/// final.
pub(super) fn apply_cached(
    ex: &mut StyleExemplar,
    c: &crate::style_cache::CachedExemplar,
    want_embed: bool,
    want_desc: bool,
    provenance: &str,
) {
    if want_embed && c.embedding_is_current(provenance) {
        ex.embed = c.embed.clone();
        ex.vocab_scores = c.vocab_scores.clone();
    }
    if want_desc {
        ex.desc = c.current_desc().map(str::to_string);
    }
}

/// Serve one cache entry's description VECTOR, once the text it is the vector
/// OF is settled.
///
/// The rule is unchanged — a stored vector is reusable only while the record
/// still SAYS the text it was made from — but it can only be applied after
/// [`retag`], because [`desc_text`] falls back to the tags and since v6 those
/// are derived from the library mean. Running it inside the pool, as v5 did,
/// would compare the cached text against tags this build had not derived yet.
///
/// And only under this build's own provenance: the text tower's numbers move
/// with the checkpoint and the tokenizer exactly as the image tower's do, and
/// the entry's one stamp covers all three of its vectors.
pub(super) fn adopt_cached_desc_embed(
    ex: &mut StyleExemplar,
    c: &crate::style_cache::CachedExemplar,
    provenance: &str,
) {
    if ex.desc_embed.is_none()
        && c.embedding_is_current(provenance)
        && c.desc_embed.is_some()
        && c.desc_text.is_some()
        && c.desc_text == desc_text(ex.desc.as_deref(), &ex.tags)
    {
        ex.desc_embed = c.desc_embed.clone();
    }
}

/// May this build REWRITE the exemplar cache, pruned to `keep`?
///
/// Only when its keep-set is COMPLETE, which takes both halves. A build that
/// staged frames knows the content key of every photograph it decoded, so
/// whatever its keep-set lacks really has left the library. A `style-index`
/// without the embedding pass stages nothing: its keep-set still holds the
/// keys the WARM path carried out of the cache — so it is not empty, which is
/// all the old guard tested — but a photograph that was merely touched (a
/// copy, a `touch`, a re-pointed library) decoded without staging and has no
/// key at all, and pruning to that set retired its entry: the hour of SigLIP
/// work the cache exists to keep, thrown away by the build that is meant to
/// be the cheap one. Such a build leaves the file exactly as the previous
/// embedding build published it.
///
/// An EMPTY keep-set is refused even from a build that staged (every frame
/// failed) — same instinct as `save`'s empty-index refusal.
pub(super) fn cache_is_publishable(staged_frames: bool, keep: &std::collections::BTreeSet<String>) -> bool {
    staged_frames && !keep.is_empty()
}

/// The cache entry for one finished exemplar.
///
/// Only an EMBEDDING build writes one ([`cache_is_publishable`]), so the
/// vectors here are always this build's own measurements — or the cache's,
/// served back through [`apply_cached`] under this build's provenance — and
/// the stamp is an honest one. The DESCRIPTION pass is the one such a build
/// may not have asked for: `style-index --embed` without `--describe` carries
/// the PREVIOUS entry's prose forward instead of overwriting it with the
/// absence, because publishing "no description" as the new truth would throw
/// away a Qwen pass the user never asked to redo.
pub(super) fn cache_entry(
    ex: &StyleExemplar,
    source: crate::style_cache::SourceStamp,
    prior: Option<&crate::style_cache::CachedExemplar>,
    want_desc: bool,
    provenance: &str,
) -> crate::style_cache::CachedExemplar {
    crate::style_cache::CachedExemplar {
        source,
        version: CURRENT_INDEX_VERSION,
        feat: ex.feat.clone(),
        embed: ex.embed.clone(),
        vocab_scores: ex.vocab_scores.clone(),
        // A stamp is a claim about a vector: a record that has none is not
        // stamped, so no later build can read "measured, from this
        // checkpoint" off an entry that measured nothing.
        provenance: ex.embed.is_some().then(|| provenance.to_string()),
        desc: if want_desc {
            ex.desc.clone().map(crate::describe::CachedDescription::current)
        } else {
            prior.and_then(|p| p.desc.clone())
        },
        desc_text: desc_text(ex.desc.as_deref(), &ex.tags),
        desc_embed: ex.desc_embed.clone(),
    }
}

/// How many unpaired RAWs [`unpaired_note`] NAMES before it stops counting.
///
/// Ten, not all of them: the count is the fact, the names are the recognition
/// aid, and a library where every photograph is unpaired (the user pointed the
/// build at the wrong folder — the single most common way this ends in an
/// empty index) would otherwise answer with a 2,000-line wall.
pub(super) const MAX_UNPAIRED_LISTED: usize = 10;

/// The RAWs a build could NOT learn from, because no folder in the pairing
/// rule held their sidecar.
///
/// Until this existed the build printed only the SURVIVING pair count, so a
/// library that indexed 40 of 2,000 photographs and a library of 40 read
/// identically — and the commonest cause of "0 RAW+.xmp pairs" (sidecars kept
/// in a separate tree, which `--xmp-dir` now reaches) was indistinguishable
/// from "you have not edited anything".
///
/// STEMS, never full paths, like every other per-photograph disclosure in this
/// module: the folder is already on the line above it.
pub(super) fn unpaired_note(unpaired: &[&Path]) -> Option<String> {
    if unpaired.is_empty() {
        return None;
    }
    let named: Vec<&str> =
        unpaired.iter().take(MAX_UNPAIRED_LISTED).map(|p| pipeline::stem(p)).collect();
    let more = unpaired.len() - named.len();
    let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
    Some(format!(
        "  {} RAW(s) skipped — no .xmp sidecar found (looked in --xmp-dir, its mirror of this \
         folder, and beside the RAW): {}{}",
        unpaired.len(),
        named.join(", "),
        tail
    ))
}

pub(super) fn report(on_progress: &dyn Fn(BuildProgress), stage: BuildStage, done: usize, total: usize) {
    on_progress(BuildProgress { stage, done, total });
}

/// ONE SigLIP image call for a whole build: N staged frames in, N records out,
/// in order.
///
/// `frames[i] == None` (this record's staging failed) contributes no manifest
/// line and gets `None` back, exactly like [`embed_desc_texts`]'s text arm.
/// The answer is mapped back by PATH rather than by position, because
/// `embed.py`'s batch door reports a malformed manifest line as soon as it
/// reads it and the rest in loop order — a positional zip would attach one
/// photograph's vector to another's exemplar.
pub(super) fn embed_frames(
    opts: &crate::embed::EmbedOpts,
    dir: &Path,
    frames: &[Option<StagedFrame>],
    want: &[bool],
    what: &str,
) -> Result<Vec<Option<crate::embed::EmbedRecord>>> {
    let mut out: Vec<Option<crate::embed::EmbedRecord>> = (0..frames.len()).map(|_| None).collect();
    // `want[i] == false` is a record whose vector the CACHE already answered
    // (`crate::style_cache`): it contributes no manifest line, gets `None`
    // back, and keeps what it has — the same shape as a record whose staging
    // failed, and the reason a full-hit rebuild starts no sidecar at all.
    let live: Vec<(usize, &StagedFrame)> = frames
        .iter()
        .enumerate()
        .filter(|(i, _)| want.get(*i).copied().unwrap_or(true))
        .filter_map(|(i, f)| f.as_ref().map(|f| (i, f)))
        .collect();
    if live.is_empty() {
        return Ok(out);
    }
    std::fs::create_dir_all(dir)?;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem = format!(
        "autoshade-embed-frames-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let manifest = dir.join(format!("{stem}.jsonl"));
    let scratch = dir.join(format!("{stem}.out.jsonl"));
    let body: String = live
        .iter()
        .map(|(_, f)| serde_json::json!({ "path": f.image_path() }).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&manifest, body + "\n")
        .with_context(|| format!("write {what} embedding manifest {}", manifest.display()))?;
    let answered = crate::embed::embed_image_batch(opts, &manifest, &scratch);
    let _ = std::fs::remove_file(&manifest);
    let answered = answered?;
    let by_path: std::collections::HashMap<&str, &crate::embed::EmbedBatchRecord> =
        answered.iter().map(|r| (r.path.as_str(), r)).collect();
    let mut ok = 0usize;
    for (slot, frame) in live {
        let key = frame.image_path();
        match by_path.get(key.as_str()) {
            Some(rec) => match &rec.record {
                Some(record) => {
                    out[slot] = Some(record.clone());
                    ok += 1;
                }
                None => eprintln!(
                    "  {what}: no style embedding for one frame ({}) — indexed on the 14-dim \
                     feature alone",
                    rec.error.as_deref().unwrap_or("the sidecar gave no reason")
                ),
            },
            None => eprintln!(
                "  {what}: the embedding sidecar answered nothing for one staged frame — that \
                 record is indexed on the 14-dim feature alone"
            ),
        }
    }
    println!("  {what}: {ok} image vector(s) in one sidecar call");
    Ok(out)
}

/// The two record shapes that carry a DESCRIPTION, behind one door — so the
/// RAW builder and the look builder cannot drift into two rules about what
/// `desc` holds. Sibling of [`DescribedRecord`], which owns the vector.
pub(super) trait DescribableRecord {
    fn set_desc(&mut self, desc: Option<String>);
    fn has_desc(&self) -> bool;
}

impl DescribableRecord for StyleExemplar {
    fn set_desc(&mut self, desc: Option<String>) { self.desc = desc; }
    fn has_desc(&self) -> bool { self.desc.is_some() }
}

impl DescribableRecord for LookExemplar {
    fn set_desc(&mut self, desc: Option<String>) { self.desc = desc; }
    fn has_desc(&self) -> bool { self.desc.is_some() }
}

/// STAGE 3 of a build: fill `desc` for every record whose frame this machine
/// can describe, in ONE sidecar call.
///
/// DEGRADES, never fails, at every step — the same contract the embedding arm
/// has kept since R27 Batch-5, and for the same reason: an hour-long index
/// build must not be lost to an optional field. A missing switch, a missing
/// script, an unreadable frame, a sidecar that refused: each leaves `desc`
/// absent for the records it touched, prints one sentence saying so, and the
/// build carries on with the tags.
///
/// The CACHE is content-keyed ([`crate::describe::frame_digest`]), so a
/// library that gained one photograph describes one photograph. It is written
/// once, at the end, with the keys this build used — never per record, which
/// would publish a 78 MiB-capped file 169 times.
#[allow(clippy::too_many_arguments)] // one stage's whole input; see BuildSidecars
pub(super) fn attach_descriptions<R: DescribableRecord>(
    opts: &crate::describe::DescribeOpts,
    describe: DescribeSwitch,
    dir: &Path,
    cache_path: &Path,
    library: &str,
    digests: &[Option<String>],
    frames: &[Option<StagedFrame>],
    records: &mut [R],
    what: &str,
    on_progress: &dyn Fn(BuildProgress),
) {
    if !describe.on() || records.is_empty() {
        return;
    }
    let total = records.len();
    if !opts.available() {
        eprintln!(
            "  look descriptions requested but the sidecar is not at {} — the index carries \
             attribute tags only (set AUTOSHADE_DESCRIBE_SCRIPT, or run from the project dir)",
            opts.script.display()
        );
        return;
    }
    println!(
        "  look descriptions ON ({}) — first run downloads ~4.3 GB of Qwen3-VL weights",
        opts.script.display()
    );
    let mut cache = crate::describe::DescriptionCache::load(cache_path);
    // The digests are the BUILD's, computed once by [`frame_digests`] and
    // shared with the image stage and the exemplar cache — a record that came
    // whole out of that cache has a key here and no frame, which is exactly
    // what makes it a hit with nothing to send.
    let mut hits = 0usize;
    for (record, digest) in records.iter_mut().zip(digests) {
        if let Some(desc) = digest.as_deref().and_then(|d| cache.get(d)) {
            record.set_desc(Some(desc.to_string()));
            hits += 1;
        }
    }
    report(on_progress, BuildStage::Describe, hits, total);
    // The MISSES, in record order. A frame the cache already answered is not
    // sent again — that is the whole reason the cache exists.
    let misses: Vec<(usize, &StagedFrame)> = frames
        .iter()
        .enumerate()
        .filter(|(i, _)| digests[*i].is_some() && !records[*i].has_desc())
        .filter_map(|(i, f)| f.as_ref().map(|f| (i, f)))
        .collect();
    let mut keep: std::collections::BTreeSet<String> =
        digests.iter().flatten().cloned().collect();
    if misses.is_empty() {
        println!("  {what}: {hits} description(s), all from the content cache");
        report(on_progress, BuildStage::Describe, total, total);
        // Still republished: the cache's retention set is what this build
        // used, so a build that hit 100 % keeps those entries alive.
        if let Err(e) = cache.save(cache_path, &keep, library) {
            eprintln!("  {what}: the description cache could not be published ({e:#})");
        }
        return;
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem = format!(
        "autoshade-describe-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let manifest = dir.join(format!("{stem}.jsonl"));
    let scratch = dir.join(format!("{stem}.out.jsonl"));
    let body: String = misses
        .iter()
        .map(|(_, f)| serde_json::json!({ "path": f.image_path() }).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&manifest, body + "\n")) {
        eprintln!("  {what}: the description manifest could not be written ({e}) — no prose this build");
        // The stage was OPENED above (`hits` of `total`); every other way out
        // of it closes it, and a progress bar left at the hit count would
        // read as a description pass still running.
        report(on_progress, BuildStage::Describe, total, total);
        return;
    }
    let answered = crate::describe::describe_manifest(opts, &manifest, &scratch);
    let _ = std::fs::remove_file(&manifest);
    let answered = match answered {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "  {what}: look descriptions unavailable ({e:#}) — the index carries attribute \
                 tags only"
            );
            report(on_progress, BuildStage::Describe, total, total);
            return;
        }
    };
    let by_path: std::collections::HashMap<&str, &crate::describe::DescribeRecord> =
        answered.iter().map(|r| (r.path.as_str(), r)).collect();
    let mut fresh = 0usize;
    let mut refused = 0usize;
    for (slot, frame) in misses {
        let key = frame.image_path();
        match by_path.get(key.as_str()).and_then(|r| r.desc.clone()) {
            Some(desc) => {
                if let Some(d) = digests[slot].clone() {
                    cache.insert(d.clone(), desc.clone(), library);
                    keep.insert(d);
                }
                records[slot].set_desc(Some(desc));
                fresh += 1;
            }
            None => refused += 1,
        }
    }
    if refused > 0 {
        eprintln!(
            "  {what}: {refused} frame(s) got no description — those exemplars carry their \
             attribute tags alone"
        );
    }
    println!("  {what}: {fresh} new description(s) in one sidecar call, {hits} from the cache");
    if let Err(e) = cache.save(cache_path, &keep, library) {
        eprintln!("  {what}: the description cache could not be published ({e:#})");
    }
    report(on_progress, BuildStage::Describe, total, total);
}

pub fn embed_preview_with_text(
    opts: &crate::embed::EmbedOpts,
    preview: &image::DynamicImage,
    dir: &Path,
    tag: &str,
    text: Option<&str>,
) -> Result<crate::embed::EmbedRecord> {
    let staged = stage_embed_frame(preview, dir, tag)?;
    let mut o = crate::embed::EmbedOpts { python_bin: opts.python_bin.clone(), script: opts.script.clone(), text_file: None, vocab_file: opts.vocab_file.clone() };
    let text_path = if let Some(t) = text.filter(|s| !s.trim().is_empty()) {
        static TEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = dir.join(format!("autoshade-embed-{}-{}-{tag}.txt", std::process::id(), TEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(&p, t)?;
        o.text_file = Some(p.clone());
        Some(p)
    } else { None };
    let result = embed_staged_record(&o, &staged);
    if let Some(p) = text_path { let _ = std::fs::remove_file(p); }
    result
}

/// The ONE degradation line the index build prints when a photo ends up
/// without a vector — shared by the staging arm and the sidecar arm so the two
/// failures read identically to the user, who cannot tell them apart and does
/// not need to.
pub(super) fn note_no_embedding(raw: &Path, e: &anyhow::Error) {
    eprintln!(
        "  {}: no style embedding ({e:#}) — indexed on the 14-dim feature alone",
        pipeline::stem(raw)
    );
}

/// Is this exemplar the QUERY photo itself? Path identity when the exemplar
/// records one (case-folded on Windows, like `store::photo_key`); stem
/// fallback for pre-path indexes — there, over-exclusion is the safe
/// direction (an unrelated same-stem exemplar loses one retrieval slot; a
/// self-reference would teach the AI to copy the photo's own edit back).
pub(super) fn is_self(e: &StyleExemplar, query_path: &str, query_stem: &str) -> bool {
    match &e.path {
        Some(p) => {
            if cfg!(windows) {
                p.to_lowercase() == query_path.to_lowercase()
            } else {
                p == query_path
            }
        }
        None => e.stem == query_stem,
    }
}
