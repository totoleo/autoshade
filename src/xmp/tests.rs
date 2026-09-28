use super::*;
use crate::recipe::{CurvePoint, EditRecipe, LocalAdjustment};

// The tests themselves, in parts under tests/: each file is spliced in here
// by include!, so its items are items of this module and every test keeps
// its name and its path. The order is the order the old file had.
include!("tests/globals_and_scope.rs");
include!("tests/detail_passthrough_looks.rs");
include!("tests/render_gaps_and_radials.rs");
include!("tests/crops_hdr_orientation.rs");
include!("tests/inversion_and_era_gates.rs");
include!("tests/curves_calibration_losses.rs");
include!("tests/brush_groups_and_real_sidecars.rs");
include!("tests/bands_prerename_merge.rs");
include!("tests/round_trips_and_attribute_forms.rs");
include!("tests/ai_masks_and_frame_scope.rs");
include!("tests/mask_brush_tables.rs");
