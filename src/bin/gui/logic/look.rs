//! The AI-look protocol, headless.
//!
//! A "look" is the one analysis the app has in flight at a time. Four VIEW
//! verbs start one — 「AI Analyze」, 「AI adjust」, Normal's one-click (a fixed
//! look or the reference photos) and the Portraits ✨ look — and they share ONE
//! protocol: a start under an epoch, a claim only the current epoch can win, and
//! a landing whose per-verb facts (line, remembered fine-tune, kept portrait
//! masks) come back as data. The panels draw the buttons; the words and the
//! decisions live here.
//!
//! WIRED IN THE NEXT SLICE: the panels still call the old starters; this
//! allowance goes away with them.
#![allow(dead_code)]
//!
//! This is the reflection layer, not a second business core: the model, the
//! request type, the presets and the WB/library rules are the library's; this
//! module maps a view verb onto them and back.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::advisor::Verdict;
use autoshade::library::{EmbeddingSwitch, DEFAULT_NAME, NORMAL_NAME};
use autoshade::pipeline::{Guide, GradeRequest, region_guidance};
use autoshade::rationale::Note;
use autoshade::recipe::{DirectionAdherence, EditRecipe, GradeStrength};
use autoshade::render::LensBlur;
use autoshade::recipe::LocalAdjustment;
use std::path::{Path, PathBuf};

/// What a one-click asks for: one of the fixed looks, or the photos in the
/// chosen reference folder. (It is a domain choice, so it lives here; the panel
/// only draws the buttons.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OneClick {
    Preset(&'static str),
    References,
}

/// The one-click policy's fixed numbers: a preset look is followed closely but
/// only by direction (style 0, no reference images, no deep thinking), while
/// the reference folder is followed as the library at full style and the photos
/// are shown to the model.
pub(crate) const PRESET_ADHERENCE: f32 = 0.8;
pub(crate) const REFERENCE_STYLE: f32 = 1.0;

/// The four fine-tune values as a one-click left them, so 「back to the
/// one-click result」 can put them back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FineTune {
    pub(crate) exposure_ev: f32,
    pub(crate) contrast: f32,
    pub(crate) temperature_k: Option<f32>,
    pub(crate) saturation: f32,
}

impl FineTune {
    pub(crate) fn of(r: &EditRecipe) -> Self {
        FineTune {
            exposure_ev: r.exposure_ev,
            contrast: r.contrast,
            temperature_k: r.temperature_k,
            saturation: r.saturation,
        }
    }

    pub(crate) fn apply(self, r: &mut EditRecipe) {
        r.exposure_ev = self.exposure_ev;
        r.contrast = self.contrast;
        r.temperature_k = self.temperature_k;
        r.saturation = self.saturation;
    }
}

/// Which view verb asked for the analysis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LookKind {
    Analyze,
    Adjust,
    OneClick(OneClick),
    Portrait,
}

/// The panel preferences a start reads, snapshotted on the UI thread so a
/// toggle flipped after the button cannot change what the call pays for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LookPrefs {
    pub(crate) guide: Guide,
    pub(crate) style: f32,
    pub(crate) send_reference_image: bool,
    pub(crate) strength: GradeStrength,
    pub(crate) think: bool,
    pub(crate) adherence: DirectionAdherence,
    pub(crate) embed: EmbeddingSwitch,
}

/// What the user asked for, as facts: which verb, the typed direction, a
/// box-selected region, and (for adjust / portrait) the payload the worker
/// needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LookIntent {
    pub(crate) kind: Option<LookKind>,
    pub(crate) typed: String,
    pub(crate) region: Option<[f32; 4]>,
    pub(crate) base: Option<EditRecipe>,
    pub(crate) pixels: Option<PathBuf>,
    /// A portrait look replaces the recipe whole; these come back on top of it.
    pub(crate) kept_masks: Vec<LocalAdjustment>,
    pub(crate) kept_blur: Option<LensBlur>,
}

impl LookIntent {
    pub(crate) fn of(kind: LookKind) -> Self {
        Self { kind: Some(kind), ..Default::default() }
    }
}

/// What a landing has to put back on the recipe, and what the verb remembers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Keep {
    #[default]
    None,
    OneClick,
    Portrait { masks: Vec<LocalAdjustment>, blur: Option<LensBlur> },
}

/// The analysis one start put in flight.
#[derive(Clone, Debug)]
pub(crate) struct PendingLook {
    pub(crate) kind: LookKind,
    pub(crate) photo: PathBuf,
    pub(crate) epoch: u64,
    pub(crate) keep: Keep,
}

/// Everything the worker needs, and nothing it must ask the UI for later.
#[derive(Clone, Debug)]
pub(crate) struct AnalysisJob {
    pub(crate) photo: PathBuf,
    pub(crate) pixels: Option<PathBuf>,
    pub(crate) guidance: Option<String>,
    pub(crate) base: Option<EditRecipe>,
    pub(crate) req: GradeRequest,
    /// Which library NAME the worker's scope opens (`library::with_library`).
    pub(crate) library: &'static str,
    pub(crate) epoch: u64,
}

/// A start's output: the job the facade runs, the pending look it stores, and
/// the opening effects (status line, the verb's working line).
pub(crate) struct Started {
    pub(crate) job: AnalysisJob,
    pub(crate) pending: PendingLook,
    pub(crate) effects: Vec<Effect>,
}

/// The presentation line a verb owns. The view maps these to its own line
/// enums; the words are rendered there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LookLine {
    Idle,
    Working,
    Done,
    /// No image key: the built-in look stood in.
    BuiltIn,
    Looked,
    Failed,
}

/// The state the protocol needs: at most one look in flight.
#[derive(Default)]
pub(crate) struct LookState {
    pub(crate) pending: Option<PendingLook>,
}

/// A result's claim on the pending look.
#[derive(Clone, Debug)]
pub(crate) struct Claim {
    pub(crate) kind: LookKind,
    pub(crate) photo: PathBuf,
    pub(crate) keep: Keep,
    /// The built-in look stood in for the model (a rationale note says so).
    pub(crate) built_in: bool,
}

/// Plan one start: guard, request, payload, pending facts and the opening
/// lines. `None` means the verb cannot start (no photo, or an analysis is
/// already in flight) — the same refusal the panels grey their buttons for.
pub(crate) fn plan(
    prefs: &LookPrefs,
    intent: LookIntent,
    photo: Option<&Path>,
    busy: bool,
    inflight: bool,
    epoch: u64,
) -> Option<Started> {
    let kind = intent.kind.clone()?;
    let photo = photo?.to_path_buf();
    if busy || inflight {
        return None;
    }
    let adjusting = matches!(kind, LookKind::Adjust);
    // A box-selected region is a LOCAL direction, so it guides by direction
    // whatever the panel's switch says; an adjust is guided by its request.
    let region_guided = intent.region.is_some();
    let guide = if adjusting || region_guided { Guide::Direction } else { prefs.guide };
    let req = GradeRequest {
        guide,
        style: prefs.style,
        send_reference_image: prefs.send_reference_image,
        strength: prefs.strength,
        think: prefs.think,
        adherence: prefs.adherence,
        embed: prefs.embed,
    };
    let typed = intent.typed.trim().to_string();
    let guidance = match intent.region {
        Some([l, t, r, b]) => Some(region_guidance(l, t, r, b, &typed)),
        None => (guide == Guide::Direction && !typed.is_empty()).then(|| typed.clone()),
    };
    let (req, guidance, library, keep, status, line) = match &kind {
        LookKind::Analyze => (
            req,
            guidance,
            DEFAULT_NAME,
            Keep::None,
            UiText::of(K_LOGIC_ANALYZING_WITH_THE_CONFIGURED_AI_SERVICE),
            None,
        ),
        LookKind::Adjust => (
            req,
            guidance,
            DEFAULT_NAME,
            Keep::None,
            UiText::of(K_LOGIC_ADJUSTING_THE_SELECTED_VARIANT_WITH_AI),
            None,
        ),
        LookKind::OneClick(OneClick::Preset(id)) => (
            GradeRequest {
                guide: Guide::Direction,
                style: 0.0,
                send_reference_image: false,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::new(PRESET_ADHERENCE),
                embed: EmbeddingSwitch::OFF,
            },
            autoshade::advisor::presets::preset_guidance(id).map(str::to_owned),
            NORMAL_NAME,
            Keep::OneClick,
            UiText::of(K_LOGIC_MAKING_THE_LOOK),
            Some(LookLine::Working),
        ),
        LookKind::OneClick(OneClick::References) => (
            GradeRequest {
                guide: Guide::Library,
                style: REFERENCE_STYLE,
                send_reference_image: true,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::default(),
                // The folder was just read by the same model, so its download
                // is done; the environment override still wins.
                embed: EmbeddingSwitch::resolve(None, true),
            },
            None,
            NORMAL_NAME,
            Keep::OneClick,
            UiText::of(K_LOGIC_MAKING_THE_LOOK),
            Some(LookLine::Working),
        ),
        LookKind::Portrait => (
            GradeRequest {
                guide: Guide::Direction,
                style: 0.0,
                send_reference_image: false,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::new(PRESET_ADHERENCE),
                embed: EmbeddingSwitch::OFF,
            },
            autoshade::advisor::presets::preset_guidance("portrait").map(str::to_owned),
            DEFAULT_NAME,
            Keep::Portrait {
                masks: intent.kept_masks.clone(),
                blur: intent.kept_blur.clone(),
            },
            UiText::of(K_LOGIC_MAKING_THE_PORTRAIT_LOOK),
            Some(LookLine::Working),
        ),
    };
    let job = AnalysisJob {
        photo: photo.clone(),
        pixels: intent.pixels,
        guidance,
        base: intent.base,
        req,
        library,
        epoch,
    };
    let mut effects = vec![Effect::Status(status)];
    if let Some(line) = line {
        effects.push(Effect::LookLine { kind: kind.clone(), line });
    }
    Some(Started {
        job,
        pending: PendingLook { kind, photo, epoch, keep },
        effects,
    })
}

/// Claim a finished analysis for the pending look, if it is this one's and it
/// is still current. A result under an old epoch is DISCARDED: its Ok arm
/// would otherwise write a develop over the photo the user has moved on to.
pub(crate) fn claim(
    state: &mut LookState,
    epoch: u64,
    gen_epoch: u64,
    res: &anyhow::Result<(EditRecipe, Verdict, Vec<Note>)>,
) -> Option<Claim> {
    let pending = state.pending.as_ref()?;
    if pending.epoch != epoch {
        return None;
    }
    let pending = state.pending.take()?;
    if epoch != gen_epoch {
        return None;
    }
    let built_in = res.as_ref().is_ok_and(|(_, _, notes)| {
        notes.iter().any(|n| {
            matches!(
                n.key,
                autoshade::rationale::keys::HEURISTIC_NO_KEY
                    | autoshade::rationale::keys::HEURISTIC_UNAVAILABLE
            )
        })
    });
    Some(Claim { kind: pending.kind, photo: pending.photo, keep: pending.keep, built_in })
}

/// Before the landed recipe is written: put back what a fresh look replaced.
pub(crate) fn restore(claim: &Claim, recipe: &mut EditRecipe) {
    if let Keep::Portrait { masks, blur } = &claim.keep {
        autoshade::face::restore_portrait_masks(recipe, masks.clone(), blur.clone());
    }
}

/// After the landed recipe is written: the verb's own line and what it
/// remembers. `ok` is the shared landing's outcome.
pub(crate) fn landed(claim: &Claim, recipe: &EditRecipe, ok: bool) -> Vec<Effect> {
    let mut effects = Vec::new();
    match &claim.kind {
        LookKind::Analyze | LookKind::Adjust => {}
        LookKind::OneClick(_) => {
            if ok {
                effects.push(Effect::LookLine {
                    kind: claim.kind.clone(),
                    line: if claim.built_in { LookLine::BuiltIn } else { LookLine::Done },
                });
                effects.push(Effect::RememberOneClick {
                    photo: claim.photo.clone(),
                    fine: FineTune::of(recipe),
                });
            } else {
                effects.push(Effect::LookLine { kind: claim.kind.clone(), line: LookLine::Failed });
            }
        }
        LookKind::Portrait => {
            effects.push(Effect::LookLine {
                kind: claim.kind.clone(),
                line: if ok { LookLine::Looked } else { LookLine::Failed },
            });
        }
    }
    effects
}

/// 「Match my reference photos…」: read the picked folder into Normal's own
/// library, then hand over to the reference look. Refused while another build
/// or analysis runs, or with no photo open.
pub(crate) fn references_start(
    photo: Option<&Path>,
    dir: PathBuf,
    busy: bool,
    inflight: bool,
    building: bool,
) -> Vec<Effect> {
    if busy || inflight || building {
        return Vec::new();
    }
    let Some(photo) = photo else { return Vec::new() };
    vec![
        Effect::Status(UiText::of(K_LOGIC_READING_YOUR_REFERENCE_PHOTOS)),
        Effect::LookLine { kind: LookKind::OneClick(OneClick::References), line: LookLine::Idle },
        Effect::ReferenceBuild { photo: photo.to_path_buf(), dir },
    ]
}

/// The folder build's landing: a built library hands straight over to the
/// reference look for the photo it was started for; anything else is said on
/// the line (and, for a read failure, as an error toast) instead of dropped.
pub(crate) fn references_built(
    res: &Result<usize, String>,
    built_photo: Option<&Path>,
    current_photo: Option<&Path>,
    inflight: bool,
) -> Vec<Effect> {
    let kind = LookKind::OneClick(OneClick::References);
    match res {
        Ok(_) if built_photo.is_some() && built_photo == current_photo && !inflight => {
            vec![Effect::StartLook(Box::new(LookIntent::of(kind)))]
        }
        Ok(_) => vec![
            // Status FIRST: the failure line shows the status the landing wrote.
            Effect::Status(UiText::of(
                K_LOGIC_YOUR_REFERENCE_PHOTOS_ARE_READ_BUT_THE_LOOK_COUL,
            )),
            Effect::LookLine { kind, line: LookLine::Failed },
        ],
        Err(err) => {
            let text = UiText::with(K_LOGIC_COULD_NOT_READ_YOUR_REFERENCE_PHOTOS, &[("err", err)]);
            vec![
                Effect::Status(text.clone()),
                Effect::LookLine { kind, line: LookLine::Failed },
                Effect::ToastError(text),
            ]
        }
    }
}

/// The ✕ or a cancelled start lets go of the pending look; a late result then
/// has no claim to win.
pub(crate) fn cancel(state: &mut LookState) {
    state.pending = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::advisor::{Decision, Verdict};
    use autoshade::rationale::keys;

    fn prefs() -> LookPrefs {
        LookPrefs {
            guide: Guide::Library,
            style: 0.4,
            send_reference_image: true,
            strength: GradeStrength::new(0.7),
            think: true,
            adherence: DirectionAdherence::new(0.5),
            embed: EmbeddingSwitch::resolve(None, true),
        }
    }

    fn ok_result() -> anyhow::Result<(EditRecipe, Verdict, Vec<Note>)> {
        Ok((
            EditRecipe::default(),
            Verdict { decision: Decision::Accept, reasons: vec![], revised_hint: None },
            Vec::new(),
        ))
    }

    fn heuristic_result() -> anyhow::Result<(EditRecipe, Verdict, Vec<Note>)> {
        Ok((
            EditRecipe::default(),
            Verdict { decision: Decision::Accept, reasons: vec![], revised_hint: None },
            vec![Note { key: keys::HEURISTIC_NO_KEY, args: Vec::new() }],
        ))
    }

    /// The fixed one-click look: direction-guided, style 0, no reference
    /// images, no thinking, its preset text, Normal's library, and the line
    /// that says work started.
    #[test]
    fn a_preset_one_click_plans_its_fixed_policy() {
        let started = plan(
            &prefs(),
            LookIntent::of(LookKind::OneClick(OneClick::Preset("natural"))),
            Some(Path::new("photo.arw")),
            false,
            false,
            7,
        )
        .expect("a free worker starts");
        assert_eq!(started.pending.epoch, 7);
        assert_eq!(started.pending.keep, Keep::OneClick);
        assert_eq!(started.job.library, NORMAL_NAME);
        assert_eq!(started.job.req.guide, Guide::Direction);
        assert_eq!(started.job.req.style, 0.0);
        assert!(!started.job.req.send_reference_image);
        assert!(!started.job.req.think, "a one-click never goes deep");
        assert_eq!(started.job.req.strength, prefs().strength);
        assert_eq!(started.job.req.adherence, DirectionAdherence::new(PRESET_ADHERENCE));
        assert_eq!(started.job.req.embed, EmbeddingSwitch::OFF);
        assert_eq!(started.job.guidance, autoshade::advisor::presets::preset_guidance("natural").map(str::to_owned));
        assert!(matches!(started.effects.as_slice(), [Effect::Status(_), Effect::LookLine { line: LookLine::Working, .. }]));
    }

    /// The reference-folder one-click is the library at full style, with the
    /// photos sent along and Normal's own library.
    #[test]
    fn a_reference_one_click_plans_the_library_policy() {
        let started = plan(
            &prefs(),
            LookIntent::of(LookKind::OneClick(OneClick::References)),
            Some(Path::new("photo.arw")),
            false,
            false,
            1,
        )
        .expect("a free worker starts");
        assert_eq!(started.job.library, NORMAL_NAME);
        assert_eq!(started.job.req.guide, Guide::Library);
        assert_eq!(started.job.req.style, REFERENCE_STYLE);
        assert!(started.job.req.send_reference_image);
        assert_eq!(started.job.guidance, None);
    }

    /// The Portraits look is the portrait preset through the DEFAULT library,
    /// and it carries the masks/blur the landed recipe has to keep.
    #[test]
    fn a_portrait_look_carries_the_masks_it_must_keep() {
        let mut intent = LookIntent::of(LookKind::Portrait);
        intent.kept_masks = vec![autoshade::face::portrait_adjustment(
            autoshade::recipe::MaskRole::Face,
            "mask-face.png".into(),
        )];
        intent.kept_blur = Some(LensBlur { depth_map: "depth.png".into(), ..Default::default() });
        let started = plan(&prefs(), intent, Some(Path::new("photo.arw")), false, false, 3)
            .expect("a free worker starts");
        assert_eq!(started.job.library, DEFAULT_NAME);
        assert_eq!(started.job.req.guide, Guide::Direction);
        assert_eq!(started.job.guidance, autoshade::advisor::presets::preset_guidance("portrait").map(str::to_owned));
        assert!(matches!(started.pending.keep, Keep::Portrait { .. }));
    }

    /// Analyze keeps the panel's preferences; a typed direction only steers a
    /// direction-guided develop, and a selected region folds into the prompt
    /// (forcing the direction guide) with the neutral fallback when the box is
    /// empty.
    #[test]
    fn an_analyze_snapshots_the_panel_and_folds_a_region() {
        let started = plan(&prefs(), LookIntent::of(LookKind::Analyze), Some(Path::new("p.arw")), false, false, 4)
            .expect("a free worker starts");
        assert_eq!(started.job.library, DEFAULT_NAME);
        assert_eq!(started.job.req, GradeRequest {
            guide: Guide::Library,
            style: 0.4,
            send_reference_image: true,
            strength: GradeStrength::new(0.7),
            think: true,
            adherence: DirectionAdherence::new(0.5),
            embed: EmbeddingSwitch::resolve(None, true),
        });
        assert_eq!(started.job.guidance, None, "library-guided: typed words are not sent");

        let mut typed = LookIntent::of(LookKind::Analyze);
        typed.typed = "  moodier  ".into();
        let mut library = prefs();
        library.guide = Guide::Library;
        assert_eq!(plan(&library, typed.clone(), Some(Path::new("p.arw")), false, false, 4).unwrap().job.guidance, None,
            "typed words under a library guide are not a direction");
        let mut direction = prefs();
        direction.guide = Guide::Direction;
        assert_eq!(plan(&direction, typed, Some(Path::new("p.arw")), false, false, 4).unwrap().job.guidance.as_deref(),
            Some("moodier"));

        let mut region = LookIntent::of(LookKind::Analyze);
        region.region = Some([0.0, 0.0, 0.5, 0.5]);
        let started = plan(&library, region, Some(Path::new("p.arw")), false, false, 4).unwrap();
        assert_eq!(started.job.req.guide, Guide::Direction, "a box is a local direction");
        let guidance = started.job.guidance.unwrap();
        assert!(guidance.contains("left=0.000 top=0.000 right=0.500 bottom=0.500"), "{guidance}");
        assert!(guidance.ends_with("Direction: make a tasteful local improvement"), "{guidance}");
    }

    /// Adjust is guided by its request, carries the current recipe and (when
    /// the card sits on generated pixels) their raster.
    #[test]
    fn an_adjust_carries_the_base_and_the_generated_raster() {
        let mut intent = LookIntent::of(LookKind::Adjust);
        intent.typed = "brighter".into();
        intent.base = Some(EditRecipe { exposure_ev: 0.5, ..Default::default() });
        intent.pixels = Some(PathBuf::from("card.png"));
        let started = plan(&prefs(), intent, Some(Path::new("p.arw")), false, false, 2)
            .expect("a free worker starts");
        assert_eq!(started.job.req.guide, Guide::Direction);
        assert_eq!(started.job.req.style, 0.4, "the panel's style still travels");
        assert_eq!(started.job.guidance.as_deref(), Some("brighter"));
        assert_eq!(started.job.base.as_ref().map(|b| b.exposure_ev), Some(0.5));
        assert_eq!(started.job.pixels.as_deref(), Some(Path::new("card.png")));
    }

    /// Nothing starts without a photo, or while another analysis is in flight.
    #[test]
    fn a_start_needs_a_photo_and_a_free_worker() {
        let intent = LookIntent::of(LookKind::Analyze);
        assert!(plan(&prefs(), intent.clone(), None, false, false, 0).is_none());
        assert!(plan(&prefs(), intent.clone(), Some(Path::new("p.arw")), true, false, 0).is_none());
        assert!(plan(&prefs(), intent, Some(Path::new("p.arw")), false, true, 0).is_none());
    }

    /// Only the pending look's own epoch claims, and only while the epoch is
    /// current; the claim takes the pending look with it (one landing each).
    #[test]
    fn only_the_pending_looks_current_epoch_claims() {
        let mut state = LookState::default();
        let started = plan(
            &prefs(),
            LookIntent::of(LookKind::OneClick(OneClick::Preset("film"))),
            Some(Path::new("p.arw")),
            false,
            false,
            9,
        )
        .unwrap();
        state.pending = Some(started.pending);
        assert!(claim(&mut state, 8, 9, &ok_result()).is_none(), "another epoch's result");
        assert!(state.pending.is_some(), "and it did not consume the pending look");
        let won = claim(&mut state, 9, 9, &ok_result()).expect("its own result claims");
        assert_eq!(won.photo, PathBuf::from("p.arw"));
        assert!(!won.built_in);
        assert!(state.pending.is_none());
        assert!(claim(&mut state, 9, 9, &ok_result()).is_none(), "a second landing has nothing to claim");
    }

    /// A cancelled analysis (its epoch bumped) claims nothing even though its
    /// result arrives under the epoch it was started with.
    #[test]
    fn a_cancelled_result_is_discarded() {
        let mut state = LookState::default();
        let started = plan(
            &prefs(),
            LookIntent::of(LookKind::Portrait),
            Some(Path::new("p.arw")),
            false,
            false,
            5,
        )
        .unwrap();
        state.pending = Some(started.pending);
        assert!(claim(&mut state, 5, 6, &ok_result()).is_none());
        assert!(state.pending.is_none(), "the stale claim lets go of the pending look");
    }

    /// The heuristic fallback is read from the rationale notes the library
    /// produced: it is the ONLY thing that turns a landed one-click into
    /// 「built-in」.
    #[test]
    fn the_built_in_look_is_read_from_the_rationale_notes() {
        let mut state = LookState::default();
        let started = plan(
            &prefs(),
            LookIntent::of(LookKind::OneClick(OneClick::Preset("natural"))),
            Some(Path::new("p.arw")),
            false,
            false,
            1,
        )
        .unwrap();
        state.pending = Some(started.pending);
        let claim = claim(&mut state, 1, 1, &heuristic_result()).expect("claimed");
        assert!(claim.built_in);
        let effects = landed(&claim, &EditRecipe::default(), true);
        assert!(matches!(effects[0], Effect::LookLine { line: LookLine::BuiltIn, .. }), "{effects:?}");
    }

    /// A portrait landing puts its kept masks and blur back BEFORE the write;
    /// the one-click landing remembers the fine-tune it landed on.
    #[test]
    fn a_landing_restores_what_the_look_replaced_and_remembers_what_it_set() {
        let mut recipe = EditRecipe { exposure_ev: 0.2, contrast: 12.0, saturation: 3.0, ..Default::default() };
        let claim = Claim {
            kind: LookKind::Portrait,
            photo: PathBuf::from("p.arw"),
            keep: Keep::Portrait {
                masks: vec![autoshade::face::portrait_adjustment(
                    autoshade::recipe::MaskRole::Face,
                    "mask-face.png".into(),
                )],
                blur: Some(LensBlur { depth_map: "depth.png".into(), ..Default::default() }),
            },
            built_in: false,
        };
        restore(&claim, &mut recipe);
        assert!(recipe.masks.iter().any(|m| m.role == autoshade::recipe::MaskRole::Face));
        assert!(recipe.lens_blur.is_some());
        let effects = landed(&claim, &recipe, true);
        assert!(matches!(effects.as_slice(), [Effect::LookLine { line: LookLine::Looked, .. }]));
        assert!(matches!(landed(&claim, &recipe, false).as_slice(), [Effect::LookLine { line: LookLine::Failed, .. }]));

        let claim = Claim {
            kind: LookKind::OneClick(OneClick::Preset("film")),
            photo: PathBuf::from("p.arw"),
            keep: Keep::OneClick,
            built_in: false,
        };
        let effects = landed(&claim, &recipe, true);
        assert!(matches!(effects[0], Effect::LookLine { line: LookLine::Done, .. }));
        match &effects[1] {
            Effect::RememberOneClick { fine, photo } => {
                assert_eq!(fine, &FineTune::of(&recipe));
                assert_eq!(photo, &PathBuf::from("p.arw"));
            }
            other => panic!("expected the remembered fine-tune, got {other:?}"),
        }
    }

    /// Cancelling lets go of the pending look; an analyze/adjust landing has
    /// no line of its own (the shared landing owns those words).
    #[test]
    fn cancelling_clears_the_pending_look() {
        let mut state = LookState::default();
        let started = plan(&prefs(), LookIntent::of(LookKind::Analyze), Some(Path::new("p.arw")), false, false, 0)
            .unwrap();
        state.pending = Some(started.pending);
        cancel(&mut state);
        assert!(state.pending.is_none());
        let claim = Claim { kind: LookKind::Analyze, photo: PathBuf::from("p"), keep: Keep::None, built_in: false };
        assert!(landed(&claim, &EditRecipe::default(), true).is_empty());
    }

    fn photo() -> PathBuf {
        PathBuf::from("photo.arw")
    }

    /// The reference folder build: refused while work runs, otherwise status +
    /// the folder job for the photo that was open when it was picked.
    #[test]
    fn the_reference_build_starts_for_the_photo_it_was_picked_on() {
        let dir = PathBuf::from("refs");
        assert!(references_start(None, dir.clone(), false, false, false).is_empty(), "no photo, no build");
        assert!(references_start(Some(Path::new("p.arw")), dir.clone(), true, false, false).is_empty());
        assert!(references_start(Some(Path::new("p.arw")), dir.clone(), false, true, false).is_empty());
        assert!(references_start(Some(Path::new("p.arw")), dir.clone(), false, false, true).is_empty());
        let effects = references_start(Some(Path::new("p.arw")), dir.clone(), false, false, false);
        assert!(matches!(effects[0], Effect::Status(_)));
        assert!(matches!(effects[1], Effect::LookLine { line: LookLine::Idle, .. }));
        match &effects[2] {
            Effect::ReferenceBuild { photo, dir: d } => {
                assert_eq!(photo, &PathBuf::from("p.arw"));
                assert_eq!(d, &dir);
            }
            other => panic!("expected the build job, got {other:?}"),
        }
    }

    /// A built library hands over ONLY to the photo it was started for; a
    /// different open photo, work in flight, or a failed read is said instead
    /// of silently dropping the look.
    #[test]
    fn a_built_library_hands_over_only_to_the_photo_it_was_read_for() {
        let built = photo();
        let hand_off = references_built(&Ok(3), Some(&built), Some(Path::new("photo.arw")), false);
        assert!(matches!(hand_off.as_slice(), [Effect::StartLook(_)]), "{hand_off:?}");

        let elsewhere = references_built(&Ok(3), Some(&built), Some(Path::new("other.arw")), false);
        assert!(matches!(elsewhere[0], Effect::Status(_)));
        assert!(matches!(elsewhere[1], Effect::LookLine { line: LookLine::Failed, .. }));

        let busy = references_built(&Ok(3), Some(&built), Some(Path::new("photo.arw")), true);
        assert!(matches!(busy[1], Effect::LookLine { line: LookLine::Failed, .. }));

        let failed = references_built(&Err("no photos in that folder".into()), Some(&built), Some(Path::new("photo.arw")), false);
        assert!(matches!(failed[0], Effect::Status(_)));
        assert!(matches!(failed[1], Effect::LookLine { line: LookLine::Failed, .. }));
        assert!(matches!(failed[2], Effect::ToastError(_)));
    }
}
