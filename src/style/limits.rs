//! Dimensions, weights, the index versions, the size limits, and the bounded-text helpers.

use super::*;

pub(crate) const NDIM: usize = 14;
/// Unbounded (log / ratio) dims to z-score; the rest are already ~bounded.
pub(super) const ZSCORE_DIMS: [usize; 4] = [0, 1, 2, 10];
/// Per-dim distance weights (scene-type discriminators heavier).
pub(super) const WEIGHTS: [f32; NDIM] = [
    1.5, 1.0, 1.0, 0.5, 0.5, 1.5, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.5,
];
/// Slider keys shown as the reference (crs key → label). Tint/Saturation/Dehaze
/// were added in index v2 so the style blend captures the user's colour habits.
/// Bump when the FEATURE semantics change (v3: display-frame Meta dims, i.e.
/// the EXIF orientation reaching the aspect feature at last; v4: the COMPOSED
/// orientation — v3 plus the photographer's own `quarter_turns`, R27, so a
/// hand-rotated shot is retrieved as the portrait/landscape it now IS;
/// **v5: the optional SigLIP 2 embedding block** — a v5 exemplar MAY carry
/// [`StyleExemplar::embed`] and the distance MAY gain a cosine term, which
/// changes the RANKING and therefore the version; **v6: the attribute TAGS are
/// derived against the library mean** ([`tags_from_scores`]), which changes
/// what a stored tag list means and, through [`desc_text`], the text a
/// tag-only record's description vector is the vector OF — i.e. the ranking).
pub(super) const CURRENT_INDEX_VERSION: u32 = 6;
/// Index versions this build will serve. v4 and v5 are accepted alongside v6
/// on purpose. The v5 change was PURELY ADDITIVE (an absent `embed`
/// contributes nothing to the distance, [`embed_distance`]), and the v6 change
/// is RE-DERIVABLE: [`StyleIndex::load`] re-scores every stored tag list from
/// the file's own `vocab_scores` and drops a description vector whose text the
/// new tags no longer spell, so a v5 file is served as a v6 one without the
/// hour-long rebuild it would otherwise cost. What is refused is a version
/// whose 14 FEATURES mean something else — which is why v3 is still refused
/// (its aspect dim means something else).
pub(super) const READABLE_INDEX_VERSIONS: [u32; 3] = [4, 5, CURRENT_INDEX_VERSION];
// Load-time bounds: an index is disk input that reaches the model prompt, so
// its size, shape and values get invariants at the door (there is no
// exfiltration channel — the response is strict json_schema — but an
// unbounded index means an unbounded paid request and a steerable grade).
//
// RE-DERIVED TOGETHER in R27 Batch-5, because they had stopped agreeing, and
// again here (F-5) because the look library had joined the file without
// joining the arithmetic. The whole derivation, measured rather than argued:
//
//   * one embedding, as serialised: 768 elements at `str(np.float32(x))`
//     precision measured **12.41 bytes each** over three real SigLIP 2
//     vectors (9,529 bytes of array text). The conservative bound is a
//     15-character shortest-decimal plus its comma = 16 B, so
//     768 x 16 = 12,288 B = 12 KiB.
//   * the LARGEST record either side can produce is a maximal RAW exemplar:
//     TWO such vectors (image + description), the 14-dim feature, 33
//     vocabulary scores, four bounded tags, a `MAX_DESC_CHARS` description, a
//     full settings map, curve, family summary and its stem/path envelope.
//     `capacity_constants_hold_two_vectors_and_the_scores` serialises exactly
//     that record and measures it: **20,940 B** (a maximal LOOK record, which
//     has no feature/settings/curve, measures 20,331 B). 40 KiB is therefore a
//     ~2x bound, which is the room JSON escaping of a hostile stem/description
//     would need.
//   * the file must hold BOTH populations at once, because `save` merges them
//     into one document: 5,000 RAW exemplars + `MAX_LOOK_EXEMPLARS` look
//     records. 5,500 x 40 KiB = 225,280,000 B = 214.84 MiB, and the 228 MiB
//     file cap leaves 13.16 MiB for the top-level envelope (two 14-element
//     normalisation vectors, source/looks dir, provenance).
//
// Before F-5 the `const _` gate counted the RAW population only while `load`
// admitted 5,000 looks BESIDE it — 10,000 x 40 KiB = 390 MiB against a 228 MiB
// cap. The gate was true and meaningless; the look cap is what makes it mean
// something, and it is enforced at the door like the RAW one.
pub(super) const MAX_STYLE_INDEX_BYTES: usize = 228 * 1024 * 1024;
pub(super) const MAX_STYLE_EXEMPLARS: usize = 5_000;
/// The look population's own cap. A look library is a CURATED set of finished
/// photos (the reference grades a photographer would point at), not a whole
/// archive — the 5,000-entry product limit belongs to the RAW library it sits
/// beside, and giving looks the same number is what broke the byte envelope.
pub(super) const MAX_LOOK_EXEMPLARS: usize = 500;
/// The per-record bound both caps above are derived from.
pub(super) const MAX_EXEMPLAR_BYTES: usize = 40 * 1024;
/// Longest description a record may carry, at the door and in the prompt.
pub const MAX_DESC_CHARS: usize = 512;

/// How much of a description reaches a PROMPT BLOCK, as opposed to the index.
///
/// Two different budgets were spending one number. The index stores up to
/// [`MAX_DESC_CHARS`] (512) and the diagnostic and the text tower use the whole
/// sentence — that is right, the sentence is the vector's input. A BLOCK is
/// bounded by `advisor::REFERENCE_BUDGET_BYTES` (4,096) instead, and four
/// neighbours at 512 characters is 2,048 characters of description alone: a
/// maximal block measured 5,920 B before S3 and lost its TAIL to
/// `BoundedUntrustedText` — since S3 that tail is the local-work note, the
/// sentence most useful exactly when the neighbours carry the most. Capping the
/// description INSIDE the block (user ruling 2026-08-30) keeps every note in the
/// budget without changing what the index stores, what it embeds, or how it
/// ranks.
pub const REFERENCE_DESC_CHARS: usize = 200;

/// How many characters of an exemplar's TAG STRING a block carries.
///
/// The other half of the same defect as [`REFERENCE_DESC_CHARS`]: the index
/// door admits up to 128 characters per tag, four of them, so a hand-edited
/// index could spend 500+ characters of the block's 4,096-byte budget on ONE
/// neighbour's tags. Real tags are `LOOK_VOCAB` phrases with their prefixes
/// stripped — the longest is 41 characters before stripping — so four of them
/// join to roughly 120 characters and this never bites a real library; it
/// bounds the adversarial one.
pub const REFERENCE_TAGS_CHARS: usize = 128;

/// How many characters of ONE tag phrase a block carries.
///
/// The index door admits 128 per tag; the vocabulary's longest phrase is 41
/// characters BEFORE `LOOK_VOCAB`'s prefixes are stripped, so this never cuts a
/// real tag. It exists because the block has THREE tag consumers — the
/// per-exemplar look note, the shared-tag note and the look-reference block —
/// and bounding only the joins would leave the third one spending the budget a
/// phrase at a time.
pub const REFERENCE_TAG_PHRASE_CHARS: usize = 48;

/// Cut a string to `cap` CHARACTERS, ellipsis included in the bound.
///
/// The three doors below each bound a different thing for a different reason
/// (a tag phrase, the joined tag list, a description) and each wrote this tail
/// out by hand. The cut itself is one rule and belongs in one place: the bound
/// is in characters, never bytes — a byte slice would split a codepoint — and
/// the ellipsis sits INSIDE the budget so a reader of the block can tell a cut
/// sentence from one that merely stops.
fn bounded_chars(s: String, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s;
    }
    let mut out: String = s.chars().take(cap - 1).collect();
    out.push('\u{2026}');
    out
}

/// One tag phrase as a block carries it.
fn block_tag_phrase(tag: &str) -> String {
    bounded_chars(tag.to_string(), REFERENCE_TAG_PHRASE_CHARS)
}

/// The tags as a block carries them: at most [`LOOK_TAGS_K`], each phrase
/// bounded, joined, and the join itself bounded to [`REFERENCE_TAGS_CHARS`].
pub(super) fn block_tags(tags: &[String]) -> String {
    let joined =
        tags.iter().take(LOOK_TAGS_K).map(|t| block_tag_phrase(t)).collect::<Vec<_>>().join(", ");
    bounded_chars(joined, REFERENCE_TAGS_CHARS)
}

/// The tag phrases the retrieved exemplars SHARE, most-shared first and ties
/// broken by phrase so the order is stable, at most [`LOOK_TAGS_K`] of them,
/// each already through the block's own phrase door.
///
/// ONE ranking, TWO consumers: the reference block's `THEIR SHARED LOOK`
/// clause and [`StyleIndex::look_summary`], which is what the visual judge is
/// told the photographer asked for (B2). A second hand-written ranking is how
/// the block and the rubric would come to describe different looks.
pub(super) fn shared_look_tags(ex: &[&StyleExemplar]) -> Vec<(String, usize)> {
    let mut freq: BTreeMap<&str, usize> = BTreeMap::new();
    for tag in ex.iter().flat_map(|e| e.tags.iter()) {
        *freq.entry(tag.as_str()).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = freq.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked.into_iter().take(LOOK_TAGS_K).map(|(t, n)| (block_tag_phrase(t), n)).collect()
}

/// One exemplar's description as a block carries it: through the sanitising
/// door, then bounded to [`REFERENCE_DESC_CHARS`] CHARACTERS (never bytes — a
/// byte slice would split a codepoint).
///
/// THE one door for both blocks. Two call sites applied `sanitize_desc` and
/// nothing else; a cap added at one of them would have left the other spending
/// the old budget, which is the shape of defect this codebase fixes at the seam
/// rather than at the sites.
pub(super) fn block_desc(desc: Option<&str>) -> Option<String> {
    let d = crate::describe::sanitize_desc(desc?)?;
    Some(bounded_chars(d, REFERENCE_DESC_CHARS))
}

// R18 CANNOT RECUR, because this is a BUILD gate and not a test: moving either
// cap without the other stops compilation with the sentence below. A runtime
// test would have been the weaker choice — clippy's own `assertions_on_constants`
// points at exactly this, and a constant-vs-constant invariant belongs where
// the constants are.
//
// MUTATION: set `MAX_STYLE_EXEMPLARS` back to 50_000, raise
// `MAX_LOOK_EXEMPLARS` to the RAW cap, or set `MAX_STYLE_INDEX_BYTES` back to
// 32 MiB, and `cargo check` fails here.
const _: () = assert!(
    (MAX_STYLE_EXEMPLARS + MAX_LOOK_EXEMPLARS) * MAX_EXEMPLAR_BYTES <= MAX_STYLE_INDEX_BYTES,
    "both populations x the per-record bound exceed the index file cap — the constants \
     moved apart again (R18; the look cap joined them in S1-fix F-5)"
);
// …and the per-record bound must actually HOLD a maximal record, or the line
// above would be true and meaningless. 768 elements at the 16-byte worst case
// of a shortest-round-trip f32 decimal plus its comma is 12 KiB; the runtime
// half of this check is `capacity_constants_hold_two_vectors_and_the_scores`,
// which serialises the record and measures it.
const _: () = assert!(
    MAX_EXEMPLAR_BYTES >= crate::embed::EMBED_DIM * 16 * 2
        + LOOK_VOCAB.len() * 16
        + MAX_DESC_CHARS * 6
        + 4096,
    "the per-record bound cannot hold two embeddings, scores, and description"
);
