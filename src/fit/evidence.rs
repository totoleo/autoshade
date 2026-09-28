//! The evidence model: fit modes, divergence, the luma and hue ranges, their aggregation and withholding, and the atmosphere white-balance pairing.

use super::*;

/// The global reverse-fit policy selected before any CDF solve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitMode {
    Full,
    Atmosphere,
}

/// The two calibrated components of structural divergence and their Euclidean
/// combination `d = sqrt((1-correlation)^2 + energy_error^2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divergence {
    pub correlation: f32,
    pub energy_error: f32,
    pub d: f32,
}

/// Evidence carried by one value range.  A range is identifiable only when
/// both images populate it and its own structural comparison is stable.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceRange {
    pub label: String,
    pub source_share: f32,
    pub target_share: f32,
    pub source_evidence_share: f32,
    pub target_evidence_share: f32,
    pub two_sided_share: f32,
    pub divergence: f32,
    pub weight: f32,
    pub source_populated: bool,
    pub target_populated: bool,
}

/// One evidence map is shared by the objective, all gates, confidence and
/// disclosures.  Pixel weights are attached to the source/target analysis
/// rasters; candidates keep the source weights, so edits cannot move pixels
/// into evidence after the fact.
#[derive(Clone, Debug)]
pub struct EvidenceModel {
    pub source_pixels: Vec<[f32; 3]>,
    /// Soft membership of each aligned source pixel in this model's
    /// population: all ones for the frame, the source coverage for a scoped
    /// view. Blind-move numerators use the same mass as [`Self::population`].
    pub source_membership: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub spatial_supported: Vec<bool>,
    pub source_weights: Vec<f32>,
    pub target_weights: Vec<f32>,
    pub source_hue_weights: Vec<f32>,
    pub target_hue_weights: Vec<f32>,
    pub luma: Vec<EvidenceRange>,
    pub hue: Vec<EvidenceRange>,
    /// Population chromaticity facts used to recognize a coherent global cast.
    pub source_chroma_vector: [f32; 2],
    pub target_chroma_vector: [f32; 2],
    pub global_cast: Option<GlobalCast>,
    pub identifiability: f32,
    /// Per-pixel ingredients of the range verdicts above — the pixel's
    /// spatial-cell confidence, that cell's divergence and the frame-wide
    /// same-content verdict — kept so [`EvidenceModel::scoped`] can
    /// re-aggregate the same support over a zone's own population.
    pub spatial_weights: Vec<f32>,
    pub spatial_divergence: Vec<f32>,
    pub globally_same_content: bool,
    /// Weighted member count of the population these verdicts are over: the
    /// frame's pixel count for the global model, a coverage's mass for a
    /// scoped view. The blind-move audit's region line is a share of THIS,
    /// so a half-withheld 6% tile is a region of its own population, not a
    /// 3% speckle of the frame.
    pub population: f32,
}

impl EvidenceModel {
    /// This model's range verdicts re-aggregated over ONE zone: `source_zone`
    /// / `target_zone` are the zone's soft memberships on the analysis
    /// rasters, `tp` the target's analysis pixels. Evidence verdicts follow
    /// the population a correction MOVES — the frame for the global fit, the
    /// zone for a zone — so a ground zone is no longer withheld because a
    /// replaced sky happens to share its luma bins, while a zone whose own
    /// members are divergent stays withheld unless the frame-wide
    /// same-content verdict holds. That verdict deliberately bypasses range
    /// survival and per-pixel withholding. Per-pixel support
    /// (`spatial_supported`) is unchanged; over the whole aligned frame this
    /// is the model itself. The two rasters share one geometry by
    /// construction ([`analysis_pair`]); the aligned-prefix arithmetic below
    /// is the defensive form of that contract.
    pub fn scoped(&self, tp: &[[f32; 3]], source_zone: &[f32], target_zone: &[f32]) -> EvidenceModel {
        let ranges = aggregate_ranges(
            &self.source_pixels,
            tp,
            source_zone,
            target_zone,
            SupportField {
                spatial_weights: &self.spatial_weights,
                spatial_divergence: &self.spatial_divergence,
                globally_same_content: self.globally_same_content,
            },
        );
        EvidenceModel {
            source_pixels: self.source_pixels.clone(),
            source_membership: ranges.source_membership,
            width: self.width,
            height: self.height,
            spatial_supported: self.spatial_supported.clone(),
            source_weights: ranges.source_weights,
            target_weights: ranges.target_weights,
            source_hue_weights: ranges.source_hue_weights,
            target_hue_weights: ranges.target_hue_weights,
            luma: ranges.luma,
            hue: ranges.hue,
            source_chroma_vector: self.source_chroma_vector,
            target_chroma_vector: self.target_chroma_vector,
            global_cast: self.global_cast,
            identifiability: ranges.identifiability,
            spatial_weights: self.spatial_weights.clone(),
            spatial_divergence: self.spatial_divergence.clone(),
            globally_same_content: self.globally_same_content,
            population: ranges.population,
        }
    }

    /// Re-aggregate this frame on Atmosphere mode's structure-blind evidence
    /// doctrine. Atmosphere mode is entered because structure diverges; its
    /// instruments are the budgets (EV +/-1, WB gain [0.80, 1.25], saturation
    /// +/-30, curve slope [0.5, 1.5]) and the population facts, not structural
    /// survival. Population vetoes remain intact because [`Self::scoped`]
    /// re-derives them from the unchanged source and target pixels.
    ///
    /// On a model that is already blind — every spatial weight 1.0, every
    /// pixel supported, `globally_same_content` true, as an identical pair
    /// produces — this is byte-equal to `scoped(tp, ones, ones)`, and the unit
    /// test pins that. Anywhere else the two differ by design: `scoped` reads
    /// the model's own structural weights and support, and replacing those is
    /// this function's whole job.
    pub fn structure_blind(&self, tp: &[[f32; 3]]) -> EvidenceModel {
        let n = self.source_pixels.len().min(tp.len());
        let ones = vec![1.0; n];
        let mut blind = self.clone();
        blind.spatial_weights = ones.clone();
        blind.spatial_supported = vec![true; n];
        blind.globally_same_content = true;
        blind.scoped(tp, &ones, &ones)
    }
}

pub(super) const EVIDENCE_LUMA_BINS: usize = 17;
pub(super) const EVIDENCE_HUE_BANDS: usize = 8;
pub(super) const EVIDENCE_MIN_SHARE: f32 = 0.015;
const EVIDENCE_DIVERGENCE_CUTOFF: f32 = 1.0;
/// The range form of the existing per-zone divergence policy. A range must
/// retain at least the support left at `DIVERGENCE_ZONE`; the texture
/// calibration is D=0.628 and survives, while the invented-sky value ranges
/// retain only 9-16% and do not.
pub(super) const EVIDENCE_RANGE_SURVIVAL_MIN: f32 = 1.0 - DIVERGENCE_ZONE;
pub(crate) const UNSUPPORTED_RANGE_MOVE: f32 = 2.0 / 255.0;
/// The same-content texture calibration retains 0.341 identifiability; the
/// invented-sky pair retains 0.218. Detail fitting is enabled between them.
pub(super) const DETAIL_EVIDENCE_MIN_IDENTIFIABILITY: f32 = 0.30;

/// Fold the existing 17-bin luma verdict over one inclusive contiguous run.
/// Range partitioning uses this instead of inventing a second evidence model:
/// population, structural survival and estimator weight retain the global
/// fit's exact per-bin meanings.
pub(crate) fn luma_evidence_for_bins(
    evidence: &EvidenceModel,
    first: usize,
    last: usize,
) -> EvidenceRange {
    let end = last.saturating_add(1).min(evidence.luma.len());
    let start = first.min(end);
    // The label names the bins actually FOLDED: `last` clamps to the model's
    // top bin here, and printing the request would name a bin never read.
    let last = end.saturating_sub(1).max(start);
    let bins = &evidence.luma[start..end];
    let source_share = bins.iter().map(|r| r.source_share).sum::<f32>();
    let target_share = bins.iter().map(|r| r.target_share).sum::<f32>();
    let source_evidence_share = bins.iter().map(|r| r.source_evidence_share).sum::<f32>();
    let target_evidence_share = bins.iter().map(|r| r.target_evidence_share).sum::<f32>();
    let two_sided_share = source_evidence_share.min(target_evidence_share);
    let structural_mass = bins.iter().map(|r| r.weight).sum::<f32>();
    let divergence_weight = bins
        .iter()
        .filter(|r| r.divergence.is_finite())
        .map(|r| r.two_sided_share)
        .sum::<f32>();
    let divergence = if divergence_weight > 0.0 {
        bins.iter()
            .filter(|r| r.two_sided_share > 0.0 && r.divergence.is_finite())
            .map(|r| r.divergence * r.two_sided_share)
            .sum::<f32>()
            / divergence_weight
    } else {
        f32::INFINITY
    };
    let source_populated = source_share >= EVIDENCE_MIN_SHARE;
    let target_populated = target_share >= EVIDENCE_MIN_SHARE;
    EvidenceRange {
        label: format!("luma bins {start:02}-{last:02}"),
        source_share,
        target_share,
        source_evidence_share,
        target_evidence_share,
        two_sided_share,
        divergence,
        weight: if source_populated && target_populated { structural_mass } else { 0.0 },
        source_populated,
        target_populated,
    }
}

pub fn evidence_luma_bin(v: f32) -> usize {
    ((v.clamp(0.0, 1.0) * EVIDENCE_LUMA_BINS as f32).floor() as usize)
        .min(EVIDENCE_LUMA_BINS - 1)
}

pub fn evidence_hue_band(p: &[f32; 3]) -> Option<usize> {
    let chroma = p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
    if chroma < 0.06 {
        return None;
    }
    let (h, _, _) = render::rgb_to_hsl(p[0], p[1], p[2]);
    let (b0, b1, w1) = render::bracket_bands(h * 360.0, &render::HSL_CENTERS);
    Some(if w1 < 0.5 { b0 } else { b1 })
}

fn evidence_range(
    label: String,
    source_members: &[f32],
    target_members: &[f32],
    source_population: f32,
    target_population: f32,
    support: SupportField<'_>,
) -> EvidenceRange {
    let SupportField { spatial_weights, spatial_divergence, globally_same_content } = support;
    // Memberships are soft weights (1.0 everywhere for the frame); the
    // populations are the weighted member counts the shares are taken over.
    let source_n = source_population.max(1.0);
    let target_n = target_population.max(1.0);
    let source_share = source_members.iter().sum::<f32>() / source_n;
    let target_share = target_members.iter().sum::<f32>() / target_n;
    let supported_share = |members: &[f32], population: f32| {
        members
            .iter()
            .zip(spatial_weights)
            .map(|(&member, &weight)| member * weight)
            .sum::<f32>()
            / population
    };
    let source_evidence_share = supported_share(source_members, source_n);
    let target_evidence_share = supported_share(target_members, target_n);
    // Population and structural support are two different facts.  The 1.5%
    // line answers only whether the range exists on each side; applying it a
    // second time after the divergence discount made a weakly-correlated
    // population look absent.  A populated range can therefore be withheld
    // for structure (zero `two_sided_share`) without being mislabeled as
    // one-sided/empty in the disclosure.
    let source_populated = source_share >= EVIDENCE_MIN_SHARE;
    let target_populated = target_share >= EVIDENCE_MIN_SHARE;
    let two_sided_share = source_evidence_share.min(target_evidence_share);
    let (div_sum, div_weight) = source_members
        .iter()
        .zip(spatial_weights)
        .zip(spatial_divergence)
        .filter(|((member, weight), divergence)| {
            **member > 0.0 && **weight > 0.0 && divergence.is_finite()
        })
        .fold((0.0f32, 0.0f32), |(sum, weight), ((&m, &w), &d)| {
            (sum + m * w * d, weight + m * w)
        });
    let divergence = if div_weight > 0.0 { div_sum / div_weight } else { f32::INFINITY };
    let structural_survival = (source_evidence_share / source_share.max(1e-6))
        .min(target_evidence_share / target_share.max(1e-6));
    let weight = if source_populated
        && target_populated
        && (globally_same_content || structural_survival >= EVIDENCE_RANGE_SURVIVAL_MIN)
    {
        two_sided_share
    } else {
        0.0
    };
    EvidenceRange {
        label,
        source_share,
        target_share,
        source_evidence_share,
        target_evidence_share,
        two_sided_share,
        divergence,
        weight,
        source_populated,
        target_populated,
    }
}

/// The value-range verdicts and per-pixel evidence weights of ONE population.
/// The global fit moves the whole frame and is judged by the frame's bins; a
/// zone moves only its members, so [`EvidenceModel::scoped`] re-aggregates the
/// same per-pixel structural support over them. Memberships are soft (a
/// refined mask's feather), the frame is the all-ones case, and target luma
/// bins are rank-paired within the population's own target members at its own
/// source-to-target mass ratio. Every sum and share uses the one aligned
/// `0..min(source.len(), target.len())` prefix. A divergent population is
/// withheld unless the frame-wide same-content verdict deliberately bypasses
/// range survival and per-pixel withholding.
pub(super) struct RangeAggregate {
    /// Source membership over the same aligned prefix as every range sum.
    source_membership: Vec<f32>,
    source_weights: Vec<f32>,
    target_weights: Vec<f32>,
    source_hue_weights: Vec<f32>,
    target_hue_weights: Vec<f32>,
    pub(super) luma: Vec<EvidenceRange>,
    hue: Vec<EvidenceRange>,
    identifiability: f32,
    population: f32,
}

/// The per-pixel structural support one population's verdicts are read
/// against: the frame's spatial-cell confidence, that cell's divergence and
/// the frame-wide same-content verdict.
#[derive(Clone, Copy)]
pub(super) struct SupportField<'a> {
    pub(super) spatial_weights: &'a [f32],
    pub(super) spatial_divergence: &'a [f32],
    pub(super) globally_same_content: bool,
}

pub(super) fn aggregate_ranges(
    sp: &[[f32; 3]],
    tp: &[[f32; 3]],
    source_zone: &[f32],
    target_zone: &[f32],
    support: SupportField<'_>,
) -> RangeAggregate {
    let spatial_weights = support.spatial_weights;
    let n = sp.len().min(tp.len());
    let member = |zone: &[f32], i: usize| zone.get(i).copied().unwrap_or(0.0).max(0.0);
    let source_membership = (0..n).map(|i| member(source_zone, i)).collect::<Vec<_>>();
    let source_mass = source_membership.iter().sum::<f32>();
    let target_mass = (0..n).map(|i| member(target_zone, i)).sum::<f32>();
    let mut luma = Vec::with_capacity(EVIDENCE_LUMA_BINS);
    let mut luma_source_weights = vec![0.0f32; n];
    let mut luma_target_weights = vec![0.0f32; n];
    let mut target_order: Vec<usize> = (0..n).filter(|&i| member(target_zone, i) > 0.0).collect();
    target_order.sort_by(|&a, &b| luma601(&tp[a]).total_cmp(&luma601(&tp[b])));
    let ratio = if source_mass > 0.0 { target_mass / source_mass } else { 0.0 };
    let mut cursor = 0usize;
    let mut taken = 0.0f32;
    let mut quota = 0.0f32;
    for bin in 0..EVIDENCE_LUMA_BINS {
        let sm: Vec<f32> = sp[..n]
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if evidence_luma_bin(luma601(p)) == bin { member(source_zone, i) } else { 0.0 }
            })
            .collect();
        let lo = bin as f32 / EVIDENCE_LUMA_BINS as f32;
        let hi = (bin + 1) as f32 / EVIDENCE_LUMA_BINS as f32;
        // Luma correspondence is monotone: a real exposure/tone edit moves a
        // source bin to a different numeric target interval. Pair the bin to
        // the same target population ranks, then let spatial evidence decide
        // whether that population actually survives on both sides.
        let mut tm = vec![0.0f32; n];
        quota += sm.iter().sum::<f32>() * ratio;
        // A rank-boundary target member is consumed whole. Consequently the
        // cumulative allocation can drift by at most one member over the
        // entire population, rather than one fresh overshoot in every bin.
        while cursor < target_order.len() && taken < quota {
            let i = target_order[cursor];
            tm[i] = member(target_zone, i);
            taken += tm[i];
            cursor += 1;
        }
        let range = evidence_range(
            format!("luma[{lo:.2}-{hi:.2}]"),
            &sm,
            &tm,
            source_mass,
            target_mass,
            support,
        );
        for (i, &m) in sm.iter().enumerate().take(n) {
            if m > 0.0 && range.source_evidence_share > 0.0 {
                luma_source_weights[i] =
                    m * spatial_weights[i] * range.weight / range.source_evidence_share;
            }
        }
        for (i, &m) in tm.iter().enumerate().take(n) {
            if m > 0.0 && range.target_evidence_share > 0.0 {
                luma_target_weights[i] =
                    m * spatial_weights[i] * range.weight / range.target_evidence_share;
            }
        }
        luma.push(range);
    }
    let mut hue = Vec::with_capacity(EVIDENCE_HUE_BANDS);
    let mut source_hue_weights = vec![0.0f32; n];
    let mut target_hue_weights = vec![0.0f32; n];
    for band in 0..EVIDENCE_HUE_BANDS {
        let sm: Vec<f32> = sp[..n]
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if evidence_hue_band(p) == Some(band) { member(source_zone, i) } else { 0.0 }
            })
            .collect();
        let tm: Vec<f32> = tp[..n]
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if evidence_hue_band(p) == Some(band) { member(target_zone, i) } else { 0.0 }
            })
            .collect();
        let range = evidence_range(
            crate::recipe::HSL_BANDS[band].to_string(),
            &sm,
            &tm,
            source_mass,
            target_mass,
            support,
        );
        for (i, &m) in sm.iter().enumerate().take(n) {
            if m > 0.0 && range.source_evidence_share > 0.0 {
                source_hue_weights[i] =
                    m * spatial_weights[i] * range.weight / range.source_evidence_share;
            }
        }
        for (i, &m) in tm.iter().enumerate().take(n) {
            if m > 0.0 && range.target_evidence_share > 0.0 {
                target_hue_weights[i] =
                    m * spatial_weights[i] * range.weight / range.target_evidence_share;
            }
        }
        hue.push(range);
    }
    let luma_mass = luma.iter().map(|r| r.weight).sum::<f32>().min(1.0);
    let hue_mass = hue.iter().map(|r| r.weight).sum::<f32>().min(1.0);
    let identifiability = (0.75 * luma_mass + 0.25 * hue_mass).clamp(0.0, 1.0);
    RangeAggregate {
        source_membership,
        source_weights: luma_source_weights,
        target_weights: luma_target_weights,
        source_hue_weights,
        target_hue_weights,
        luma,
        hue,
        identifiability,
        population: source_mass,
    }
}

/// Per-pixel structural support from contiguous cells. The divergence
/// primitive expects coherent image geometry (erosion, gradients and Gaussian
/// bands), so value-bin masks are applied only after these cell readings.
fn spatial_evidence(
    sp: &[[f32; 3]],
    tp: &[[f32; 3]],
    w: u32,
    h: u32,
) -> (Vec<f32>, Vec<f32>) {
    const COLS: u32 = 3;
    const ROWS: u32 = 3;
    let scale = w.max(h).div_ceil(192).max(1);
    let sw = w.div_ceil(scale);
    let sh = h.div_ceil(scale);
    let mut ss = Vec::with_capacity((sw * sh) as usize);
    let mut tt = Vec::with_capacity((sw * sh) as usize);
    for y in (0..h).step_by(scale as usize) {
        for x in (0..w).step_by(scale as usize) {
            let i = (y * w + x) as usize;
            ss.push(sp[i]);
            tt.push(tp[i]);
        }
    }
    let mut weights = vec![0.0f32; sp.len().min(tp.len())];
    let mut divergences = vec![f32::INFINITY; weights.len()];
    let mut readings = Vec::with_capacity((ROWS * COLS) as usize);
    for row in 0..ROWS {
        for col in 0..COLS {
            let mut mask = vec![0.0f32; ss.len()];
            for y in 0..sh {
                for x in 0..sw {
                    if x * COLS / sw.max(1) == col && y * ROWS / sh.max(1) == row {
                        mask[(y * sw + x) as usize] = 1.0;
                    }
                }
            }
            readings.push(structure_divergence(&ss, &tt, sw, sh, &mask));
        }
    }
    // WHOLESALE, or per cell. A frame whose every cell falls under the
    // instrument's resolvable core carries no structural reading anywhere,
    // and withholding every pixel's evidence on that ground would starve a
    // frame nothing measured; there the modulation is dropped, which is the
    // behaviour this function had before the instrument learned to abstain.
    // Where SOME cell resolves the frame IS measured, and a cell that did not
    // resolve earns no support claim: its divergence keeps the initialised
    // infinity and its confidence stays 0.
    if readings.iter().all(Option::is_none) {
        return (vec![1.0; weights.len()], vec![0.0; divergences.len()]);
    }
    for (cell, reading) in readings.into_iter().enumerate() {
        let (row, col) = (cell as u32 / COLS, cell as u32 % COLS);
        let Some(reading) = reading else { continue };
        let d = reading.d;
        let confidence = (1.0 - d / EVIDENCE_DIVERGENCE_CUTOFF).clamp(0.0, 1.0);
        for y in 0..h {
            for x in 0..w {
                if x * COLS / w.max(1) == col && y * ROWS / h.max(1) == row {
                    let i = (y * w + x) as usize;
                    weights[i] = confidence;
                    divergences[i] = d;
                }
            }
        }
    }
    (weights, divergences)
}

/// Build the single per-pixel/per-range evidence map.  Range weights are the
/// two-sided population share discounted by that range's existing structural
/// divergence; no second divergence statistic is introduced.
fn inferred_geometry(n: usize) -> (u32, u32) {
    if n > 0 {
        let mut h = (n as f64).sqrt().floor() as usize;
        while h > 1 && !n.is_multiple_of(h) {
            h -= 1;
        }
        ((n / h) as u32, h as u32)
    } else {
        (0, 0)
    }
}

pub fn evidence_model_for(
    sp: &[[f32; 3]],
    tp: &[[f32; 3]],
    width: u32,
    height: u32,
) -> EvidenceModel {
    let n = sp.len().min(tp.len());
    let (w, h) = if width as usize * height as usize == n {
        (width, height)
    } else {
        inferred_geometry(n)
    };
    let (spatial_weights, spatial_divergence) = spatial_evidence(sp, tp, w, h);
    let all_spatial = vec![1.0f32; n];
    // The frame-wide reading carries the same wholesale rule: its mask is
    // all-ones, so no cell mask can resolve where this one abstains, and an
    // abstention here means the frame itself is under the instrument's
    // resolvable core. That is not a verdict about content, so it must not
    // withhold every range's evidence.
    let global_reading = structure_divergence(sp, tp, w, h, &all_spatial);
    let globally_same_content = global_reading.is_none_or(|r| r.d < DIVERGENCE_GLOBAL);
    let spatial_supported = spatial_divergence
        .iter()
        .map(|&divergence| globally_same_content || divergence < DIVERGENCE_ZONE)
        .collect::<Vec<_>>();
    let frame = vec![1.0f32; n];
    let ranges = aggregate_ranges(
        sp,
        tp,
        &frame,
        &frame,
        SupportField {
            spatial_weights: &spatial_weights,
            spatial_divergence: &spatial_divergence,
            globally_same_content,
        },
    );
    let source_chroma_vector = mean_chroma_vector(sp, &frame);
    let target_chroma_vector = mean_chroma_vector(tp, &frame);
    let global_cast = detect_global_cast(sp, tp, &ranges.hue);
    EvidenceModel {
        source_pixels: sp.iter().take(n).copied().collect(),
        source_membership: ranges.source_membership,
        width: w,
        height: h,
        spatial_supported,
        source_weights: ranges.source_weights,
        target_weights: ranges.target_weights,
        source_hue_weights: ranges.source_hue_weights,
        target_hue_weights: ranges.target_hue_weights,
        luma: ranges.luma,
        hue: ranges.hue,
        source_chroma_vector,
        target_chroma_vector,
        global_cast,
        identifiability: ranges.identifiability,
        spatial_weights,
        spatial_divergence,
        globally_same_content,
        population: ranges.population,
    }
}

#[cfg(test)]
pub(crate) fn evidence_model(sp: &[[f32; 3]], tp: &[[f32; 3]]) -> EvidenceModel {
    let (width, height) = inferred_geometry(sp.len().min(tp.len()));
    evidence_model_for(sp, tp, width, height)
}

pub(super) fn movement_identifiability(after: &[[f32; 3]], evidence: &EvidenceModel) -> f32 {
    let mut unsupported = 0.0f32;
    for (i, (before, after)) in evidence.source_pixels.iter().zip(after).enumerate() {
        let movement = (0..3).map(|ch| (after[ch] - before[ch]).abs()).sum::<f32>() / 3.0;
        let unsupported_luma = source_luma_is_withheld(i, evidence);
        let unsupported_hue = evidence_hue_band(before).is_some()
            && source_hue_is_withheld(i, evidence);
        if unsupported_luma || unsupported_hue {
            unsupported += movement;
        }
    }
    let mean_unsupported = unsupported / evidence.source_pixels.len().max(1) as f32;
    (-UNSUPPORTED_MOVEMENT_CONFIDENCE_SLOPE * mean_unsupported)
        .exp()
        .clamp(0.0, 1.0)
}

pub(super) fn source_luma_is_withheld(index: usize, evidence: &EvidenceModel) -> bool {
    let Some(pixel) = evidence.source_pixels.get(index) else { return false };
    let range = &evidence.luma[evidence_luma_bin(luma601(pixel))];
    range.source_populated
        && (range.weight <= 0.0
            || !evidence.spatial_supported.get(index).copied().unwrap_or(false))
}

pub(super) fn source_hue_is_withheld(index: usize, evidence: &EvidenceModel) -> bool {
    let Some(pixel) = evidence.source_pixels.get(index) else { return false };
    let Some(band) = evidence_hue_band(pixel) else { return false };
    let range = &evidence.hue[band];
    range.source_populated
        && (range.weight <= 0.0
            || !evidence.spatial_supported.get(index).copied().unwrap_or(false))
}

fn range_is_withheld(range: &EvidenceRange) -> bool {
    range.weight <= 0.0 && (range.source_populated || range.target_populated)
}

fn withheld_range_hits(evidence: &EvidenceModel) -> (Vec<bool>, Vec<bool>) {
    let mut luma = evidence
        .luma
        .iter()
        .map(range_is_withheld)
        .collect::<Vec<_>>();
    let mut hue = evidence
        .hue
        .iter()
        .map(range_is_withheld)
        .collect::<Vec<_>>();
    for (index, pixel) in evidence.source_pixels.iter().enumerate() {
        if !evidence.spatial_supported.get(index).copied().unwrap_or(false) {
            luma[evidence_luma_bin(luma601(pixel))] = true;
            if let Some(band) = evidence_hue_band(pixel) {
                hue[band] = true;
            }
        }
    }
    (luma, hue)
}

pub(crate) fn withheld_range_names(evidence: &EvidenceModel) -> (String, String) {
    let (luma, hue) = withheld_range_hits(evidence);
    let names = |hits: &[bool], ranges: &[EvidenceRange]| {
        hits
            .iter()
            .zip(ranges)
            .filter_map(|(&hit, range)| hit.then_some(range.label.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    (names(&luma, &evidence.luma), names(&hue, &evidence.hue))
}

/// Whether the shared evidence model withheld a populated range because only
/// one image carried it. This is the cause input for the single FAR classifier.
pub(crate) fn evidence_has_one_sided(evidence: &EvidenceModel) -> bool {
    evidence
        .luma
        .iter()
        .chain(&evidence.hue)
        .any(|range| {
            range.weight <= 0.0 && range.source_populated != range.target_populated
        })
}

pub(super) fn divergent_range_names(evidence: &EvidenceModel) -> String {
    let (luma, hue) = withheld_range_hits(evidence);
    luma.iter()
        .zip(&evidence.luma)
        .chain(hue.iter().zip(&evidence.hue))
        .filter_map(|(&withheld, range)| {
            (withheld && range.source_populated && range.target_populated)
                .then_some(range.label.as_str())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn weighted_cdf(px: &[[f32; 3]], weights: &[f32], value: impl Fn(&[f32; 3]) -> f32) -> Vec<f32> {
    let mut hist = vec![0.0f32; HIST_BINS];
    let mut total = 0.0f32;
    for (i, p) in px.iter().enumerate() {
        let w = weights.get(i).copied().unwrap_or(0.0).max(0.0);
        if w <= 0.0 { continue; }
        let bin = (value(p).clamp(0.0, 1.0) * (HIST_BINS - 1) as f32).round() as usize;
        hist[bin] += w;
        total += w;
    }
    if total <= 1e-8 { return vec![0.0; HIST_BINS]; }
    let mut acc = 0.0;
    for v in &mut hist { acc += *v; *v = acc / total; }
    hist
}

/// Weighted median of a scattered sample, lower-median convention: the first
/// value at which the cumulative weight reaches half of the total.
///
/// Deliberately NOT [`weighted_cdf`] + [`quantile`]. That pair bins its key
/// into `HIST_BINS` buckets over `[0, 1]`, which is right for a channel value
/// and wrong for the quantity this batch reads: a per-pixel LOG RATIO is
/// signed, unbounded, and concentrated near zero, so a `[0, 1]` histogram
/// would clamp half of it into one bin. Sorting is exact and the cost is one
/// sort of the analysis frame per channel, against thirteen full renders on
/// the same path.
fn weighted_median(samples: &mut [(f32, f32)]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = samples.iter().map(|&(_, w)| w as f64).sum::<f64>();
    if total <= 0.0 {
        return 0.0;
    }
    let half = total * 0.5;
    let mut carried = 0.0f64;
    for &(value, weight) in samples.iter() {
        carried += weight as f64;
        if carried >= half {
            return value;
        }
    }
    samples[samples.len() - 1].0
}

/// The population AND the pairing the Atmosphere white balance is solved on.
/// ONE authority chooses both, and that is the point: a readable
/// shared-content population EXISTS only because a correspondence field was
/// readable, and the field that is trusted to say WHICH pixels are shared is
/// the same field that says WHICH target pixel each source pixel corresponds
/// to. Taking the population without the pairing is how a RECOMPOSED pair —
/// the same content, moved in frame, which is squarely inside Atmosphere's
/// remit — gets read as a colour cast: same-index pairing against the raw
/// target array is a random association once content has moved.
///
/// With no readable field the two sides are index-paired exactly as they
/// always were, and the per-pixel weight is `min(source, target)`. That is
/// not an invention: it is this module's own two-sidedness rule, the
/// `source_evidence_share.min(target_evidence_share)` that [`EvidenceModel`]
/// already applies to every range.
///
/// With a readable field `p.source` ALONE is the coherent weight, and mixing
/// in `p.target` would not be. `shared_content_population` sets `source[i]`
/// exactly where `conf[i] >= CONFIDENT_MATCH`, i.e. exactly where `pair_tp[i]`
/// IS that source pixel's counterpart; the target weights are RANK-paired
/// (built by sorting `luma601(&tp[..])`), so `target_weights[i]` is a
/// statement about `tp[i]`'s own luma rank, not about a pairing with `sp[i]`.
pub(super) fn atmosphere_wb_pairing<'a>(
    tp: &'a [[f32; 3]],
    evidence: &'a EvidenceModel,
    correspondence: Option<&'a PairCorrespondence>,
    readable: Option<&'a SharedPopulation>,
) -> (&'a [[f32; 3]], Cow<'a, [f32]>) {
    match readable {
        Some(p) => (
            correspondence.map_or(tp, |c| c.tp.as_slice()),
            Cow::Borrowed(p.source.as_slice()),
        ),
        None => {
            let n = evidence.source_weights.len().min(evidence.target_weights.len());
            let paired = (0..n)
                .map(|i| evidence.source_weights[i].min(evidence.target_weights[i]))
                .collect::<Vec<_>>();
            (tp, Cow::Owned(paired))
        }
    }
}

/// The Atmosphere global white balance: a weighted median of the PER-PIXEL
/// log ratio, taken jointly over ONE population, normalised by its own
/// geometric mean and inverted through the engine's WB model. Returns the
/// rounded Kelvin, the rounded tint, and the normalised `wanted` gains the
/// search was fitted to.
///
/// Three INDEPENDENT per-channel medians — what this replaces — are a ratio
/// of two marginals. On a bimodal frame the two marginals' halfway points
/// fall in different sub-populations, so their ratio is not the colour change
/// of any pixel in the frame; on the crate's own `flat_sky_to_cloud_deck`,
/// where no pixel changed its chromaticity at all, they read K 4400 /
/// tint +55.2. This reads one cloud of per-pixel colour CHANGES and takes its
/// centre.
///
/// The LOG form is written rather than the raw ratio because the median
/// commutes with a monotone map — the two are numerically identical — and the
/// log is what makes the estimator a LOCATION statistic on one cloud instead
/// of a ratio of two. The `1e-5` floor is this call site's own existing
/// convention, not a new knob.
///
/// The MEDIAN, not the mean, for exactly the reason it always was: a newly
/// generated cloud highlight must not own a frame-wide average. That comment
/// was not overruled by this batch — what the per-pixel form adds is that the
/// robustness now applies to the CHANGE each pixel underwent rather than to
/// two brightness distributions read apart from each other.
pub(super) fn atmosphere_wb_from_populations(
    sp: &[[f32; 3]],
    pair_tp: &[[f32; 3]],
    pair_w: &[f32],
    anchor: f32,
) -> (f32, f32, [f32; 3]) {
    let n = sp.len().min(pair_tp.len());
    let mut ratio = [1.0f32; 3];
    let mut samples: Vec<(f32, f32)> = Vec::with_capacity(n);
    for (ch, slot) in ratio.iter_mut().enumerate() {
        samples.clear();
        for i in 0..n {
            let w = pair_w.get(i).copied().unwrap_or(0.0).max(0.0);
            if w <= 0.0 {
                continue;
            }
            let source = render::srgb_to_linear(sp[i][ch]).max(1e-5);
            let target = render::srgb_to_linear(pair_tp[i][ch]).max(1e-5);
            samples.push((target.ln() - source.ln(), w));
        }
        *slot = weighted_median(&mut samples).exp();
    }
    let common = (ratio[0] * ratio[1] * ratio[2]).max(1e-12).powf(1.0 / 3.0);
    let wanted = ratio.map(|v| v / common);
    let (lo, hi) = (WB_SEARCH_K.0.ln(), WB_SEARCH_K.1.ln());
    let tint = ((1.0 - wanted[1]) / 0.20 * 100.0).clamp(-100.0, 100.0);
    let mut best = (anchor, f32::INFINITY);
    for i in 0..=400 {
        let k = (lo + (hi - lo) * i as f32 / 400.0).exp();
        let gains = render::wb_gains(anchor, k, tint);
        let err = gains
            .iter()
            .zip(wanted)
            .map(|(&g, want)| (g.max(1e-5) / want.max(1e-5)).log2().powi(2))
            .sum::<f32>();
        if err < best.1 {
            best = (k, err);
        }
    }
    ((best.0 / 50.0).round() * 50.0, round1(tint), wanted)
}

pub(super) fn weighted_mean(px: &[[f32; 3]], weights: &[f32], ch: usize) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut total = 0.0f32;
    for (i, p) in px.iter().enumerate() {
        let w = weights.get(i).copied().unwrap_or(0.0).max(0.0);
        sum += p[ch] * w;
        total += w;
    }
    (total > 1e-8).then_some(sum / total)
}

pub(super) fn weighted_mean_chroma(px: &[[f32; 3]], weights: &[f32]) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut total = 0.0f32;
    for (i, p) in px.iter().enumerate() {
        let w = weights.get(i).copied().unwrap_or(0.0).max(0.0);
        sum += (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) * w;
        total += w;
    }
    (total > 1e-8).then_some(sum / total)
}

impl Divergence {
    /// A hand-built matched reading, for fixtures that need one.
    /// PRODUCTION NEVER MANUFACTURES ONE: `structure_divergence` either
    /// measures a divergence or abstains, and a manufactured 0.0 on a
    /// production path is the defect v1.2.4 removed.
    #[cfg(test)]
    pub(super) fn matched() -> Self {
        Self { correlation: 1.0, energy_error: 0.0, d: 0.0 }
    }
}
