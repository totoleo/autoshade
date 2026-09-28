//! Style similarity-retrieval reference (V2_PLAN §3).
//!
//! For a photo being edited, find the user's edits on the most SIMILAR past
//! photos (by EXIF + histogram features) and feed those to the advisor as SOFT
//! reference. This deliberately replaces the earlier global-bias "distillation":
//! different photo TYPES are edited differently, so we condition on similar
//! context instead of averaging everything. The retrieved edits are reference,
//! not a target to copy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::decode::{self, Histogram, Meta};
// The typed `crs:` readers (R28 Batch-5 5d): the trait carries the methods,
// `Scope` declares that a whole-sidecar read is a subtree read.
use crate::xmp::CrsSource;
use crate::pipeline;
use crate::recipe::EditRecipe;
mod build;
mod distil;
mod exemplar;
mod features;
mod index;
mod limits;
mod load;
mod standardise;
mod switches;
mod targets;
mod voice;

pub use build::{BuildProgress, BuildSidecars, BuildStage, embed_preview_with_text};
use build::{
    BuiltRecord, CACHE_BANDS, adopt_cached_desc_embed, apply_cached, attach_descriptions,
    cache_answers_everything, cache_entry, cache_is_publishable, embed_frames, is_self,
    note_no_embedding, read_exemplar, report, unpaired_note,
};
#[cfg(test)]
use build::{MAX_UNPAIRED_LISTED};
pub use distil::{MAX_DISTILLED_FIELDS_CHARS, distillation_preview, distilled_fields, style_pull};
use distil::{compute_norm, normalize};
pub use exemplar::{
    LookExemplar, StagedFrame, StyleExemplar, embed_staged_record, stage_embed_frame,
};
use exemplar::{
    DescribedRecord, VocabScratch, attach_desc_embeddings, cosine_gap, desc_text, embed_distance,
    frame_digests, library_key, sweep_intermediates_and_say, vocab_scratch_path,
};
#[cfg(test)]
use exemplar::{INTERMEDIATE_PREFIXES, STALE_INTERMEDIATE_AGE, sweep_stale_intermediates};
pub use features::{LEGACY_INDEX_PATH, RETRIEVE_K, feature_vector};
pub(crate) use features::{MAX_FEATURE_ABS};
use features::{
    COLOR_GRADE_LABEL, EMBED_FRAME_EDGE, LOOK_GROUPS, MAX_DISCLOSED_NEIGHBOURS, MAX_STEM_CHARS,
    REF_KEYS, derive_tag, read_monochrome, read_settings, setting_bands, walkdir,
};
#[cfg(test)]
use features::{distil_keys, parse_hour};
pub use index::{StyleIndex};
pub use limits::{
    MAX_DESC_CHARS, REFERENCE_DESC_CHARS, REFERENCE_TAGS_CHARS, REFERENCE_TAG_PHRASE_CHARS,
};
pub(crate) use limits::{NDIM};
use limits::{
    CURRENT_INDEX_VERSION, MAX_LOOK_EXEMPLARS, MAX_STYLE_EXEMPLARS, MAX_STYLE_INDEX_BYTES,
    READABLE_INDEX_VERSIONS, WEIGHTS, ZSCORE_DIMS, block_desc, block_tags, shared_look_tags,
};
#[cfg(test)]
use limits::{MAX_EXEMPLAR_BYTES};
pub use load::{
    EffectiveIndex, StyleIndexInfo, StyleIndexState, index_info, index_info_at, load_effective,
    load_effective_at, neighbour_stems,
};
pub use standardise::{DistanceTerms};
use standardise::{exemplar_is_finite, hubness_profile, retag, standardise};
#[cfg(test)]
use standardise::{tags_from_scores, text_hubness};
pub use switches::{
    DescribeSwitch, EmbeddingSwitch, RetrievalWeights, W_DESC_DEFAULT, W_EMB_DEFAULT,
    W_LOOK_DEFAULT, W_TXT_DEFAULT, embed_provenance_string, model_of, vocab_version_of,
};
#[cfg(test)]
use switches::{ENV_DESC_WEIGHT, ENV_EMBED_WEIGHT, ENV_LOOK_WEIGHT, ENV_TEXT_WEIGHT};
pub use targets::{StyleTargets, TARGET_CONSISTENCY, blend_toward, style_targets};
use targets::{local_slider_mut, style_targets_map};
#[cfg(test)]
use targets::{label_is_one_sided};
pub use voice::{
    COLOUR_HABIT_FLOOR, LOOK_TAGS_K, LOOK_VOCAB, LOOK_VOCAB_VERSION, STYLE_TARGET_MIN, StyleQuery,
    StyleVoice,
};
use voice::{style_colour_floor};

/// The style module's source as ONE text, for the source-text gates that read what
/// `style.rs` alone held before it was split into files: the modules in their
/// declaration order, the root last (so `source_before_tests` cuts at the root's
/// own test modules and nowhere else). Test-only: nothing here is compiled into a
/// shipped binary.
#[cfg(test)]
pub(crate) const SOURCE_FILES: [(&str, &str); 12] = [
    ("src/style/build.rs", include_str!("style/build.rs")),
    ("src/style/distil.rs", include_str!("style/distil.rs")),
    ("src/style/exemplar.rs", include_str!("style/exemplar.rs")),
    ("src/style/features.rs", include_str!("style/features.rs")),
    ("src/style/index.rs", include_str!("style/index.rs")),
    ("src/style/limits.rs", include_str!("style/limits.rs")),
    ("src/style/load.rs", include_str!("style/load.rs")),
    ("src/style/standardise.rs", include_str!("style/standardise.rs")),
    ("src/style/switches.rs", include_str!("style/switches.rs")),
    ("src/style/targets.rs", include_str!("style/targets.rs")),
    ("src/style/voice.rs", include_str!("style/voice.rs")),
    ("src/style.rs", include_str!("style.rs")),
];

#[cfg(test)]
pub(crate) fn source_text() -> String {
    SOURCE_FILES.iter().map(|(_, text)| *text).collect()
}

#[cfg(test)]
mod tests;

