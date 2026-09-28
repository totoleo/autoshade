//! Standardisation: finiteness, hubness, the standardised and distance terms, and retagging.

use super::*;

/// Every number an exemplar carries must be FINITE: serde_json writes a NaN
/// as `null`, and the next LOAD of the index then fails wholesale — one bad
/// EXIF field or curve point published an UNLOADABLE index. Features are
/// additionally BOUNDED ([`MAX_FEATURE_ABS`], L04-3): finite-but-huge
/// values overflow the z-score arithmetic downstream, and no legitimately
/// built index can produce one (every dim is a ln or a bounded ratio) —
/// only a tampered or bit-rotted file on disk, this file's own stated
/// threat model ("invariants at the door").
pub(super) fn exemplar_is_finite(e: &StyleExemplar) -> bool {
    e.feat.iter().all(|v| v.is_finite() && v.abs() <= MAX_FEATURE_ABS)
        && e.settings.values().all(|v| v.is_finite())
        && e.curve.is_none_or(|c| c.iter().all(|v| v.is_finite()))
        && e.families.is_none_or(|f| f.is_finite())
        // The embedding gets a TIGHTER band than the 14 dims, for free: it is
        // L2-normalised by construction, so every element is in [-1, 1] and
        // the whole vector's norm is 1. Checking the elements alone would
        // still admit a 768-dim vector of 1.0s (norm 27.7), which the cosine
        // would then treat as overwhelmingly similar to everything. The width
        // is pinned too — a foreign width is silently ignored by
        // `embed_distance`, and an index that carries one should say so at the
        // door instead of retrieving as if the block were absent.
        && e.embed.as_ref().is_none_or(|v| {
            v.len() == crate::embed::EMBED_DIM
                && v.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3)
                && (v.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt() - 1.0).abs() < 1e-3
        })
        && e.vocab_scores.as_ref().is_none_or(|v| v.len() == LOOK_VOCAB.len() && v.iter().all(|x| x.is_finite()))
        && e.desc.as_ref().is_none_or(|d| d.chars().count() <= MAX_DESC_CHARS)
        && e.desc_embed.as_ref().is_none_or(|v| {
            v.len() == crate::embed::EMBED_DIM
                && v.iter().all(|x| x.is_finite() && x.abs() <= 1.0 + 1e-3)
                && (v.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt() - 1.0).abs() < 1e-3
        })
        // A NaN mean serialises as `null` and makes the whole index unloadable
        // — the same reason the family summary is checked here (S3).
        && e.masks.is_none_or(|m| m.is_finite())
}

/// How many comparable candidates a query needs before its text terms are
/// standardised. Below this the mean and σ of the candidate set are not an
/// estimate of anything, so the raw gap is used and the fact is disclosed.
const MIN_STANDARDISATION_CANDIDATES: usize = 3;

/// One cosine term for every candidate, standardised over the candidate set.
///
/// **Why the text terms need this and the image term does not.** SigLIP's
/// image↔image cosines spread across the library the way a distance should;
/// its image↔TEXT cosines do not. Measured on the shipped index, a direction's
/// text vector scores 0.02–0.03 against every neighbour, so `w·(1−cos)` sat at
/// 7.72–7.85 for all four of them: a spread of 0.13 against a 14-dim spread of
/// 2.5. The term was numerically enormous and informationally inert, and a
/// calibration over it would "find" W_TXT ≈ 0 for the wrong reason — not
/// because text says nothing, but because its raw scale hides what it says.
///
/// The z-score is an AFFINE map of the raw gap within one query, so it adds no
/// ordering power the raw term did not have; what it does is put the term on
/// the 14-dim block's scale, which is what makes the weight a measurable
/// quantity rather than a unit conversion. It is per QUERY, never global: the
/// absolute level of a text cosine is a property of the phrase, not of the
/// photograph, and only the ordering across candidates is evidence.
///
/// A candidate with no vector scores 0 — the candidate-set MEAN on this scale,
/// i.e. "no evidence", the same reading the raw term's 0 had.
pub(super) struct StandardisedTerm {
    pub(super) terms: Vec<f64>,
    pub(super) standardised: bool,
    pub(super) hub_corrected: bool,
}

/// How much a candidate resembles ANY sentence about a grade: its mean cosine
/// against the whole of [`LOOK_VOCAB`], which every embedded record already
/// stores as `vocab_scores`.
///
/// This is the per-CANDIDATE main effect of a text cosine, and it is the one
/// thing a per-query z-score cannot remove. [`standardise`] centres a
/// direction's gaps over the candidate set, which takes out the level the
/// PHRASE sits at; what survives is `cos(t, e)`'s dependence on `e` alone —
/// some photographs simply score high against every sentence. Measured on the
/// user's 169-exemplar index against twelve direction texts, that candidate
/// main effect is 21.7 % of the cosine's total variance, against 25.3 % for
/// the direction x candidate interaction that is the only part able to tell
/// two directions apart. Six ANTONYM pairs ranked the corpus with a mean
/// Spearman of +0.27 BETWEEN the two members of a pair — opposite wishes
/// agreeing about which photographs to show — and −0.17 once this quantity is
/// removed first.
///
/// `None` when the record carries no profile, or one of another width: the
/// mean of a vocabulary this build cannot name is not this quantity.
pub(super) fn text_hubness(vocab_scores: Option<&[f32]>) -> Option<f64> {
    let v = vocab_scores?;
    if v.len() != LOOK_VOCAB.len() {
        return None;
    }
    Some(v.iter().map(|&s| s as f64).sum::<f64>() / v.len() as f64)
}

/// The candidate set's hubness, or `None` when ANY candidate is missing it.
///
/// All-or-nothing on purpose. The correction is applied to a gap BEFORE the
/// set is standardised, so correcting some candidates and not others would
/// rank them on two different scales and quietly favour the uncorrected ones —
/// the same asymmetry [`embed_distance`] refuses to invent for a missing
/// vector. An index built before the vocabulary existed therefore keeps its
/// previous ranking exactly, and the report says which happened
/// ([`DistanceTerms::txt_hub_corrected`]).
pub(super) fn hubness_profile<'a>(scores: impl Iterator<Item = Option<&'a [f32]>>) -> Option<Vec<f64>> {
    scores.map(text_hubness).collect()
}

/// The weighted RAW gaps — `w · (1 − cos)`, and an exact zero where the pair
/// was not comparable or the weight is off.
fn raw_term(raw: &[Option<f64>], w: f64) -> StandardisedTerm {
    // A zero weight is the term's ABSENCE, bit for bit — never `0.0 * z`,
    // which would be a signed zero riding on the sum.
    if w == 0.0 {
        return StandardisedTerm { terms: vec![0.0; raw.len()], standardised: false, hub_corrected: false };
    }
    StandardisedTerm { terms: raw.iter().map(|r| r.map_or(0.0, |v| w * v)).collect(), standardised: false, hub_corrected: false }
}

/// `raw[i] = Some(1 − cos)` when the pair is comparable, `None` when it is not.
///
/// **This is the only text term there is.** F-14 built the standardised
/// variant beside a raw one and a `STANDARDISE_TEXT_TERMS` switch chose
/// between them; the switch is gone because the measurement that would decide
/// it has already been made and cannot be re-opened by a boolean. S1 measured
/// both on a grid whose query text was the exemplar's TAG STRING and the raw
/// variant won. S2 gave every exemplar real prose and reversed it: best
/// standardised `(4, 4, 0.5)` = 0.664818 against best raw `(4, 2, 0)` =
/// 0.693811, and the raw variant's own text terms were indistinguishable from
/// having none (paired CI against `(4, 0, 0)` = [−0.001834, +0.004821]) while
/// the standardised variant's were not ([+0.001589, +0.055436]). The
/// head-to-head between the two BEST rows was itself not significant (paired
/// 95 % CI [−0.000205, +0.054341], including 0 barely), so the choice rested
/// then — and rests now — on which variant's text terms earn their keep.
///
/// The short-direction grid settles the head-to-head S2 could not: best raw
/// `(4, 2, 0)` = 0.410916 against best standardised `(0, 2, 0)` = 0.377820,
/// paired 95 % CI [+0.021870, +0.043719] — raw worse, 0 excluded. Those two
/// weight triples are each grid's own argmax and neither is a recommendation
/// ([`W_TXT_DEFAULT`] refuses both); what they establish is that the variant
/// chosen for its text terms earning their keep is the one a like-for-like
/// comparison picks as well, once the query text is the length a user types.
///
/// What settled it is that everything measured since is a property of THIS
/// variant and of nothing else. The hubness correction below is defined on the
/// z-score; `W_TXT_DEFAULT`'s 0.5, `W_DESC_DEFAULT`'s 0.5 and the antonym and
/// collapse numbers beside them are the corrected standardised grid's. A raw
/// arm kept behind a `false` would have been an untested second ranking
/// carrying weights nobody calibrated for it, which is what a switch nothing
/// flips actually costs. `scripts/calibrate_style_retrieval.py` still sweeps
/// both arms and prints both tables, so the comparison stays re-runnable
/// without a dead branch in the ranking.
///
/// Standardising is TWO centrings, and only the second one used to be here.
/// Subtracting the candidate set's MEAN takes out the level a phrase sits at —
/// a per-direction constant, which is why the z-score is per query. It leaves
/// the per-CANDIDATE constant untouched: the photographs that score high
/// against every sentence keep scoring high, on a term the z-score has just
/// rescaled to unit spread — whatever `W_TXT` then multiplies it by. `hub`
/// is the candidate set's [`hubness_profile`], and removing it first is what
/// makes the surviving order a statement about THIS direction.
///
/// One-sided, and measured that way: the correction is applied to the term
/// scored against the exemplars' IMAGE vectors, where `vocab_scores` is the
/// matching quantity (that image against grade prose). The DESCRIPTION term is
/// text against text and has no stored bank of its own; the best available
/// stand-in — each description's cosine with the mean description of the
/// corpus — made the six antonym pairs agree MORE, not less (mean Spearman
/// +0.339 -> +0.377), so that term is left alone and `hub` is `None` for it.
pub(super) fn standardise(raw: &[Option<f64>], hub: Option<&[f64]>, w: f64) -> StandardisedTerm {
    if w == 0.0 {
        return raw_term(raw, w);
    }
    let plain = || raw_term(raw, w);
    // A profile of the wrong length is not this candidate set's profile.
    let hub = hub.filter(|h| h.len() == raw.len());
    // `gap = 1 − cos`, so REMOVING a candidate's hubness from its cosine is
    // ADDING it to its gap. A candidate with no vector stays `None`: a
    // correction is not evidence either.
    let corrected: Vec<Option<f64>> = match hub {
        Some(h) => raw.iter().zip(h).map(|(g, b)| g.map(|g| g + b)).collect(),
        None => raw.to_vec(),
    };
    let live: Vec<f64> = corrected.iter().flatten().copied().collect();
    if live.len() < MIN_STANDARDISATION_CANDIDATES {
        return plain();
    }
    let n = live.len() as f64;
    let mean = live.iter().sum::<f64>() / n;
    let sd = (live.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n).sqrt();
    // NaN is caught by the finiteness test, so the second comparison is a
    // plain one (clippy::neg_cmp_op_on_partial_ord: `!(sd > 0.0)` also
    // swallowed NaN, silently and by accident).
    if !sd.is_finite() || sd <= 0.0 {
        return plain();
    }
    StandardisedTerm {
        terms: corrected.iter().map(|r| r.map_or(0.0, |v| w * (v - mean) / sd)).collect(),
        standardised: true,
        hub_corrected: hub.is_some(),
    }
}

/// The additive components of one candidate's distance, as the retrieval
/// computed them — so a diagnostic can print the production numbers instead of
/// maintaining a second ranking path.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistanceTerms {
    /// The 14-dim hand-feature block (0 for a look record, which has none).
    pub d14: f64,
    /// `W_EMB·(1−cos(q_img, e_img))` — raw, never standardised (its cosines are
    /// already spread). For a look record this is the `W_LOOK` term.
    pub emb: f64,
    /// The standardised direction-text ↔ exemplar-image term.
    pub txt: f64,
    /// The standardised direction-text ↔ exemplar-description term.
    pub desc: f64,
    /// The raw `1−cos` behind [`Self::txt`], when the pair was comparable.
    pub txt_gap: Option<f64>,
    /// The raw `1−cos` behind [`Self::desc`], when the pair was comparable.
    pub desc_gap: Option<f64>,
    /// False when the candidate set was too small (or degenerate) to
    /// standardise and the raw gap was used — disclosed, never silent.
    pub txt_standardised: bool,
    pub desc_standardised: bool,
    /// This candidate's TEXT HUBNESS ([`text_hubness`]), removed from
    /// [`Self::txt_gap`] before the set was standardised. `None` when the
    /// correction was not in force, or when this pair had no cosine at all.
    pub txt_hub: Option<f64>,
    /// Did the txt term's standardisation remove the candidates' hubness?
    ///
    /// False on an index whose records carry no `vocab_scores` — where the
    /// ranking is bit for bit what it was before this correction existed, and
    /// a reader of the terms is told so rather than left to assume.
    pub txt_hub_corrected: bool,
}

impl DistanceTerms {
    pub fn total(&self) -> f64 {
        self.d14 + self.emb + self.txt + self.desc
    }
}

/// The four attribute phrases one photograph is tagged with — the strongest
/// phrase inside each [`LOOK_GROUPS`] group, then the strongest four of those.
///
/// `mean` is the LIBRARY's per-phrase mean ([`vocab_mean`]), subtracted first,
/// and this is the whole of the fix: a SigLIP image↔text cosine carries a
/// large per-PHRASE constant that has nothing to do with the photograph. Some
/// captions simply score high against everything, so the raw argmax picks the
/// same phrases for the whole library — measured on the user's 169-exemplar
/// index, "a harsh midday-light photo" was tagged on 88 of 169 photographs
/// (52 %), 3 of the 33 phrases were never chosen at all, and the GOLDEN-HOUR
/// exemplar came back tagged `harsh midday-light, film-like grain, cinematic
/// tones, cross-processed`. Centring is the same move [`standardise`] makes on
/// the ranking's text terms and for the same reason; it is per-phrase over the
/// library rather than per-query because a tag is a property of the
/// photograph, not of a query. After it the commonest phrase is on 21 %, all
/// 33 are used, and that exemplar reads `warm golden tones, sepia toning,
/// film-like grain, gentle natural light`. On the 94-photo look library:
/// commonest phrase 43 % → 27 %, phrases ever used 25 of 33 → 33.
///
/// `None` reproduces the v5 derivation exactly, and is what a population with
/// no score profile at all gets — there is no mean to subtract, which is a
/// different fact from a mean of zero.
pub(super) fn tags_from_scores(scores: &[f32], mean: Option<&[f32]>) -> Vec<String> {
    if scores.len() != LOOK_VOCAB.len() { return Vec::new(); }
    let mean = mean.filter(|m| m.len() == scores.len());
    let at = |i: usize| scores[i] - mean.map_or(0.0, |m| m[i]);
    let mut chosen = Vec::new();
    for group in LOOK_GROUPS {
        if let Some(&idx) = group.iter().max_by(|&&a, &&b| at(a).total_cmp(&at(b))) {
            chosen.push((at(idx), LOOK_VOCAB[idx]));
        }
    }
    chosen.sort_by(|a,b| b.0.total_cmp(&a.0));
    chosen.into_iter().take(LOOK_TAGS_K).map(|(_, s)| s.strip_prefix("a photo with ").or_else(|| s.strip_prefix("an ")).unwrap_or(s).to_string()).collect()
}

/// A population's per-phrase mean `vocab_scores`, or `None` when no record in
/// it carries a profile this build can name.
///
/// The mean is over the records that HAVE one, never over the population size:
/// a library where half the photographs failed their sidecar call would
/// otherwise have every phrase halved, which is a different centre.
fn vocab_mean<'a>(rows: impl Iterator<Item = Option<&'a [f32]>>) -> Option<Vec<f32>> {
    let mut sum = vec![0.0f64; LOOK_VOCAB.len()];
    let mut n = 0u32;
    for row in rows.flatten().filter(|r| r.len() == LOOK_VOCAB.len()) {
        for (s, v) in sum.iter_mut().zip(row) {
            *s += *v as f64;
        }
        n += 1;
    }
    (n > 0).then(|| sum.iter().map(|s| (s / n as f64) as f32).collect())
}

/// Re-derive the tag list of every record in ONE population from that
/// population's own scores.
///
/// A population-level pass rather than a per-record one, because since v6 a
/// tag list is not a function of one score vector: [`tags_from_scores`]
/// subtracts the library mean, so no record's tags are knowable until every
/// record's scores are. That is why a build calls this once, after the
/// embedding and description stages and before the text stage, instead of
/// tagging each exemplar where its scores arrive.
///
/// It also owns the consequence. A stored `desc_embed` is the vector OF a
/// text, and [`desc_text`] reads the tags for a record with no description; a
/// record whose tags this pass changed therefore no longer holds the vector it
/// claims to. Dropping it is what makes the pass safe to run at LOAD as well
/// as at build — an index written by v5 is re-scored in place, and the text
/// stage (or the next build) fills the vector back in.
///
/// Measured on the user's library, which is the shape this costs something
/// on: all 169 RAW exemplars carry description PROSE, so [`desc_text`] never
/// reads their tags and not one vector is dropped there. The 94-photograph
/// look library carries none — the look build describes nothing — so its
/// records fall back to the tag string, 92 of the 94 come out with a
/// different tag list under the library-mean rule, and those 92 hold no
/// description vector until the next build. That is the upgrade cost of the
/// rule, in records, and it is paid once.
pub(super) fn retag<R: DescribedRecord>(records: &mut [R]) {
    let Some(mean) = vocab_mean(records.iter().map(|r| r.vocab_scores())) else {
        return;
    };
    for record in records.iter_mut() {
        let Some(tags) = record.vocab_scores().map(|s| tags_from_scores(s, Some(&mean))) else {
            continue;
        };
        let before = desc_text(record.desc(), record.tags());
        record.set_tags(tags);
        if record.has_desc_embed() && before != desc_text(record.desc(), record.tags()) {
            record.set_desc_embed(None);
        }
    }
}
