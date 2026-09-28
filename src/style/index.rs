//! The style index: its record and everything it does.

use super::*;

#[derive(Serialize, Deserialize, Clone)]
pub struct StyleIndex {
    pub version: u32,
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
    pub exemplars: Vec<StyleExemplar>,
    /// Absolute folder this index was built from (the user's edited-RAW library),
    /// so the UI can show its provenance. `#[serde(default)]` keeps old index
    /// files (written before this field) loadable.
    #[serde(default)]
    pub source_dir: Option<String>,
    #[serde(default)]
    pub looks: Vec<LookExemplar>,
    #[serde(default)]
    pub looks_dir: Option<String>,
    #[serde(default)]
    pub embed_provenance: Option<String>,
}

impl StyleIndex {
    /// Scan a folder for RAW+.xmp pairs (the user's own edits) and build the
    /// index, reporting nothing but the historical stdout lines.
    ///
    /// `xmp_dir` is the `--xmp-dir` folder of sidecars, or `None` for the
    /// beside-the-RAW convention; either way the pairing rule is
    /// [`crate::xmp_pair`]'s and only its.
    pub fn build(
        dir: &Path,
        xmp_dir: Option<&Path>,
        embed: EmbeddingSwitch,
        describe: DescribeSwitch,
    ) -> Result<StyleIndex> {
        Self::build_reporting(dir, xmp_dir, embed, describe, &|_| {})
    }

    /// Build the finished-photo look library. Looks are embedding-only and
    /// never participate in settings targets or recipe blending.
    pub fn build_looks(
        dir: &Path,
        embed: EmbeddingSwitch,
        describe: DescribeSwitch,
        on_progress: &dyn Fn(BuildProgress),
    ) -> Result<StyleIndex> {
        Self::build_looks_with(
            BuildSidecars::from_config(&crate::config::Config::load()),
            dir,
            embed,
            describe,
            on_progress,
        )
    }

    /// [`build_looks`](Self::build_looks) over an explicit sidecar
    /// configuration — the seam its refusal test drives.
    ///
    /// The test used to point `AUTOSHADE_EMBED_SCRIPT` at a nonexistent file
    /// with an unsafe environment write and put it back afterwards. `cargo test` runs on
    /// parallel threads in one process, so for the duration of that test EVERY
    /// other test's idea of where the sidecar lives was wrong. Same rule as the
    /// switch and the weights: pass the value.
    pub fn build_looks_with(
        sidecars: BuildSidecars,
        dir: &Path,
        embed: EmbeddingSwitch,
        describe: DescribeSwitch,
        on_progress: &dyn Fn(BuildProgress),
    ) -> Result<StyleIndex> {
        let BuildSidecars { embed: opts, describe: describe_opts, scratch } = sidecars;
        if !opts.available() || !embed.on() {
            anyhow::bail!("look library requires the style-embedding sidecar; enable embedding and rebuild")
        }
        let mut files = Vec::new();
        for entry in walkdir(dir)? {
            if pipeline::BAKED_EXTS.iter().any(|e| entry.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case(e))) {
                files.push(entry);
            }
        }
        files.sort();
        if files.is_empty() { anyhow::bail!("look library contains no finished photos") }
        // Refused BEFORE the first decode, not after an hour of them: the file
        // cap is derived from this number (`MAX_LOOK_EXEMPLARS`), and a build
        // that overran it would produce an index its own loader refuses.
        if files.len() > MAX_LOOK_EXEMPLARS {
            anyhow::bail!(
                "{} holds {} finished photos and the look library caps at {} — point it at a \
                 curated folder of reference grades, not a whole archive",
                dir.display(), files.len(), MAX_LOOK_EXEMPLARS
            )
        }
        let total = files.len();
        report(on_progress, BuildStage::Frames, 0, total);
        std::fs::create_dir_all(&scratch)?;
        sweep_intermediates_and_say(&scratch, "look library");
        let vocab = VocabScratch::write(&scratch, "looks")?;
        let mut opts = opts;
        opts.vocab_file = Some(vocab.path().to_path_buf());
        // STAGE 1 — decode every finished photo and stage its frame. Nothing
        // else: the model stages below each run ONCE for the whole library.
        let mut looks = Vec::new();
        let mut frames: Vec<Option<StagedFrame>> = Vec::new();
        for (i, path) in files.iter().enumerate() {
            let _permit = decode::DecodePermit::acquire();
            let decoded = decode::decode_any(path).with_context(|| format!("decode look {}", path.display()))?;
            let frame = stage_embed_frame(&decoded.preview, &scratch, &format!("look-{i}"))?;
            drop(_permit);
            let path_abs = std::path::absolute(path)?.display().to_string();
            looks.push(LookExemplar {
                stem: pipeline::stem(path).to_string(),
                path: path_abs,
                embed: Vec::new(),
                tags: Vec::new(),
                vocab_scores: None,
                desc: None,
                desc_embed: None,
            });
            frames.push(Some(frame));
            report(on_progress, BuildStage::Frames, i + 1, total);
        }
        // STAGE 2 — ONE SigLIP image call for the whole library. A look
        // WITHOUT a vector is not a look (the library is embedding-only), so
        // unlike the RAW build this one fails the record rather than degrading
        // it.
        report(on_progress, BuildStage::Embed, 0, total);
        let want = vec![true; frames.len()];
        let vectors = embed_frames(&opts, &scratch, &frames, &want, "look library")?;
        // `vocab` stays alive to the end of the function: the description
        // pass below hands the sidecar the same `--vocab-file`
        // (`attach_desc_embeddings` clones `opts.vocab_file`), and the
        // sidecar opens whatever path it is given. Removing the file here,
        // as the build once did, made every look's text call fail on a path
        // that no longer existed — a look library without description
        // vectors, disclosed only as a degradation line.
        for (i, (look, record)) in looks.iter_mut().zip(&vectors).enumerate() {
            let record = record.as_ref().ok_or_else(|| {
                anyhow::anyhow!("embed look {}: the sidecar returned no vector", files[i].display())
            })?;
            look.embed = record.vector.clone();
            look.vocab_scores = record.vocab_scores.clone();
        }
        report(on_progress, BuildStage::Embed, total, total);
        // STAGE 3 — ONE Qwen call over the frames that are not already
        // described, then STAGE 4, ONE SigLIP text call over the whole set.
        // The look library keeps the DESCRIPTION cache and nothing else: its
        // records carry no 14-dim feature for a `style_cache` entry to be
        // about, it is capped at a few hundred CURATED finished photos, and it
        // is rebuilt only when the user re-curates that folder — where the RAW
        // build is the hour-long one this batch exists for.
        let digests = frame_digests(&frames, "look library");
        attach_descriptions(
            &describe_opts,
            describe,
            &scratch,
            &crate::describe::cache_path_in(&scratch),
            &library_key(dir),
            &digests,
            &frames,
            &mut looks,
            "look library",
            on_progress,
        );
        drop(frames);
        // The look library is its OWN population, so it gets its own mean:
        // ninety curated finished photographs and five thousand RAWs are not
        // one corpus, and centring the two together would tag each against a
        // level neither of them sits at.
        retag(&mut looks);
        report(on_progress, BuildStage::Text, 0, total);
        attach_desc_embeddings(&opts, &scratch, &mut looks, "look library");
        report(on_progress, BuildStage::Text, total, total);
        Ok(StyleIndex { version: CURRENT_INDEX_VERSION, mean: vec![0.0; NDIM], std: vec![1.0; NDIM], exemplars: Vec::new(), source_dir: None, looks, looks_dir: std::path::absolute(dir).ok().map(|p| p.display().to_string()), embed_provenance: Some(embed_provenance_string()) })
    }

    /// [`build`](StyleIndex::build) with a progress callback — one
    /// [`BuildProgress`] per record inside the decode stage, and one at each
    /// end of every model stage.
    ///
    /// Called on the CALLER's thread (from the result-collector loop), not
    /// from a decode worker: that keeps the callback free of `Send + Sync`
    /// bounds, which matters because the GUI's callback carries an mpsc
    /// `Sender` (`Send`, NOT `Sync`). The stdout lines stay in the workers,
    /// byte-identical to what the CLI has always printed.
    ///
    /// **STAGES, not per-photo model calls (step 14 / S2).** Until this batch
    /// the image half of the embedding ran ONE SIDECAR PROCESS PER PHOTOGRAPH:
    /// 169 loads of a 1.50 GB checkpoint for the photographer's own library,
    /// measured at 5,618 s. `embed.py --manifest-jsonl` has always embedded N
    /// images in one process (the TEXT half already went out as one batch in
    /// S1-fix F-12), so the build is now four phases over the WHOLE library —
    /// decode+stage every frame, ONE SigLIP image call, ONE Qwen description
    /// call, ONE SigLIP text call. One process per model per build; the model
    /// slot ([`crate::with_model_slot`]) still guarantees one resident model at
    /// a time, and the per-record fail-soft contract is unchanged: a
    /// photograph whose frame or vector failed keeps its 14-dim exemplar and
    /// says why.
    pub fn build_reporting(
        dir: &Path,
        xmp_dir: Option<&Path>,
        embed: EmbeddingSwitch,
        describe: DescribeSwitch,
        on_progress: &dyn Fn(BuildProgress),
    ) -> Result<StyleIndex> {
        // R27 P1, deliberate NON-action: RAW-only, for `eval`'s reason (see
        // that call site) plus one of its own. The index learns the user's
        // style from RAW+sidecar pairs, and its exemplars are compared against
        // the CAMERA's own rendition — `decode::embedded_preview`, the one
        // door that keeps the strict "camera pixels or nothing" contract. A
        // baked source has no camera rendition at all, so it could contribute
        // an exemplar to the index but never a comparable one.
        let raws = pipeline::find_raws(dir)?;
        // ONE pairing rule ([`crate::xmp_pair`]), resolved HERE on the calling
        // thread and carried into the pool as a PATH. The rule memoises one
        // folder listing per folder, so resolving the whole scan in one place
        // is also what keeps a 2,000-photograph library at one `read_dir` per
        // folder; and a worker that re-derived the name would be the second
        // definition of the rule that module exists to remove.
        let pairing = crate::xmp_pair::XmpPairing::new(dir, xmp_dir);
        let mut pairs: Vec<(&Path, PathBuf)> = Vec::new();
        let mut unpaired: Vec<&Path> = Vec::new();
        for raw in &raws {
            match pairing.find(raw) {
                Some(sidecar) => pairs.push((raw.as_path(), sidecar)),
                None => unpaired.push(raw.as_path()),
            }
        }
        println!("building style index from {} RAW+.xmp pairs ...", pairs.len());
        if let Some(note) = unpaired_note(&unpaired) {
            println!("{note}");
        }
        // Decode in parallel: each pair pays ~1s of full-res embedded-JPEG decode
        // and the old serial scan left every other core idle (a 2000-pair library
        // took the better part of an hour). The worker count IS the process-wide
        // decode cap (decode::MAX_CONCURRENT_DECODES; each in-flight decode can
        // hold ~180 MB), and each decode also takes a DecodePermit — so two
        // concurrent builds (the web server's request threads) share one budget
        // instead of stacking to ~1.4 GB, while a single build never blocks. An
        // atomic counter hands out indices, and each result lands in its own
        // slot so the exemplar ORDER stays identical to the serial version.
        use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
        let cfg = crate::config::Config::load();
        // The two sidecars, resolved ONCE before the pool starts. `None` when
        // the user has not asked for one, or when its script is not on disk —
        // announced here, not discovered 169 times.
        let mut embedder: Option<crate::embed::EmbedOpts> = embed
            .on()
            .then(|| crate::embed::EmbedOpts::from_config(&cfg))
            .filter(|o| {
                if o.available() {
                    println!(
                        "  style embedding ON ({}) — first run downloads ~1.50 GB of SigLIP 2 \
                         weights",
                        o.script.display()
                    );
                    true
                } else {
                    eprintln!(
                        "  style embedding requested but the sidecar is not at {} — building the \
                         14-dim index only (set AUTOSHADE_EMBED_SCRIPT, or run from the project dir)",
                        o.script.display()
                    );
                    false
                }
            });
        let describer = crate::describe::DescribeOpts::from_config(&cfg);
        let embed_dir = crate::store::store_root();
        // Before anything is staged: a build is the only thing that ever looks
        // at this directory for the intermediates a killed build left in it.
        sweep_intermediates_and_say(&embed_dir, "style index");
        let vocab_path = vocab_scratch_path(&embed_dir, "raw");
        if let Some(opts) = embedder.as_mut()
            && std::fs::create_dir_all(&embed_dir).is_ok()
            && std::fs::write(&vocab_path, LOOK_VOCAB.join("\n")).is_ok() {
                opts.vocab_file = Some(vocab_path.clone());
        }
        let total = pairs.len();
        // What this machine has ALREADY measured (`crate::style_cache`):
        // loaded once here, read by every worker, republished at the end with
        // the keys this build used.
        let cache_path = crate::style_cache::cache_path_in(&embed_dir);
        let cache = crate::style_cache::ExemplarCache::load(&cache_path, CACHE_BANDS);
        // WHICH passes this build is asking for. The decode-skipping fast path
        // is all-or-nothing per photograph against exactly these: a partial
        // hit that skipped the decode would leave a record with no staged
        // frame and therefore no way ever to embed or describe it.
        let want_embed = embedder.is_some();
        let want_desc = want_embed && describe.on() && describer.available();
        let provenance = embed_provenance_string();
        // What this library's mask content COST on the way in, by named reason
        // (S3). An atomic per reason rather than a channel field: the losses
        // are a property of the BUILD, not of any one exemplar, and the two
        // producers (`build_reporting`'s pool) only ever add.
        let loss_counts: [AtomicU32; crate::xmp::MaskImportReason::ALL.len()] =
            std::array::from_fn(|_| AtomicU32::new(0));
        let mut slots: Vec<Option<BuiltRecord>> = Vec::new();
        slots.resize_with(total, || None);
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let workers = total.clamp(1, decode::MAX_CONCURRENT_DECODES);
        report(on_progress, BuildStage::Frames, 0, total);
        std::thread::scope(|s| {
            let (tx, rx) = std::sync::mpsc::channel();
            for _ in 0..workers {
                let tx = tx.clone();
                let (pairs, next, done) = (&pairs, &next, &done);
                let (embedder, embed_dir) = (&embedder, &embed_dir);
                let loss_counts = &loss_counts;
                let (cache, provenance) = (&cache, provenance.as_str());
                s.spawn(move || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((raw, sidecar)) = pairs.get(i).map(|(r, s)| (*r, s)) else { break };
                    // --- UNDER THE DECODE PERMIT: the decode itself, and the
                    // staging of the embedding frame FROM the decoded buffer.
                    // Nothing else. R28 Batch-4 4b (adjudication F3) shortened
                    // this scope from what it used to be — it ran to the end of
                    // the photo, so a non-decode process (the sidecar, seconds
                    // of model load) sat inside the DECODE budget and the
                    // ~181 MB preview stayed alive underneath it. Since S2 no
                    // sidecar runs in this loop at all.
                    //
                    // The staging stays INSIDE, deliberately: it reads the
                    // preview, so releasing first would let this worker hold
                    // 181 MB while another started a fresh decode — and across
                    // two concurrent builds (the web server's request threads)
                    // that is exactly the ~1.4 GB stack `MAX_CONCURRENT_DECODES`
                    // exists to bound. Inside, the invariant is unchanged: a
                    // 61 MP preview is alive only while its permit is.
                    // `saved_quarter_turns` is part of the FILE's identity
                    // (the decode below composes it), so it is read before the
                    // cache is asked, not inside the decode call.
                    let turns = crate::store::saved_quarter_turns(raw);
                    let stamp = crate::style_cache::SourceStamp::of(raw, turns);
                    // Already measured, unchanged, and able to answer
                    // everything this build wants? Then this photograph costs
                    // NOTHING: no decode, no staged frame, no model call. The
                    // 14 features are a function of the FILE (EXIF, histogram,
                    // the saved rotation), which is why the stamp — and not
                    // the frame digest alone — is what unlocks this.
                    let warm = stamp
                        .as_ref()
                        .and_then(|s| cache.warm(s))
                        .and_then(|(digest, entry)| {
                            let feat: [f32; NDIM] = entry.feat.as_slice().try_into().ok()?;
                            cache_answers_everything(entry, want_embed, want_desc, provenance)
                                .then_some((digest.to_string(), entry, feat))
                        });
                    if let Some((digest, entry, feat)) = warm {
                        let record = read_exemplar(raw, sidecar, feat, loss_counts, |ex| {
                            apply_cached(ex, entry, want_embed, want_desc, provenance)
                        });
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if n % 20 == 0 {
                            println!("  {} / {}", n, total);
                        }
                        let _ = tx.send((
                            i,
                            record.map(|ex| BuiltRecord {
                                ex,
                                frame: None,
                                digest: Some(digest),
                                stamp,
                            }),
                        ));
                        continue;
                    }
                    let permit = decode::DecodePermit::acquire();
                    // Destructured, not bound whole: bound as `d`, the entire
                    // `Decoded` — rawler's sensor buffers included — would stay
                    // alive across the staging below, when all this loop wants
                    // is `meta`, `histogram` and (~181 MB at 61 MP) `preview`.
                    // In the frame the photo's own saved develop asks for
                    // (R27): the aspect feature is a retrieval discriminator
                    // with weight 1.5, so indexing a hand-rotated shot as the
                    // landscape it no longer is biases every neighbour it
                    // answers. `saved_quarter_turns` is one small read beside
                    // a ~1 s decode, and answers 0 for the common case (a
                    // foreign Lightroom library this app has never developed).
                    let staged = match decode::decode_raw_turned(raw, turns) {
                        Ok(decode::Decoded { meta, histogram, preview, .. }) => {
                            let feat = feature_vector(&meta, &histogram);
                            // 181 MB in, 200 KB out: after this the sidecars
                            // need a PATH, not the buffer.
                            let frame = embedder.as_ref().and_then(|_| {
                                match stage_embed_frame(&preview, embed_dir, &format!("idx-{i}")) {
                                    Ok(f) => Some(f),
                                    // DEGRADE, never fail — see the sidecar arm
                                    // below; the two failures are one outcome.
                                    Err(e) => {
                                        note_no_embedding(raw, &e);
                                        None
                                    }
                                }
                            });
                            drop(preview);
                            Some((feat, frame))
                        }
                        Err(e) => {
                            // println!/eprintln! are line-atomic, so per-photo
                            // prints stay whole across workers.
                            eprintln!("  skip {}: {e}", pipeline::stem(raw));
                            None
                        }
                    };
                    // --- OUT of the decode budget. Everything below is a
                    // small file read; the model sidecars run once each, after
                    // the pool has finished.
                    drop(permit);
                    // The sidecar half is the SAME work the warm path above
                    // does, through the same door — `fill` is a no-op here
                    // because a decoded record's content key is not known
                    // until its frame has been hashed, which happens once for
                    // the whole build after the pool.
                    let ex = staged.and_then(|(feat, frame)| {
                        read_exemplar(raw, sidecar, feat, loss_counts, |_| {})
                            .map(|ex| BuiltRecord { ex, frame, digest: None, stamp })
                    });
                    // Progress counts COMPLETED photos (completion order differs
                    // from index order under parallelism) so it stays monotonic.
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 20 == 0 {
                        println!("  {} / {}", n, total);
                    }
                    let _ = tx.send((i, ex));
                });
            }
            drop(tx); // workers hold the remaining senders; rx ends when they exit
            let mut received = 0usize;
            for (i, ex) in rx {
                slots[i] = ex;
                received += 1;
                report(on_progress, BuildStage::Frames, received, total);
            }
        });
        // Failed decodes left None slots — drop them in order, like the serial
        // `continue` did.
        let mut exemplars: Vec<StyleExemplar> = Vec::with_capacity(total);
        let mut frames: Vec<Option<StagedFrame>> = Vec::with_capacity(total);
        // The CONTENT key of each record, and the identity of the file it came
        // from: carried from the cache when the decode was skipped, filled in
        // from the staged frame below otherwise.
        let mut digests: Vec<Option<String>> = Vec::with_capacity(total);
        let mut stamps: Vec<Option<crate::style_cache::SourceStamp>> = Vec::with_capacity(total);
        // How many photographs cost NOTHING this build. Reported, because the
        // fast path is the one place that trusts an mtime.
        let mut reused = 0usize;
        for record in slots.into_iter().flatten() {
            if record.frame.is_none() && record.digest.is_some() {
                reused += 1;
            }
            exemplars.push(record.ex);
            frames.push(record.frame);
            digests.push(record.digest);
            stamps.push(record.stamp);
        }
        let live = exemplars.len();
        // Every staged frame's key, once for the whole build.
        for (slot, digest) in frame_digests(&frames, "style index").into_iter().enumerate() {
            if digest.is_some() {
                digests[slot] = digest;
            }
        }
        // A photograph whose FILE identity moved but whose PIXELS did not — a
        // rename, a copy, a `touch`, a re-pointed library — had to decode, and
        // its content key only became knowable just now. Its model answers are
        // still this build's answers, and it must not pay for them twice —
        // when they ARE this build's: a vector from another checkpoint is
        // refused here exactly as the warm path refuses it, and the embedding
        // stage below measures that record again.
        for ((ex, digest), frame) in exemplars.iter_mut().zip(&digests).zip(&frames) {
            if frame.is_none() {
                continue; // came whole out of the cache, already complete
            }
            let Some(entry) = digest.as_deref().and_then(|d| cache.get(d)) else { continue };
            apply_cached(ex, entry, want_embed, want_desc, &provenance);
        }
        // What the index LEARNED about this library's local work, and what it
        // could not read, in one line each (S3). The second line is the honest
        // half: a habit summarised from masks the importer refused is a
        // partial claim, and the build says so instead of the user finding out
        // from a reference block that under-counts.
        {
            use crate::mask_habit::Bucket;
            let carried = exemplars.iter().filter(|e| e.masks.is_some()).count();
            let habits: Vec<crate::mask_habit::MaskHabit> =
                exemplars.iter().filter_map(|e| e.masks).collect();
            let worked = habits.iter().filter(|h| h.count > 0).count();
            let masks: u32 = habits.iter().map(|h| h.count as u32).sum();
            let refined: u32 = habits.iter().map(|h| h.refined as u32).sum();
            let per = |b| habits.iter().map(|h| h.bucket(b).n as u32).sum::<u32>();
            println!(
                "  style index: {worked} of {carried} exemplar(s) carry local mask work \
                 ({masks} mask(s): sky {}, subject {}, ground {}, range {}, other {}; \
                 {refined} refined by a range mask)",
                per(Bucket::Sky),
                per(Bucket::Subject),
                per(Bucket::Ground),
                per(Bucket::Range),
                per(Bucket::Other),
            );
            let unread: Vec<String> = crate::xmp::MaskImportReason::ALL
                .iter()
                .zip(loss_counts.iter())
                .filter_map(|(r, n)| {
                    let n = n.load(Ordering::Relaxed);
                    (n > 0).then(|| format!("{} x{n}", r.en()))
                })
                .collect();
            if !unread.is_empty() {
                println!(
                    "  style index: mask content this build could not carry whole — {} \
                     (the local-work habit is summarised from what it could read)",
                    unread.join(", ")
                );
            }
        }
        if let Some(opts) = embedder.as_ref() {
            // STAGE 2 — ONE SigLIP image call for the whole library.
            report(on_progress, BuildStage::Embed, 0, live);
            // Only the records the cache did NOT answer. When it answered
            // every one, `embed_frames` starts no sidecar at all — which is
            // the 1.50 GB checkpoint a rebuild used to load for nothing.
            let want: Vec<bool> = exemplars.iter().map(|e| e.embed.is_none()).collect();
            match embed_frames(opts, &embed_dir, &frames, &want, "style index") {
                Ok(records) => {
                    for (ex, record) in exemplars.iter_mut().zip(records) {
                        // DEGRADE, never fail: a photo without a vector keeps
                        // its 14-dim exemplar and the index becomes a
                        // legitimate mixed one (see `retrieve_with_embed`). The
                        // GUI COUNTS these (R28 4b) — an all-failed build used
                        // to land the same toast as an all-embedded one.
                        let Some(record) = record else { continue };
                        ex.vocab_scores = record.vocab_scores;
                        ex.embed = Some(record.vector);
                    }
                }
                Err(e) => eprintln!(
                    "  style index: no style embeddings ({e:#}) — every exemplar is indexed on \
                     the 14-dim feature alone"
                ),
            }
            report(on_progress, BuildStage::Embed, live, live);
            // STAGE 3 — ONE Qwen call over the frames that are not already
            // described.
            attach_descriptions(
                &describer,
                describe,
                &embed_dir,
                &crate::describe::cache_path_in(&embed_dir),
                &library_key(dir),
                &digests,
                &frames,
                &mut exemplars,
                "style index",
                on_progress,
            );
            // The TAGS, once — over the finished population, because since v6
            // they are derived against its mean (`retag`). It runs after the
            // description stage so a record that gained prose there is judged
            // on the text it now says, and before the text stage so that stage
            // embeds the final text.
            retag(&mut exemplars);
            // …and only now may a cached description VECTOR be served, for the
            // same reason: `desc_text` reads the tags the line above settled.
            if want_embed {
                for (ex, digest) in exemplars.iter_mut().zip(&digests) {
                    if let Some(entry) = digest.as_deref().and_then(|d| cache.get(d)) {
                        adopt_cached_desc_embed(ex, entry, &provenance);
                    }
                }
            }
            // STAGE 4 — ONE SigLIP text call for the whole library. It runs
            // AFTER the three above because the text it embeds is what they
            // just produced, and because one call is the entire point.
            report(on_progress, BuildStage::Text, 0, live);
            attach_desc_embeddings(opts, &embed_dir, &mut exemplars, "style index");
            report(on_progress, BuildStage::Text, live, live);
        }
        drop(frames);
        // What this build MEASURED, for the next one. Written after the model
        // stages so every entry carries this build's finished answers, and
        // only for records whose content key is known.
        let mut keep: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut next = crate::style_cache::ExemplarCache::default();
        for ((ex, digest), stamp) in exemplars.iter().zip(&digests).zip(&stamps) {
            let (Some(digest), Some(stamp)) = (digest.clone(), stamp.clone()) else { continue };
            let prior = cache.get(&digest);
            keep.insert(digest.clone());
            next.insert(digest, cache_entry(ex, stamp, prior, want_desc, &provenance));
        }
        // …and only by a build whose keep-set is complete enough to prune to
        // (`cache_is_publishable`): a `style-index` without the embedding pass
        // leaves the file as the last embedding build wrote it, and retires
        // nothing.
        let retired = if cache_is_publishable(want_embed, &keep) {
            if let Err(e) = next.save(&cache_path, &keep) {
                eprintln!("  style index: the exemplar cache could not be published ({e:#})");
            }
            cache.retired(&keep)
        } else {
            0
        };
        println!(
            "  style index cache: reused {reused}, recomputed {}, removed {retired}, \
             skipped-for-sidecar {}",
            live - reused,
            unpaired.len()
        );
        // Mean and σ come from the MERGED exemplar set, always: `compute_norm`
        // averages over every exemplar, so a library that gained or lost ONE
        // photograph legitimately moves every z-scored dimension. Re-derived
        // from the stored features rather than cached with them, because a
        // cached normalisation would be the one number in this file that no
        // longer describes the set it is used on.
        let (mean, std) = compute_norm(&exemplars);
        // Record where this index was built from, for UI provenance / other users.
        let source_dir = std::path::absolute(dir).map(|p| p.display().to_string()).ok();
        // v2: exemplars now carry tint/saturation/dehaze + tone-curve shape.
        let _ = std::fs::remove_file(&vocab_path);
        let embed_provenance = embedder
            .as_ref()
            .filter(|_| exemplars.iter().any(|e| e.embed.is_some()))
            .map(|_| embed_provenance_string());
        Ok(StyleIndex { version: CURRENT_INDEX_VERSION, mean, std, exemplars, source_dir, looks: Vec::new(), looks_dir: None, embed_provenance })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // An empty index is a FAILED build, not a result: `fs::write`
        // truncates in place, so saving it would silently destroy a good index
        // that took an hour to build (every surface's Style slider then goes
        // inert with nothing to say why). The web handler had this guard; the
        // CLI didn't — enforcing it HERE covers every caller for good.
        if self.exemplars.is_empty() && self.looks.is_empty() {
            anyhow::bail!(
                "refusing to save an EMPTY style index over {} — no RAW had its .xmp sidecar \
                 beside it (AutoShade keeps its own .xmp in the develop store, never beside your \
                 RAWs, so an AutoShade output folder always yields 0). Point the build at your \
                 Lightroom-edited folder; the existing index was left untouched.",
                path.display()
            );
        }
        pipeline::ensure_parent(path)?;
        let mut value = self.clone();
        if self.exemplars.is_empty() || self.looks.is_empty() {
            match Self::load(path) {
                Ok(existing) => {
                    // The provenance stamp travels with the VECTORS it
                    // describes — `build_reporting` stamps only an index that
                    // holds one. A build with none of its own (`style-index`
                    // without `--embed`) used to rewrite the file with `null`
                    // over the looks it had just merged in, and `load`'s
                    // vocabulary check then had nothing left to read.
                    let (mut merged_raw, mut merged_looks) = (false, false);
                    if value.exemplars.is_empty() {
                        merged_raw = existing.exemplars.iter().any(|e| e.embed.is_some());
                        value.exemplars = existing.exemplars; value.mean = existing.mean; value.std = existing.std; value.source_dir = existing.source_dir;
                    }
                    if value.looks.is_empty() {
                        merged_looks = !existing.looks.is_empty();
                        value.looks = existing.looks; value.looks_dir = existing.looks_dir;
                    }
                    let merged_vectors = merged_raw || merged_looks;
                    if value.embed_provenance.is_none() && merged_vectors {
                        value.embed_provenance = existing.embed_provenance;
                    } else if merged_vectors
                        && let (Some(ours), Some(theirs)) =
                            (value.embed_provenance.as_deref(), existing.embed_provenance.as_deref())
                        && model_of(ours) != model_of(theirs)
                    {
                        // ONE stamp cannot describe two populations embedded by
                        // two models, and until 2026-09-24 the fresh half's stamp
                        // was kept over vectors the other model wrote — a query
                        // vector from this build compared against them is a
                        // number, not a similarity. The merged half keeps its
                        // features and settings and loses its vectors, said out
                        // loud; the next `--embed` build restores them.
                        let (kept, lost) = (model_of(ours), model_of(theirs));
                        if merged_raw {
                            for e in &mut value.exemplars {
                                e.embed = None;
                                e.desc_embed = None;
                            }
                            eprintln!(
                                "existing style index {}: its {} RAW image vector(s) were embedded by {lost} \
                                 and this build embeds with {kept} — they are dropped (features and settings \
                                 stand); rebuild the index with --embed to score them again",
                                path.display(),
                                value.exemplars.len()
                            );
                        }
                        if merged_looks {
                            eprintln!(
                                "existing style index {}: its {} look record(s) were embedded by {lost} and \
                                 this build embeds with {kept} — they are dropped; rebuild the look library \
                                 with --embed",
                                path.display(),
                                value.looks.len()
                            );
                            value.looks.clear();
                            value.looks_dir = None;
                        }
                    }
                }
                Err(err) if path.exists() => {
                    eprintln!("existing style index {} is unusable ({err:#}); replacing it", path.display());
                }
                Err(_) => {}
            }
        }
        // Publish atomically (tmp + rename): fs::write truncates in place, so
        // a disk-full/interrupt mid-write left the previous good index as a
        // corrupt partial file — the empty-index guard above can't catch that.
        // pid+seq-unique tmp: a pid-only name still collided between two
        // concurrent builders in the SAME process (the web server's threads),
        // and a FIXED name let cross-process builders truncate each other.
        static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "json.tmp{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&tmp, serde_json::to_string(&value)?)
            .with_context(|| format!("write style index {}", tmp.display()))?;
        // NO pre-remove: rename replaces the destination on Windows too, and
        // deleting first meant a failed rename (or a crash between the two)
        // had already destroyed the last good index.
        // DURABLE replace (L03): staged-bytes fsync + parent-dir fsync
        // around the rename — tmp+rename alone leaves a post-crash window
        // where the live name points at bytes the disk never received,
        // which is exactly the corrupt-index state this staging exists to
        // prevent.
        if let Err(e) = crate::store::durable_replace(&tmp, path) {
            let _ = std::fs::remove_file(&tmp); // don't leak the staging file
            return Err(e)
                .with_context(|| format!("publish style index {}", path.display()));
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<StyleIndex> {
        use std::io::Read as _;

        let file = std::fs::File::open(path)
            .with_context(|| format!("read style index {}", path.display()))?;
        let mut bytes = Vec::new();
        file.take((MAX_STYLE_INDEX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("read style index {}", path.display()))?;
        if bytes.len() > MAX_STYLE_INDEX_BYTES {
            anyhow::bail!(
                "style index {} exceeds the {}-byte limit",
                path.display(),
                MAX_STYLE_INDEX_BYTES
            );
        }
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("style index {} is not UTF-8", path.display()))?;
        let mut idx: StyleIndex = serde_json::from_str(text)
            .with_context(|| format!("parse style index {}", path.display()))?;
        // Version gate: v3 = display-frame Meta dims (the portrait feature).
        // An older index recorded portrait RAWs as landscape — serving it
        // silently would bias every retrieval until a manual rebuild.
        //
        // v5 does NOT refuse v4 (see `READABLE_INDEX_VERSIONS`): the embedding
        // block is additive and an absent one contributes exactly nothing, so
        // a v4 index still ranks the way it always did. What is refused is a
        // version whose 14 FEATURES mean something else.
        if !READABLE_INDEX_VERSIONS.contains(&idx.version) {
            anyhow::bail!(
                "style index {} is version {} (this build reads {:?}) — rebuild it: \
                 autoshade style-index <dir>",
                path.display(),
                idx.version,
                READABLE_INDEX_VERSIONS
            );
        }
        if idx.exemplars.is_empty() && idx.looks.is_empty() {
            anyhow::bail!("style index {} contains no exemplars", path.display());
        }
        if idx.exemplars.len() > MAX_STYLE_EXEMPLARS {
            anyhow::bail!(
                "style index {} contains {} exemplars (limit {})",
                path.display(),
                idx.exemplars.len(),
                MAX_STYLE_EXEMPLARS
            );
        }
        if idx.looks.len() > MAX_LOOK_EXEMPLARS {
            anyhow::bail!(
                "style index {} contains {} look exemplars (limit {})",
                path.display(), idx.looks.len(), MAX_LOOK_EXEMPLARS
            );
        }
        // The stored vocabulary version, ENFORCED. It was recorded in
        // `embed_provenance` from the start and checked nowhere: a phrase list
        // that changed meaning would have left every `vocab_scores` array (and
        // so every derived tag, and so every description vector) describing a
        // vocabulary this build no longer has, while the file looked fine.
        //
        // The LOOKS are dropped, not the index: the RAW half's features,
        // settings and image vectors are unaffected by the phrase list, and
        // refusing the whole file would cost a user their hour-long RAW build
        // over the half of it that is cheap to rebuild. What the RAW half DOES
        // carry from the phrase list — its `vocab_scores`, the tags derived
        // from them, and a description vector that was the vector OF those
        // tags — goes the same way the looks do, so nothing served describes
        // a vocabulary this build cannot name; the image vectors and the
        // settings stand, and the next `--embed` build scores them again.
        if let Some(stored) = idx.embed_provenance.as_deref().and_then(vocab_version_of)
            && stored != LOOK_VOCAB_VERSION
        {
            if !idx.looks.is_empty() {
                eprintln!(
                    "style index {} was built with look vocabulary v{stored} and this build speaks \
                     v{LOOK_VOCAB_VERSION} — its {} look record(s) are being ignored; rebuild the \
                     look library (autoshade style-index --looks <dir> --embed) to use them again",
                    path.display(),
                    idx.looks.len()
                );
                idx.looks.clear();
                idx.looks_dir = None;
            }
            let scored = idx
                .exemplars
                .iter()
                .filter(|e| e.vocab_scores.is_some() || !e.tags.is_empty())
                .count();
            if scored > 0 {
                eprintln!(
                    "style index {} was built with look vocabulary v{stored} and this build speaks \
                     v{LOOK_VOCAB_VERSION} — {scored} RAW exemplar(s) lose their vocabulary scores \
                     and attribute tags (features, settings and image vectors stand); rebuild the \
                     index (autoshade style-index <dir> --embed) to score them again",
                    path.display()
                );
                for e in &mut idx.exemplars {
                    // `retag`'s own rule: a vector is the vector OF a text, and
                    // a record whose text was its tag string stops saying it.
                    let before = desc_text(e.desc.as_deref(), &e.tags);
                    e.vocab_scores = None;
                    e.tags.clear();
                    if before != desc_text(e.desc.as_deref(), &e.tags) {
                        e.desc_embed = None;
                    }
                }
            }
        }
        if idx.mean.len() != NDIM || idx.std.len() != NDIM {
            anyhow::bail!(
                "style index {} has normalization vectors with the wrong dimension",
                path.display()
            );
        }
        // Finite AND bounded (L04-3): a finite 1e38 mean overflowed
        // `(v - mean)/std` to ±inf with the guaranteed-small divisor, and
        // the retrieval sort then ordered NaN keys. With these bands the
        // worst normalize() output is (1e3+1e3)/1e-4 = 2e7 — ~22 decades of
        // f32 headroom on the summed distance.
        if !idx.mean.iter().all(|v| v.is_finite() && v.abs() <= MAX_FEATURE_ABS)
            || !idx.std.iter().all(|v| v.is_finite() && (1e-4..=MAX_FEATURE_ABS).contains(v))
        {
            anyhow::bail!(
                "style index {} has invalid normalization values (each must be finite \
                 and within ±{MAX_FEATURE_ABS})",
                path.display()
            );
        }

        for (i, exemplar) in idx.exemplars.iter_mut().enumerate() {
            if exemplar.feat.len() != NDIM {
                anyhow::bail!(
                    "style index {} exemplar {i} has {} features (expected {NDIM})",
                    path.display(),
                    exemplar.feat.len()
                );
            }
            if !exemplar_is_finite(exemplar) {
                anyhow::bail!(
                    "style index {} exemplar {i} contains a non-finite number",
                    path.display()
                );
            }
            // The RAW half's tags reach the prompt exactly as a look's do, so
            // they meet the look door's bound (`LOOK_TAGS_K` phrases of at most
            // 128 chars) — TRUNCATED rather than refused, like `desc` below,
            // because an overlong tag is not worth an hour-long rebuild. Until
            // 2026-09-24 a record without `vocab_scores` (which `retag` would
            // have rewritten) carried arbitrary tags to the prompt.
            exemplar.tags.truncate(LOOK_TAGS_K);
            for t in &mut exemplar.tags {
                if t.chars().count() > 128 {
                    *t = t.chars().take(128).collect();
                }
            }

            // The tag is free text that reaches the model prompt; only the
            // exact `derive_tag` vocabulary is a tag, anything else is not.
            let mut tag = exemplar.tag.split('/');
            let valid_tag = matches!(
                tag.next(),
                Some("ultrawide" | "wide" | "normal" | "tele")
            ) && matches!(tag.next(), Some("dark" | "mid" | "bright"))
                && matches!(tag.next(), Some("night" | "goldenish" | "midday"))
                && matches!(tag.next(), Some("portrait" | "landscape"))
                && tag.next().is_none();
            if !valid_tag {
                anyhow::bail!(
                    "style index {} exemplar {i} has an invalid scene tag",
                    path.display()
                );
            }

            // One table for every label `read_settings` can write
            // (`setting_bands`): the recipe's own clamp() bands, so a stored
            // exemplar can never carry a value the engine would re-clamp, and
            // a label the writer never produces is refused as before.
            for (key, value) in &mut exemplar.settings {
                let Some(&(lo, hi)) = setting_bands().get(key.as_str()) else {
                    anyhow::bail!(
                        "style index {} exemplar {i} has an unsupported setting key",
                        path.display()
                    )
                };
                *value = value.clamp(lo, hi);
            }

            if let Some(curve) = &mut exemplar.curve {
                curve[0] = curve[0].clamp(0.0, 255.0);
                // These are the extrema of (out@191 - 191) - (out@64 - 64)
                // when both curve outputs remain in 0..=255.
                curve[1] = curve[1].clamp(-382.0, 128.0);
            }

            // The family summary reaches the prompt like everything else here,
            // so it gets the same door treatment (means bounded to 0..100, the
            // curve count to 3).
            if let Some(families) = &mut exemplar.families {
                families.clamp();
            }
            // …and the local-work habit, on the same rule: it reaches the same
            // prompt through the same block, so it takes the same door (S3).
            if let Some(masks) = &mut exemplar.masks {
                masks.clamp();
            }
        }

        for (i, look) in idx.looks.iter_mut().enumerate() {
            let norm = look.embed.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>().sqrt();
            if look.embed.len() != crate::embed::EMBED_DIM
                || !look.embed.iter().all(|v| v.is_finite())
                || (norm - 1.0).abs() > 1e-3
            {
                anyhow::bail!("style index {} look exemplar {i} has an invalid embedding", path.display());
            }
            if look.vocab_scores.as_ref().is_some_and(|v| v.len() != LOOK_VOCAB.len() || !v.iter().all(|x| x.is_finite())) {
                anyhow::bail!("style index {} look exemplar {i} has invalid vocabulary scores", path.display());
            }
            if look.tags.len() > LOOK_TAGS_K || look.tags.iter().any(|t| t.chars().count() > 128) {
                anyhow::bail!("style index {} look exemplar {i} has invalid tags", path.display());
            }
            if let Some(desc) = &mut look.desc
                && desc.chars().count() > MAX_DESC_CHARS {
                    *desc = desc.chars().take(MAX_DESC_CHARS).collect();
            }
            if look.desc_embed.as_ref().is_some_and(|v| {
                v.len() != crate::embed::EMBED_DIM
                    || !v.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3)
                    || (v.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt() - 1.0).abs() > 1e-3
            }) {
                anyhow::bail!("style index {} look exemplar {i} has invalid description embedding", path.display());
            }
        }

        // The TAGS are RE-DERIVED from the file's own scores, never taken as
        // written. Since v6 a tag list is a statement about the library mean
        // (`tags_from_scores`), so a v5 file's stored lists are the raw argmax
        // and a v6 file's are already these — the pass is idempotent on the
        // second, which is what lets one door serve both versions. Each
        // population is centred on itself, and a description vector whose text
        // the new tags no longer spell is dropped rather than served.
        retag(&mut idx.exemplars);
        retag(&mut idx.looks);
        Ok(idx)
    }

    /// k nearest exemplars to (meta,hist), excluding the query photo itself
    /// when it is a corpus member (see [`is_self`]).
    ///
    /// The 14-dim block only. [`retrieve_with_embed`] is the same call with a
    /// query embedding; this one is exactly that call with `None`, so a caller
    /// that has no vector (and an index that carries none) ranks precisely as
    /// it did before R27 Batch-5.
    pub fn retrieve(&self, meta: &Meta, hist: &Histogram, k: usize, exclude: &Path) -> Vec<&StyleExemplar> {
        // The weights are immaterial with no query vectors — every cosine term
        // is absent — so the shipped ones are passed rather than a second
        // "no weights" spelling that could drift from them.
        self.retrieve_with_embed(meta, hist, StyleQuery::FEATURES_ONLY, k, exclude)
    }

    /// [`retrieve`](StyleIndex::retrieve) with the query photo's SigLIP 2
    /// embedding folded in as a second distance block.
    ///
    /// Tolerant in both directions by construction: `query_embed = None`, or an
    /// exemplar with no `embed`, contributes 0 to that pair's distance
    /// ([`embed_distance`]). So one index may legitimately hold a mix of
    /// exemplars with and without vectors — which is what happens when a build
    /// runs with the sidecar on and one photo's sidecar call fails.
    ///
    /// KNOWN, DELIBERATE ASYMMETRY: in such a mixed index the exemplars WITHOUT
    /// a vector get a 0 cosine term while the ones with a vector get
    /// `W_EMB·(1−cos) ≥ 0`, so a vector-less exemplar is never penalised and is
    /// mildly favoured. Normalising that away would mean inventing a distance
    /// for a comparison we did not make; the honest reading is that a missing
    /// vector is "no evidence", and no evidence does not push a candidate down.
    pub fn retrieve_with_embed(
        &self,
        meta: &Meta,
        hist: &Histogram,
        query: StyleQuery<'_>,
        k: usize,
        exclude: &Path,
    ) -> Vec<&StyleExemplar> {
        let mut scored = self.score_candidates(meta, hist, query, exclude);
        // total_cmp, never partial_cmp-with-Equal-fallback: a NaN key made
        // the comparator non-transitive — std documents an UNSPECIFIED
        // order (and may panic) for a non-total comparator, i.e. a silently
        // scrambled style reference. total_cmp orders every bit pattern.
        scored.sort_by(|a, b| a.1.total().total_cmp(&b.1.total()));
        scored.into_iter().take(k).map(|(e, _)| e).collect()
    }

    /// Every candidate this query admits, with its terms, in INDEX order.
    ///
    /// The one place the distance is computed. `retrieve_with_embed` sorts it,
    /// `distance_components` reads one row out of it, and neither can drift
    /// from the other — which matters more than it used to, because the text
    /// terms are standardised over the candidate SET and so cannot be computed
    /// one candidate at a time at all.
    pub(super) fn score_candidates(
        &self,
        meta: &Meta,
        hist: &Histogram,
        query: StyleQuery<'_>,
        exclude: &Path,
    ) -> Vec<(&StyleExemplar, DistanceTerms)> {
        let StyleQuery { image: query_embed, text: query_text, weights } = query;
        let q = normalize(feature_vector(meta, hist), &self.mean, &self.std);
        let ex_path = std::path::absolute(exclude)
            .unwrap_or_else(|_| exclude.to_path_buf())
            .display()
            .to_string();
        let ex_stem = pipeline::stem(exclude);
        let candidates: Vec<&StyleExemplar> = self
            .exemplars
            .iter()
            .filter(|e| !is_self(e, &ex_path, ex_stem) && e.feat.len() == NDIM)
            .collect();
        let txt_gaps: Vec<Option<f64>> =
            candidates.iter().map(|e| cosine_gap(query_text, e.embed.as_deref())).collect();
        let desc_gaps: Vec<Option<f64>> =
            candidates.iter().map(|e| cosine_gap(query_text, e.desc_embed.as_deref())).collect();
        // The exemplars' IMAGE vectors carry the hubness this corrects, and
        // `vocab_scores` is the stored measurement of it. The DESCRIPTION term
        // gets `None` — see `standardise`.
        let hubs = hubness_profile(candidates.iter().map(|e| e.vocab_scores.as_deref()));
        let txt = standardise(&txt_gaps, hubs.as_deref(), weights.txt);
        let desc = standardise(&desc_gaps, None, weights.desc);
        candidates
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut ef = [0.0f32; NDIM];
                ef.copy_from_slice(&e.feat);
                let en = normalize(ef, &self.mean, &self.std);
                // f64 accumulator (L04-3, second belt): even a bounds-check
                // bypass cannot overflow ((2·3.4e38)² · 1.5 · 14 ≈ 1e79 ≪
                // 1.8e308), so the ranking stays deterministic; on real data
                // this only removes f32 rounding in the sum.
                let d14 = (0..NDIM)
                    .map(|j| {
                        let d = (q[j] - en[j]) as f64;
                        WEIGHTS[j] as f64 * d * d
                    })
                    .sum::<f64>();
                // ADDED, never folded in: the 14-dim sum above is untouched, so
                // setting `W_EMB = 0` leaves every historical ranking bit-for-bit.
                (
                    *e,
                    DistanceTerms {
                        d14,
                        emb: embed_distance(query_embed, e.embed.as_deref(), weights.emb),
                        txt: txt.terms[i],
                        desc: desc.terms[i],
                        txt_gap: txt_gaps[i],
                        desc_gap: desc_gaps[i],
                        txt_standardised: txt.standardised,
                        desc_standardised: desc.standardised,
                        // `None` for a pair with no cosine even while the
                        // correction is in force: `standardise` leaves such a
                        // gap `None`, so nothing was removed from it, and the
                        // terms line says `hub` bare rather than naming a
                        // number that corrected nothing.
                        txt_hub: if txt.hub_corrected { txt_gaps[i].and(hubs.as_ref().map(|h| h[i])) } else { None },
                        txt_hub_corrected: txt.hub_corrected,
                    },
                )
            })
            .collect()
    }

    /// Retrieve finished-photo looks. Returns empty when no query embedding is
    /// available; callers disclose that condition instead of silently falling
    /// back to RAW exemplars.
    pub fn retrieve_looks(&self, query: StyleQuery<'_>, k: usize) -> Vec<&LookExemplar> {
        self.retrieve_looks_with_terms(query, k)
            .into_iter()
            .map(|(e, _)| e)
            .collect()
    }

    /// [`retrieve_looks`](Self::retrieve_looks) with the terms, for the
    /// diagnostic — the same call, so the printed numbers are the ranked ones.
    pub fn retrieve_looks_with_terms(
        &self,
        query: StyleQuery<'_>,
        k: usize,
    ) -> Vec<(&LookExemplar, DistanceTerms)> {
        let StyleQuery { image: query_img, text: query_text, weights } = query;
        if query_img.is_none() && query_text.is_none() {
            return Vec::new();
        }
        let txt_gaps: Vec<Option<f64>> =
            self.looks.iter().map(|e| cosine_gap(query_text, Some(&e.embed))).collect();
        let desc_gaps: Vec<Option<f64>> =
            self.looks.iter().map(|e| cosine_gap(query_text, e.desc_embed.as_deref())).collect();
        let hubs = hubness_profile(self.looks.iter().map(|e| e.vocab_scores.as_deref()));
        let txt = standardise(&txt_gaps, hubs.as_deref(), weights.txt);
        let desc = standardise(&desc_gaps, None, weights.desc);
        let mut scored: Vec<(&LookExemplar, DistanceTerms)> = self
            .looks
            .iter()
            .enumerate()
            .map(|(i, e)| {
                (
                    e,
                    DistanceTerms {
                        // A finished photo has no camera rendition, so it has
                        // no 14-dim feature and never gets a d14 term.
                        d14: 0.0,
                        emb: embed_distance(query_img, Some(&e.embed), weights.look),
                        txt: txt.terms[i],
                        desc: desc.terms[i],
                        txt_gap: txt_gaps[i],
                        desc_gap: desc_gaps[i],
                        txt_standardised: txt.standardised,
                        desc_standardised: desc.standardised,
                        txt_hub: if txt.hub_corrected { txt_gaps[i].and(hubs.as_ref().map(|h| h[i])) } else { None },
                        txt_hub_corrected: txt.hub_corrected,
                    },
                )
            })
            .collect();
        scored.sort_by(|a, b| a.1.total().total_cmp(&b.1.total()));
        scored.into_iter().take(k).collect()
    }

    /// The additive distance components used by retrieval, for ONE exemplar.
    /// Keeping this calculation public lets the offline diagnostic print
    /// exactly the terms the production path used instead of maintaining a
    /// second ranking path.
    ///
    /// It re-derives the whole candidate set because the text terms are
    /// standardised over it: asking for one candidate's term in isolation is
    /// not a question with an answer. An exemplar that is not IN the candidate
    /// set (the excluded query photo itself) answers all-zero terms.
    pub fn distance_components(
        &self,
        meta: &Meta,
        hist: &Histogram,
        query: StyleQuery<'_>,
        exclude: &Path,
        e: &StyleExemplar,
    ) -> DistanceTerms {
        self.score_candidates(meta, hist, query, exclude)
            .into_iter()
            .find(|(c, _)| std::ptr::eq(*c, e))
            .map(|(_, t)| t)
            .unwrap_or_default()
    }

    /// Did the DIRECTION take any part in choosing this look?
    ///
    /// Only when a direction produced a text vector AND at least one text
    /// weight is non-zero. At the shipped defaults both are 0, so the look is
    /// ranked by its image vector alone and a block claiming it was chosen
    /// "for this frame and direction" is a false receipt the model would then
    /// reason from — which is why this is a computed bit and not a sentence.
    pub fn look_ranked_by_direction(query: StyleQuery<'_>) -> bool {
        query.text.is_some() && (query.weights.txt > 0.0 || query.weights.desc > 0.0)
    }

    /// Render the look-library block appended to a proposer instruction.
    ///
    /// `by_direction` is [`Self::look_ranked_by_direction`] for the query that
    /// produced `looks`: it decides whether the block may say the direction had
    /// anything to do with the choice.
    pub fn render_look_reference(&self, looks: &[&LookExemplar], by_direction: bool) -> Option<String> {
        let first = looks.first()?;
        let tags = block_tags(&first.tags);
        // Through the SAME door the reference block uses (S2): a bare
        // `take(MAX_DESC_CHARS)` bounded the LENGTH and nothing else, so a
        // description carrying a newline could forge a line of this block.
        let desc = block_desc(first.desc.as_deref());
        let mut out = format!(
            "LOOK REFERENCE (from the photographer's LOOK LIBRARY — the finished photo closest to this frame{}; match its grade, not its content): [{}] look: {}",
            if by_direction { " and direction" } else { "" },
            first.stem.chars().take(MAX_STEM_CHARS).collect::<String>(), tags
        );
        if let Some(d) = desc.filter(|d| !d.is_empty()) {
            out.push_str("; ");
            out.push_str(&d);
        }
        Some(out)
    }

    /// The LOOK this retrieval is asking for, in ONE short line, for the
    /// DOWNSTREAM REVIEWERS (B2).
    ///
    /// The reference block itself is kilobytes of numbers and prose and it goes
    /// to the PROPOSER alone. The visual judge never saw any of it, so a
    /// deliberate look — the warm golden lean, the teal-and-orange split tone —
    /// reached the judge as an unexplained colour cast and was marked down as a
    /// flaw; six showcase runs came back with revision hints that were pure
    /// subtraction ("reduce aqua/blue saturation ~15", "lower green
    /// saturation") and the final global saturation landed at +2/-2. This is
    /// the smallest thing a reviewer needs in order to tell a look from a
    /// defect: the phrases, and nothing else.
    ///
    /// UNTRUSTED, like every other tag and description that reaches a prompt —
    /// the phrases come from the index, which is disk input. It is bounded here
    /// by construction (`block_tags` and `shared_look_tags` both go through the
    /// block's own doors) and fenced again at the consumer
    /// (`judge::intent_rubric`), for the same reason `block_desc` exists: two
    /// doors cost nothing and one of them is always the one that was forgotten.
    pub fn look_summary(looks: &[&LookExemplar], ex: &[&StyleExemplar]) -> Option<String> {
        let library = looks.first().map(|l| block_tags(&l.tags)).filter(|t| !t.is_empty());
        let ranked = shared_look_tags(ex);
        let shared = (!ranked.is_empty())
            .then(|| ranked.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", "));
        // The COUNTS stay out: "3/4" is the reference block's evidence for the
        // proposer, and a reviewer asked "is this look delivered?" would only
        // be invited to score the evidence instead of the photograph.
        match (library, shared) {
            (Some(l), Some(s)) => Some(format!(
                "{l} (the finished photo they picked out of their own look library); their \
similar past edits share: {s}"
            )),
            (Some(l), None) => Some(format!(
                "{l} (the finished photo they picked out of their own look library)"
            )),
            (None, Some(s)) => Some(format!("{s} (shared across their similar past edits)")),
            (None, None) => None,
        }
    }

    /// Render retrieved exemplars as a SOFT reference block for the advisor prompt.
    ///
    /// The pipeline passes the Style axis here for GATE 5 (R23-3); the
    /// historical `strength` parameter name remains for API compatibility.
    /// This block's two
    /// "…and not stronger / do not exceed it" clauses were the OTHER half of the
    /// binary style gate. Whatever the strength dial said, retrieving a reference
    /// re-imposed a ceiling — so the two sliders were not independent axes, and a
    /// user who built a library got MORE restraint by asking for more personal
    /// style. At [`StrengthTier::Committed`] the same measured habits become a
    /// FLOOR instead. The NUMBERS in this block never change: they are what the
    /// photographer actually did, and rewriting a measurement to match a dial
    /// would be a fabrication.
    pub fn render_reference(
        &self,
        ex: &[&StyleExemplar],
        strength: crate::recipe::GradeStrength,
    ) -> Option<String> {
        self.render_reference_voiced(ex, strength, StyleVoice::for_style(strength.get()))
    }

    /// [`Self::render_reference`] with the block's VOICE decided by the
    /// caller ([`StyleVoice`]) rather than by the Style axis alone — which is
    /// what lets a leading direction turn the same measurements into
    /// background instead of a target (v1.2.3).
    ///
    /// `strength` is still read for one thing only: the dial-allowance arm of
    /// the colour sentence, which quotes `style_colour_floor`. Every other
    /// use of it moved into the voice.
    pub fn render_reference_voiced(
        &self,
        ex: &[&StyleExemplar],
        strength: crate::recipe::GradeStrength,
        voice: StyleVoice,
    ) -> Option<String> {
        if ex.is_empty() {
            return None;
        }
        // The historical boolean, now DERIVED from the voice: one name for
        // "the habits are the target", so the four aim clauses below cannot
        // disagree about which voice is speaking.
        let bold = matches!(voice, StyleVoice::Target);
        let lines: Vec<String> = ex
            .iter()
            .map(|e| {
                // The block shows the TWELVE (`REF_KEYS`) and no more. Since
                // batch 2 the settings map ALSO carries the distillation
                // vocabulary — thirty-eight mixer and wheel keys read by
                // `style_targets` — and printing those would spend the block's
                // `advisor::REFERENCE_BUDGET_BYTES` on `key +0` pairs for bands
                // nobody touched, crowding out the descriptions, to tell the
                // model about colour families it is already told about as
                // summary statistics below. Filtering HERE rather than not
                // ingesting is also what makes an index built with the new
                // vocabulary render this block byte for byte like one built
                // without it (`the_reference_block_shows_only_the_printed_twelve`).
                let s: Vec<String> = e
                    .settings
                    .iter()
                    .filter(|(k, _)| REF_KEYS.iter().any(|(_, label)| *label == k.as_str()))
                    .map(|(k, v)| format!("{k} {v:+.0}"))
                    .collect();
                // `look: <tags> — <desc>` (S2): the tags stay FIRST because
                // they are a bounded vocabulary the proposer has seen in every
                // other block, and the prose is appended only when the
                // exemplar carries one. Both halves go through the same
                // bounds the index door applied — the description is model
                // output about the user's photograph, i.e. untrusted text
                // reaching a prompt, and a second door costs nothing.
                let desc = block_desc(e.desc.as_deref());
                let tags = block_tags(&e.tags);
                let look = match (tags.is_empty(), desc) {
                    (true, None) => String::new(),
                    (true, Some(d)) => format!(" · look: {d}"),
                    (false, None) => format!(" · look: {tags}"),
                    (false, Some(d)) => format!(" · look: {tags} — {d}"),
                };
                format!("[{}] {}{}", e.tag, s.join(", "), look)
            })
            .collect();
        // Average the retrieved exemplars' tone-curve SHAPE (those who drew one),
        // so the AI shapes its tone_curve the way this user habitually does.
        let curves: Vec<[f32; 2]> = ex.iter().filter_map(|e| e.curve).collect();
        let curve_note = if !curves.is_empty() {
            let n = curves.len() as f32;
            let bl = curves.iter().map(|c| c[0]).sum::<f32>() / n;
            let ss = curves.iter().map(|c| c[1]).sum::<f32>() / n;
            let aim = match voice {
                StyleVoice::Target => {
                    "— shape your `tone_curve` at least this strongly; you MAY go further."
                }
                StyleVoice::Ceiling => {
                    "— shape your `tone_curve` to a similar gentleness, not stronger."
                }
                StyleVoice::Background => {
                    "— their habit; the direction may ask for a different tone shape, and it leads."
                }
            };
            format!(
                "  THEIR TYPICAL MASTER TONE CURVE: black-lift {bl:+.0}, S-strength {ss:+.0} \
(0..255 scale) {aim}"
            )
        } else {
            String::new()
        };
        // The colour FAMILIES, as the same kind of averaged habit (R23-1). Only
        // over exemplars that carry a summary: a pre-R23 index has none, and an
        // all-neutral sidecar records none, so a zero here would be a claim we
        // did not measure.
        let fams: Vec<crate::eval::FamilySummary> = ex.iter().filter_map(|e| e.families).collect();
        let family_note = if fams.is_empty() {
            String::new()
        } else {
            let n = fams.len() as f32;
            let mean = |f: fn(&crate::eval::FamilySummary) -> f32| {
                fams.iter().map(f).sum::<f32>() / n
            };
            // The five shaping magnitudes, read ONCE: the sentence prints them
            // and the floor decision below reads them, so the block can never
            // promise a floor its own numbers do not support (B4).
            let (hue, sat, lum) = (mean(|f| f.hsl[0]), mean(|f| f.hsl[1]), mean(|f| f.hsl[2]));
            let (wheel_sat, wheel_lum) = (mean(|f| f.grade[0]), mean(|f| f.grade[1]));
            let shaped =
                [hue, sat, lum, wheel_sat, wheel_lum].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let aim = if matches!(voice, StyleVoice::Background) {
                // BACKGROUND states the same measurement and then hands the
                // decision over. Neither a ceiling ("do not exceed") nor a
                // floor is available to it: both are claims about what THIS
                // photo should be, and under the 2026-09-01 ruling the
                // direction is what makes that claim.
                "that is their colour HABIT, not a target for this photo; where the direction \
asks for other colour, follow the direction."
                    .to_string()
            } else if !bold {
                "match this LEVEL of colour shaping, do not exceed it.".to_string()
            } else if shaped >= COLOUR_HABIT_FLOOR {
                "treat this LEVEL of colour shaping as your FLOOR — you may go beyond it."
                    .to_string()
            } else {
                // B4: the same dial, the same measurement, and NO false floor.
                // These neighbours barely touched colour, so their habit cannot
                // be the floor the committed band promises — and rewriting the
                // numbers to make one is the fabrication this block exists to
                // refuse. The floor comes from the dial instead, stated as an
                // allowance the model may exceed.
                let (hsl_pm, grade_pm) = style_colour_floor(strength.get());
                // TIGHT ON PURPOSE. This arm renders the WIDEST block this
                // app can build, and the first draft of it put an adversarial
                // maximal block 12 B over `advisor::REFERENCE_BUDGET_BYTES`
                // (measured by `the_local_work_note_fits_the_proposers_budget`,
                // which is exactly what that test is for). The prose that
                // overflowed the door is this batch's own, so this batch's prose
                // is what pays — not S2's `REFERENCE_DESC_CHARS`, which would
                // have shortened the description every real library shows.
                format!(
                    "that is their HABIT, too near zero to BE a floor — do not read it as one. \
Your floor comes from the STYLE dial instead: at least ±{hsl_pm:.0} on whichever `hsl` \
saturation or luminance band this photo calls for, and at least {grade_pm:.0} of \
`color_grade` wheel saturation; you may go beyond that."
                )
            };
            format!(
                "  THEIR TYPICAL COLOUR SHAPING ({} of {} similar shots): HSL mixer mean |hue| \
{:.0}, |sat| {:.0}, |lum| {:.0} across the 8 bands; colour-grade strongest wheel saturation \
{:.0}, mean |wheel lum| {:.0}; per-channel RGB curves on {:.1} of 3 channels — {aim}",
                fams.len(),
                ex.len(),
                hue,
                sat,
                lum,
                wheel_sat,
                wheel_lum,
                mean(|f| f.rgb_curves as f32),
            )
        };
        // The LOOK the retrieved shots share, as a habit in its own right
        // (task book section 1c). The per-exemplar `look:` clause inside `lines` is a
        // list of four descriptions; this is what they have in COMMON, and at
        // Style >= 0.85 it is stated as part of the TARGET rather than as
        // background. Without it the bold header named settings, curve and
        // colour families and left the vocabulary tags as decoration on lines
        // the model was told not to copy.
        let look_note = {
            // Most shared first, ties by phrase — `shared_look_tags`, the very
            // ranking `look_summary` hands the visual judge (B2).
            let ranked = shared_look_tags(ex);
            if ranked.is_empty() {
                String::new()
            } else {
                let shared: Vec<String> = ranked
                    .iter()
                    .map(|(tag, n)| format!("{tag} ({n}/{})", ex.len()))
                    .collect();
                let aim = match voice {
                    StyleVoice::Target => {
                        "— REPRODUCE this look; it is the target, and you may push past it."
                    }
                    StyleVoice::Ceiling => {
                        "— the look their edits tend toward; stay within it, do not exceed it."
                    }
                    StyleVoice::Background => {
                        "— the look their edits tend toward; the direction may take this photo elsewhere."
                    }
                };
                format!("  THEIR SHARED LOOK across these shots: {} {aim}", shared.join(", "))
            }
        };
        // …and how they work LOCALLY, as the same kind of averaged habit (S3).
        // Over the neighbours that were MEASURED only: a pre-S3 exemplar
        // carries no habit, and counting it as "no masks" would invent a
        // restraint — exactly the distinction `MaskHabit`'s three states exist
        // to keep. When none was measured this is the empty string and the
        // block is byte-identical to the one S2 shipped.
        let local_work_note = crate::mask_habit::local_work_note(
            &ex.iter().map(|e| e.masks).collect::<Vec<_>>(),
            voice,
        );
        match voice {
            StyleVoice::Target => Some(format!(
                "STYLE REFERENCE — TARGET style to reproduce (the retrieved shots define the settings, curve habit, colour \
                 families and LOOK to reproduce; the scene differs): {}{}{}{}{}",
                lines.join("  |  "), curve_note, family_note, look_note, local_work_note
            )),
            StyleVoice::Ceiling => Some(format!(
                "STYLE REFERENCE — how this user edited SIMILAR past shots (for consistency with their \
                 taste; reference, do NOT copy verbatim, the scene differs): {}{}{}{}{}",
                lines.join("  |  "), curve_note, family_note, look_note, local_work_note
            )),
            // The header says BACKGROUND in its first word, because a model
            // that reads only the first clause of a 4 KB block must still
            // come away with the right one of the two jobs.
            StyleVoice::Background => Some(format!(
                "STYLE BACKGROUND — how this user edited SIMILAR past shots, for continuity only; the scene \
                 differs. The DIRECTION LEADS: where it and these habits conflict, follow the \
                 direction: {}{}{}{}{}",
                lines.join("  |  "), curve_note, family_note, look_note, local_work_note
            )),
        }
    }

    /// Style-axis spelling used by the advisor pipeline. Kept separate from
    /// the historical GradeStrength entry point so existing gate fixtures do
    /// not change their API while Style >= 0.85 gets target wording.
    ///
    /// Since v1.2.3 it also takes what the develop knows about the DIRECTION,
    /// because the voice is a function of all three inputs — see
    /// [`StyleVoice::choose`], which is the only place that decision is made.
    pub fn render_reference_for_style(
        &self,
        ex: &[&StyleExemplar],
        style: f32,
        direction: Option<&str>,
        adherence: crate::recipe::DirectionAdherence,
    ) -> Option<String> {
        self.render_reference_voiced(
            ex,
            crate::recipe::GradeStrength::new(style),
            StyleVoice::choose(style, direction, adherence),
        )
    }
}
