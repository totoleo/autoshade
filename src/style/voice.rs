//! The style query, the look vocabulary, the colour-habit floor, and the style voice.

use super::*;

/// ONE retrieval query: the vectors it was able to produce and what those
/// vectors are worth.
///
/// They travel together because they are only meaningful together — a text
/// vector with `txt = 0` contributes nothing, and a weight without a vector
/// weights nothing. Passing them as one value is also what keeps
/// [`StyleIndex::retrieve_with_embed`] and [`StyleIndex::distance_components`]
/// inside clippy's argument budget without an `allow`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StyleQuery<'a> {
    /// The query photo's SigLIP image vector, when the sidecar produced one.
    pub image: Option<&'a [f32]>,
    /// The direction text's SigLIP text vector, when there was a direction.
    pub text: Option<&'a [f32]>,
    pub weights: RetrievalWeights,
}

impl<'a> StyleQuery<'a> {
    /// A query with NO vectors — the 14-dim ranking. The weights still ride
    /// along because `retrieve_looks` reads `look` even here (it decides
    /// nothing when there is no vector, but the value is the same one).
    pub const FEATURES_ONLY: Self =
        StyleQuery { image: None, text: None, weights: RetrievalWeights::SHIPPED };

    pub fn new(image: Option<&'a [f32]>, text: Option<&'a [f32]>, weights: RetrievalWeights) -> Self {
        StyleQuery { image, text, weights }
    }
}

/// SigLIP-style attribute captions. Changing a phrase changes all stored
/// scores, therefore the version is persisted in each index envelope.
pub const LOOK_VOCAB_VERSION: u32 = 1;
pub const LOOK_VOCAB: [&str; 33] = [
    // white-balance lean
    "a photo with warm golden tones", "a photo with cool blue tones", "a photo with neutral white balance",
    // tonality
    "a photo with deep blacks", "a photo with lifted matte shadows", "a photo with bright airy high-key tones", "a photo with dark moody low-key tones",
    // contrast
    "a photo with punchy high contrast", "a photo with soft low contrast",
    // saturation
    "a photo with vivid saturated colours", "a photo with muted desaturated colours", "a photo with pastel colours",
    // colour treatment
    "a photo with a teal-and-orange split tone", "a monochrome black-and-white photo", "a photo with sepia toning", "a photo with a cross-processed colour treatment",
    // finishing
    "a photo with a soft hazy glow", "a photo with crisp clarity", "a photo with film-like grain", "a photo with a clean digital finish",
    // light
    "a golden-hour photo", "a blue-hour photo", "an overcast flat-light photo", "a harsh midday-light photo", "a night photo",
    // extra stable descriptors to keep the vocabulary useful across scenes
    "a photo with gentle natural light", "a photo with dramatic directional light", "a photo with rich shadow detail", "a photo with restrained colour", "a photo with luminous highlights", "a photo with cinematic tones", "a photo with a soft editorial grade", "a photo with a neutral documentary grade",
];
pub const LOOK_TAGS_K: usize = 4;

/// The smallest colour-shaping magnitude that can honestly be called a FLOOR
/// (B4, user ruling 2026-08-30 — "色彩下限等于没有下限").
///
/// The reference block used to promise `treat this LEVEL of colour shaping as
/// your FLOOR` over whatever the retrieved neighbours measured, and a real
/// library measured `HSL mixer mean |hue| 2, |sat| 2, |lum| 0 … strongest wheel
/// saturation 0`: a floor of zero, i.e. no floor, printed in capitals. Five is
/// the app's own smallest visible colour move — the proposer prompt already
/// tells the model that `small saturations (~5..25) read as a tasteful
/// split-tone` (`advisor::openai::propose_instruction`) — so below it the
/// neighbours did not shape colour and the sentence must say so instead of
/// dressing a zero up as a target.
pub const COLOUR_HABIT_FLOOR: f32 = 5.0;

/// The Style value at which the retrieved habits stop being a ceiling and
/// become the target.
///
/// NOT a new number: this is the `strength.get() >= 0.85` literal that has
/// sat inside `StyleIndex::render_reference` since GATE 5. Naming it is what
/// lets [`StyleVoice::for_style`] be the ONE place that reads it, now that a
/// second input (the direction) can also decide the voice.
pub const STYLE_TARGET_MIN: f32 = 0.85;

/// WHOSE aim the style-reference block states.
///
/// Until v1.2.3 this was a single `bold` boolean read off the Style axis, so
/// the block had exactly two things it could say: a CEILING ("stay within
/// it, do not exceed it") below [`STYLE_TARGET_MIN`], and a TARGET
/// ("REPRODUCE this look") at or above it. Either way the retrieved library
/// was the thing to hit, and a free-text direction could only move the
/// proposal WITHIN it.
///
/// MEASURED, 2026-09-01, on the island showcase frame at `--style 1.0
/// --strength 0.9` against this photographer's own index (169 RAW+XMP
/// exemplars + 94 finished looks): three directions as far apart as "dark
/// moody low-key … teal-and-orange", "warm golden tones, film-like grain,
/// lifted matte shadows" and "vivid saturated colours, punchy high contrast"
/// developed to per-panel-cell mean HSV S/V of 23/54 · 11/58 · 17/55 — all
/// three inside the library's own cool hazy register, against the neutral
/// develop's 17/47. The same three directions with the photographer's own
/// edits removed from the index separated to 34/38 · 12/61 · 29/65.
///
/// User ruling 2026-09-01: **when a direction is given, the photographer's
/// own edits are background and the direction leads.** [`Self::Background`]
/// is that third voice. It is chosen by the direction-adherence dial the app
/// already had — the dial whose entire job is "how strongly should this
/// direction be followed" — rather than by a fourth control the user would
/// have to discover.
///
/// The NUMBERS the block prints never depend on the voice. They are what the
/// photographer actually did, and rewriting a measurement to match a dial is
/// the fabrication this block exists to refuse.
///
/// `Default` is [`Self::Ceiling`] — the voice a develop with no direction and
/// the shipped Style default already renders — because [`crate::advisor::GradeIntent`]
/// derives `Default` and carries this voice to the reviewers. The default has to be
/// the one that says "nothing is leading": a forgotten field must not silently
/// tell a judge that the photographer's library is background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StyleVoice {
    /// Below [`STYLE_TARGET_MIN`], nothing leading: the habits are a CEILING.
    #[default]
    Ceiling,
    /// At or above [`STYLE_TARGET_MIN`], nothing leading: the habits are the
    /// TARGET to reproduce.
    Target,
    /// A non-empty direction at adherence tier `Direct` or `Brief`: the
    /// habits are BACKGROUND for continuity and the direction overrides
    /// them. The numeric distillation goes with the voice — see
    /// [`Self::distils`] and `pipeline::produce_recipe`.
    Background,
}

impl StyleVoice {
    /// The ONE derivation. Every surface that renders or forecasts the block
    /// (`pipeline::produce_recipe`, the CLI's `style-query`) calls this
    /// instead of re-deciding from `style >= 0.85` — two hand-written copies
    /// of that comparison is exactly how the block's wording and the
    /// pipeline's blend drifted apart before.
    ///
    /// A blank or absent direction cannot lead, and a `Hint` direction has
    /// asked not to; both keep the historical voices, byte for byte
    /// (`the_no_direction_block_is_byte_identical`).
    pub fn choose(
        style: f32,
        direction: Option<&str>,
        adherence: crate::recipe::DirectionAdherence,
    ) -> Self {
        let leads = direction.is_some_and(|d| !d.trim().is_empty())
            && matches!(
                adherence.tier(),
                crate::recipe::AdherenceTier::Direct | crate::recipe::AdherenceTier::Brief
            );
        if leads { Self::Background } else { Self::for_style(style) }
    }

    /// The historical two-way split, for the entry points that have no
    /// direction to offer (the gate fixtures, [`StyleIndex::render_reference`]).
    pub fn for_style(style: f32) -> Self {
        if style >= STYLE_TARGET_MIN { Self::Target } else { Self::Ceiling }
    }

    /// Whether this voice also pulls the finished recipe toward the library's
    /// measured means (`blend_toward`, `style_pull`).
    ///
    /// [`Self::Background`] does not, and that is half the ruling: leaving the
    /// wording as background while the arithmetic still lerped the proposal
    /// onto the library's mean would have moved the numbers back to exactly
    /// the place the words had just given up.
    pub fn distils(self) -> bool { !matches!(self, Self::Background) }
}

/// The colour floor the STYLE DIAL itself supplies when the neighbours' own
/// habit is too near zero to be one, as `(hsl band ±, colour-grade wheel
/// saturation)`.
///
/// The measurement is never rewritten — that is the rule the whole block is
/// built on — so the floor has to come from somewhere else, and the only
/// honest other source is the dial the photographer just turned up. Linear in
/// the dial and strictly increasing, so "more personal style" cannot buy a
/// smaller allowance; the numbers land inside the ~5..25 split-tone band the
/// proposer prompt already names, at its committed end.
pub(super) fn style_colour_floor(style: f32) -> (f32, f32) {
    let s = style.clamp(0.0, 1.0);
    ((10.0 + 20.0 * s).round(), (8.0 + 17.0 * s).round())
}
