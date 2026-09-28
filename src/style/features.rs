//! Feature vectors: the library walk, the distilled keys, setting bands, and the sidecar readers.

use super::*;

pub(super) const LOOK_GROUPS: &[&[usize]] = &[&[0,1,2], &[3,4,5,6], &[7,8], &[9,10,11], &[12,13,14,15], &[16,17,18,19], &[20,21,22,23,24], &[25,26,27,28,29,30,31,32]];

/// Every file under `root`, through directory links but never twice through
/// one directory.
///
/// `Path::is_dir` follows symlinks, and the walk used to follow a link back
/// into an ancestor round and round — re-listing the same photos under an
/// ever longer spelling until the kernel refused the path (too many levels of
/// links, or a name too long) and the whole look build failed on `scan`. The
/// rule is `pipeline::walk_photos`'s: a directory is entered once per
/// CANONICAL identity, whatever spelling reaches it. A link OUT to a folder
/// kept elsewhere is still followed — a curated look library legitimately
/// points at references it does not hold itself — and for a tree without a
/// cycle the files found are the files found before.
pub(super) fn walkdir(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(c) = std::fs::canonicalize(&dir)
            && !visited.insert(c)
        {
            continue; // already listed under another spelling
        }
        for ent in std::fs::read_dir(&dir).with_context(|| format!("scan {}", dir.display()))? {
            let p = ent?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.exists() {
                out.push(p);
            } else {
                // A link whose target is gone: `is_dir` and `exists` both
                // follow it and both say no. Listed as a file (until
                // 2026-09-24) it failed the first decode and the whole build
                // with it; a missing reference is named and stepped over.
                eprintln!("⚠ {} points nowhere (a link whose target is missing) — skipped", p.display());
            }
        }
    }
    Ok(out)
}

/// Long edge the preview is reduced to before the embedding sidecar sees it —
/// the ONE frame both the index build and the query go through.
///
/// It exists so the two cannot disagree. The sidecar squashes whatever it is
/// given to 384x384, so the only way a photo's index vector and its query
/// vector could differ is if the two paths handed it different pixels: the
/// build starts from a 61 MP embedded preview and the develop path from an
/// advisor-sized one. Both resize through THIS constant and THIS filter first.
pub(super) const EMBED_FRAME_EDGE: u32 = 512;

/// How many similar past shots one develop leans on. Named since R23-2
/// because the number is now DISCLOSED ("the 4 most similar shots"), so the
/// retrieval and the sentence about it must read the same constant.
pub const RETRIEVE_K: usize = 4;

/// Disclosure bounds for [`neighbour_stems`]: the rationale is persisted,
/// shown in three UIs and capped, so the "which photos did it reference?"
/// answer is bounded at the source rather than trusting `RETRIEVE_K` and the
/// user's file naming to stay small.
pub(super) const MAX_DISCLOSED_NEIGHBOURS: usize = 4;
pub(super) const MAX_STEM_CHARS: usize = 48;

/// The legacy, cwd-relative index a pre-store build wrote. Kept readable so
/// an index built before the central store existed keeps working; nothing
/// writes here any more.
pub const LEGACY_INDEX_PATH: &str = "out/style-index.json";

/// Physical band for feature / normalization values (L04-3): every real
/// dimension is a ln() of a physical quantity or a bounded ratio (|v| ≲ 200
/// by construction — see `feature_vector`), so 1e3 leaves 5× headroom. A
/// merely-FINITE 1e38 passed the old check, overflowed `(v - mean)/std` to
/// ±inf in `normalize`, and two exemplars straddling the mean then handed
/// the retrieval sort a mixed NaN/inf key set — an unspecified ranking (a
/// silently wrong style reference in a PAID prompt) or a sort panic.
pub(crate) const MAX_FEATURE_ABS: f32 = 1e3;

/// Slider keys the reference block shows, as `(crs attribute, label)`.
///
/// A CURATED SUBSET of `advisor::catalogue::RECIPE_CONTROLS`, not a derivation
/// of it (R23-1): the index learns a per-key MEAN across retrieved exemplars,
/// and that only says something for the tone/colour sliders a photographer
/// applies habitually. The registry-consistency test below pins every
/// attribute spelling and field name here to the registry, so a renamed
/// control cannot leave this table pointing at a key nothing writes; the
/// colour FAMILIES are carried as summary statistics instead (see
/// [`StyleExemplar::families`]), because averaging 38 per-band keys across
/// four exemplars is mush.
pub(super) const REF_KEYS: [(&str, &str); 12] = [
    ("Exposure2012", "exposure"),
    ("Contrast2012", "contrast"),
    ("Highlights2012", "highlights"),
    ("Shadows2012", "shadows"),
    ("Whites2012", "whites"),
    ("Blacks2012", "blacks"),
    ("Vibrance", "vibrance"),
    ("Clarity2012", "clarity"),
    ("Temperature", "temperature_K"),
    ("Tint", "tint"),
    ("Saturation", "saturation"),
    ("Dehaze", "dehaze"),
];

/// The `settings` label prefix for a `catalogue::COLOR_GRADE_CRS` field. One
/// spelling, because [`distil_keys`] writes it and [`style_targets`] reads it
/// back.
pub(super) const COLOR_GRADE_LABEL: &str = "color_grade.";

/// The distillation vocabulary the reference BLOCK does not show: the 8-band
/// mixer and the four grade wheels, as `(crs attribute, settings label)`.
///
/// [`REF_KEYS`] above is what the prompt PRINTS. This is the rest of what a
/// photographer's style is made of, read into the same
/// [`StyleExemplar::settings`] map so [`style_targets`] can distil it, and
/// deliberately NOT printed — see the filter in `StyleIndex::render_reference`
/// for why, and for the property that filter buys: an index built WITH this
/// vocabulary renders the same block, byte for byte, as one built without it.
///
/// Why it exists at all (batch 2, the user's ruling of 2026-08-31: *他全局饱和度、
/// 蒙版调色、色温色调、曲线调色，等等，这些都能被考虑到*): distillation used to
/// pull TWELVE flat sliders and nothing else, so at Style 1.0 a proposal's
/// `vibrance` and `saturation` were replaced by the library's means — while the
/// mixer, the wheels, the curve and the masks, which is where this
/// photographer's colour actually lives, carried no target at all. That is not
/// a gentle pull, it is an ASYMMETRY: colour was subtracted in the one channel
/// the library was read through and could not be added back in any of the four
/// it was blind to. The fix is not a cap on the pull; it is giving the pull the
/// rest of the vocabulary.
///
/// DERIVED, never hand-kept, which is the difference from `REF_KEYS` (whose
/// spellings a registry-consistency test has to pin one by one): the crs
/// attributes come from `catalogue::hsl_expansion` and
/// `catalogue::COLOR_GRADE_CRS` — the same two tables the eval ruler measures
/// with and the XMP writer writes — and the label is the metric name those
/// tables already carry. A renamed control moves here with them.
///
/// `settings` is a `BTreeMap`, so ADDING keys is backward compatible in both
/// directions: an index built before batch 2 simply has none of them, every
/// target that would have come from them degrades to "no target", and the
/// distillation is the twelve it always was
/// (`an_index_without_the_new_vocabulary_distils_exactly_the_twelve`). That is
/// why this needs no index-version bump — `CURRENT_INDEX_VERSION` gates a
/// change in what the fourteen FEATURES mean or in how candidates are RANKED,
/// and this touches neither.
pub(super) fn distil_keys() -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(38);
    for f in crate::advisor::catalogue::hsl_expansion() {
        out.push((f.crs, f.metric));
    }
    for (field, key) in crate::advisor::catalogue::COLOR_GRADE_CRS {
        out.push((key.to_string(), format!("{COLOR_GRADE_LABEL}{field}")));
    }
    out
}

/// The band every stored `settings` label is clamped to at the index door —
/// the ONE table both sides of the index read. [`read_settings`] writes every
/// label in [`REF_KEYS`] and [`distil_keys`]; [`StyleIndex::load`] refuses a
/// label this table does not carry. Until v1.2.2 the loader carried its own
/// twelve-label list, so the vocabulary v1.2.0's symmetric distillation added
/// — the 24 HSL cells and the 14 colour-grade fields — was written by every
/// build and rejected by every load: one HSL edit anywhere in the library and
/// the index failed at "exemplar 0 has an unsupported setting key",
/// `style-index --looks` then replaced it with a looks-only file, and the
/// Style control read no edits without saying why. The bands are the
/// recipe's own (`Hsl::clamp` ±100 on every cell; `ColorGrade::clamp` hue
/// 0..360, sat and blending 0..100, lum and balance ±100), so a stored value
/// can never be one the engine would re-clamp. Pinned by
/// `every_label_the_writer_produces_has_a_band_at_the_door` and the save/load
/// round trip beside it.
pub(super) fn setting_bands() -> &'static BTreeMap<String, (f32, f32)> {
    static BANDS: std::sync::OnceLock<BTreeMap<String, (f32, f32)>> = std::sync::OnceLock::new();
    BANDS.get_or_init(|| {
        let mut out = BTreeMap::new();
        for (_, label) in REF_KEYS {
            let band = match label {
                "exposure" => (-5.0, 5.0),
                "temperature_K" => (2000.0, 40000.0),
                "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "vibrance"
                | "clarity" | "tint" | "saturation" | "dehaze" => (-100.0, 100.0),
                other => panic!("REF_KEYS label {other} has no band at the index door"),
            };
            out.insert(label.to_string(), band);
        }
        for f in crate::advisor::catalogue::hsl_expansion() {
            out.insert(f.metric, (-100.0, 100.0));
        }
        for (field, _) in crate::advisor::catalogue::COLOR_GRADE_CRS {
            let band = if field.ends_with("_hue") {
                (0.0, 360.0)
            } else if field.ends_with("_sat") || field == "blending" {
                (0.0, 100.0)
            } else {
                // `*_lum` and `balance`
                (-100.0, 100.0)
            };
            out.insert(format!("{COLOR_GRADE_LABEL}{field}"), band);
        }
        out
    })
}

/// 14-dim feature vector from capture metadata + histogram.
pub fn feature_vector(meta: &Meta, hist: &Histogram) -> [f32; NDIM] {
    let lnpos = |v: f32| if v > 0.0 { v.ln() } else { 0.0 };
    let total: f64 = hist.luma.iter().map(|&v| v as f64).sum::<f64>().max(1.0);
    let mean_of = |b: &[u32]| -> f32 {
        let s: f64 = b.iter().enumerate().map(|(i, &v)| i as f64 * v as f64).sum();
        (s / total) as f32
    };
    let mean_l = mean_of(&hist.luma);
    let var: f64 = hist
        .luma
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let d = i as f64 - mean_l as f64;
            d * d * v as f64
        })
        .sum::<f64>()
        / total;
    let std_l = var.sqrt() as f32;
    let (mr, mg, mb) = (mean_of(&hist.r), mean_of(&hist.g), mean_of(&hist.b));
    let hour = parse_hour(meta.date_time.as_deref());
    let (w, h) = (meta.width.max(1) as f32, meta.height.max(1) as f32);
    let wb = meta.as_shot_wb_coeffs;
    let warmth = if wb[0] > 0.0 && wb[2] > 0.0 { (wb[0] / wb[2]).ln() } else { 0.0 };
    let ang = std::f32::consts::TAU * hour / 24.0;
    [
        lnpos(meta.focal_length_mm.unwrap_or(35.0)),
        lnpos(meta.iso.unwrap_or(100) as f32),
        lnpos(meta.aperture.unwrap_or(5.6)),
        ang.sin(),
        ang.cos(),
        mean_l / 255.0,
        hist.clip_black_pct / 100.0,
        hist.clip_white_pct / 100.0,
        (mr - mg) / 255.0,
        (mb - mg) / 255.0,
        warmth,
        w / h,
        std_l / 255.0,
        if h > w { 1.0 } else { 0.0 },
    ]
}

pub(super) fn parse_hour(dt: Option<&str>) -> f32 {
    // EXIF "2023:06:01 14:30:00" → 14
    dt.and_then(|s| s.split(' ').nth(1))
        .and_then(|t| t.split(':').next())
        .and_then(|h| h.parse::<f32>().ok())
        // EXIF is other-software input: a NaN/absurd hour would poison the
        // hour-angle feature and with it every retrieval distance.
        .filter(|h| h.is_finite() && (0.0..24.0).contains(h))
        .unwrap_or(12.0)
}

/// Is this sidecar's frame a BLACK-AND-WHITE conversion?
///
/// Three spellings, because Adobe has used three and a library spans years:
/// `crs:Treatment="Black & White"` (current), `crs:ConvertToGrayscale="True"`
/// (the older boolean), and a global `crs:Saturation` at the −100 floor, which
/// is the colour treatment that leaves no colour. Any one of them means the
/// mixer cells this build stores describe a control the render did not use.
///
/// Read through the SAME scope rule as [`read_settings`]: a nested creative
/// Look's baked black-and-white profile belongs to the profile, not to the
/// photographer, and learning it would teach the index that this user
/// "shoots monochrome".
pub(super) fn read_monochrome(xmp: &str) -> bool {
    let scoped = crate::xmp::crs_own_scope(xmp);
    let scoped = crate::xmp::Scope::new(scoped.as_ref());
    let treatment = scoped.crs_str("Treatment");
    let grayscale = scoped.crs_str("ConvertToGrayscale");
    treatment.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("black & white"))
        || grayscale.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("true"))
        || scoped.crs_f32("Saturation").is_some_and(|s| s <= -100.0)
}

pub(super) fn read_settings(xmp: &str) -> BTreeMap<String, f32> {
    // As-shot provenance (same rule as the eval harness): under
    // WhiteBalance="As Shot", crs:Temperature/Tint record the CAMERA's
    // values, not a user edit — learning them taught the style index to
    // "prefer" whatever Kelvin the user's camera metered. Any other value
    // (Custom / a preset) is a user decision; absent = non-LR, kept.
    // Same scope rule as every other whole-document reader
    // (`xmp::crs_own_scope`): a nested creative Look's baked parameters belong
    // to the PROFILE, not the photographer, and learning them would teach the
    // index that this user "prefers" whatever look Adobe ships — the same
    // provenance error the as-shot rule above guards against, one container
    // deeper.
    let xmp = crate::xmp::crs_own_scope(xmp);
    // A `Scope`, spelled out (R28 Batch-5 5d): subtree-wide first-match is what
    // a whole-sidecar read means, and the type is what says so now.
    let xmp = crate::xmp::Scope::new(xmp.as_ref());
    let user_wb = xmp.crs_str("WhiteBalance").as_deref() != Some("As Shot");
    // The printed twelve and the distillation vocabulary go into ONE map,
    // through ONE reader, under ONE provenance rule (batch 2). Two readers
    // would be two chances for the as-shot rule above to be applied to half a
    // sidecar.
    REF_KEYS
        .iter()
        .map(|(k, label)| ((*k).to_string(), (*label).to_string()))
        .chain(distil_keys())
        .filter(|(k, _)| user_wb || !matches!(k.as_str(), "Temperature" | "Tint"))
        .filter_map(|(k, label)| xmp.crs_f32(&k).map(|v| (label, v)))
        .collect()
}

/// Short human tag like "tele/bright/midday" for the reference block.
pub(super) fn derive_tag(f: &[f32; NDIM]) -> String {
    let focal = f[0].exp();
    let lens = if focal < 24.0 { "ultrawide" } else if focal < 45.0 { "wide" } else if focal < 90.0 { "normal" } else { "tele" };
    let tone = if f[5] < 0.33 { "dark" } else if f[5] > 0.6 { "bright" } else { "mid" };
    let hour = (f[3].atan2(f[4]) / std::f32::consts::TAU * 24.0 + 24.0) % 24.0;
    let tod = if !(5.0..20.0).contains(&hour) { "night" } else if (5.0..9.0).contains(&hour) || (17.0..20.0).contains(&hour) { "goldenish" } else { "midday" };
    let orient = if f[13] > 0.5 { "portrait" } else { "landscape" };
    format!("{lens}/{tone}/{tod}/{orient}")
}
