//! Retouch's business: the worker-side starts (fill / adjust / heal / AI
//! denoise / stack / clone / reimagine / style prompt) and the card-picture
//! develop they share.
//!
//! The panel draws and emits verbs; the app layer here decides, builds the
//! requests and spawns the workers. The landing is `workers::on_retouched`.

use crate::*;

/// The picture the canvas shows, before straighten and crop. Shared by fill
/// and adjust so the model sees the card's live recipe at the source's film
/// edge; downstream crop/straighten stay on the view.
///
/// In the frame the card's base is in (v1.17.0): lens-corrected under Camera
/// Raw's law, so the model is shown the corrected picture the canvas shows.
/// The brush mask paired with it is stored in the SENSOR frame and is mapped
/// onto this one by the fill ([`autoshade::render::Master::rgba_to_frame`]),
/// and the layer or card it makes is stamped with the frame.
fn developed_card_pixels(
    path: &std::path::Path,
    recipe: &EditRecipe,
    full_res: bool,
) -> anyhow::Result<autoshade::render::Master> {
    let raw = autoshade::decode::is_raw(path);
    let frame = autoshade::render::SourceFrame::of(path)?;
    frame.check(recipe)?;
    let part = autoshade::render::BasePart::of(recipe);
    // The card's own base (v1.17.0): the profile and Look table its recipe
    // names are in the pixels the preview develop below is handed; the
    // master keeps `part`.
    let base = autoshade::render::source_pixels_for(
        path,
        &autoshade::render::BasePart::of(recipe),
        (raw && !full_res).then_some(2048),
    )?;
    let geom = autoshade::render::geometry_profile(recipe);
    let corrected = frame.corrected();
    let image = autoshade::render::develop_preview_framed(
        &base,
        recipe,
        &autoshade::diag::pixels(),
        autoshade::render::MaskFrame::sampled(&geom, corrected),
        autoshade::decode::film_short_edge(path),
        raw,
    );
    autoshade::render::Master::of(image, &frame, &part)
}

impl Commands<'_> {


pub(crate) fn start_fill(&mut self) {
        // The photo names the artifact (as a reimagine's is named); the
        // card's OWN pixel source (a ✨/✎ card → its raster, ▣/◭ → the
        // negative) is what its recipe develops in the worker below.
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let Some(path) = self.editor.active_source_path() else { return };
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        // Blank = remove: the library substitutes its removal instruction
        // (`generative::fill_prompt`), so an empty box is a verb, not an
        // error — the mask already says what should go.
        let prompt = self.workspace.genai().fill_prompt.trim().to_string();
        // No painted area: the words alone say what to change, and the model
        // finds where (`generative::auto_fill_onto`; user request 2026-10-12,
        // 「跟原先的涂抹后ai生成一个道理，只不过省去了涂抹这一步骤」). Only an empty
        // box over an empty canvas has nothing to go on.
        let mask_png = self.workspace.export_area_png(BrushOwner::Fill);
        if mask_png.is_none() && prompt.is_empty() {
            self.workspace.set_status(tr(lang, K_ACTIONS_WRITE_WHAT_TO_CHANGE_E_G_REMOVE_THE_WIRES_ON_THE).into());
            return;
        }
        let Some(out) = unique_out(&src, "fill") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = if self.workspace.genai().fill_fullres {
            tr(lang, K_ACTIONS_GENERATIVE_FILL_FULL_RES_RENDER_SLOW_MINUTES).into()
        } else {
            tr(lang, K_ACTIONS_GENERATIVE_FILL_VIA_GPT_IMAGE_HIGH_QUALITY_CAN_R).into()
        };
        let quality = ["high", "medium", "low"][self.workspace.genai().fill_quality.min(2)].to_string();
        let full_res = self.workspace.genai().fill_fullres;
        // The card's live develop, captured at the click like every other
        // input the worker reads; the source kind decides the working size.
        let recipe = self.editor.doc().recipe.clone();
        let edge = self.editor.canvas_edge(self.workspace); // show at the CANVAS's res (canvas_edge)
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        let (epoch, flag) = self.workspace.arm_cancel();
        let ptx = self.services.tx.clone();
        self.services.spawn_worker(
            move || {
                // Progress heartbeats → status bar; the cancel flag stops the
                // stream between events. Installed on THIS worker thread only.
                autoshade::generative::set_worker_hooks(Some(autoshade::generative::WorkerHooks {
                    progress: Box::new(move |m| {
                        let _ = ptx.send(Msg::Progress(epoch, m));
                    }),
                    cancel: flag,
                }));
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                let res = (|| -> RetouchDone {
                    let cfg = autoshade::config::Config::load();
                    let base = || developed_card_pixels(&path, &recipe, full_res);
                    let note = if let Some(mask_png) = mask_png {
                        let mask_tmp = gui_tmp_png("fill");
                        std::fs::write(&mask_tmp, &mask_png)?;
                        let r = autoshade::generative::retouch_onto(
                            &cfg,
                            &src,
                            base,
                            "this card's look",
                            &autoshade::generative::FillJob {
                                mask_path: &mask_tmp,
                                prompt: &prompt,
                                quality: &quality,
                                out: &out,
                                layer: true,
                            },
                        );
                        let _ = std::fs::remove_file(&mask_tmp);
                        r?;
                        RetouchNote::Filled(out.clone())
                    } else {
                        let report = autoshade::generative::auto_fill_onto(
                            &cfg,
                            &src,
                            base,
                            "this card's look",
                            &autoshade::generative::AutoFillJob { prompt: &prompt, quality: &quality, out: &out },
                        )?;
                        RetouchNote::FoundFilled { out: out.clone(), coverage: report.coverage }
                    };
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    // Layer (2026-10-01, user decision): the patch joins this
                    // card's develop (`RetouchKind::Layer`); through v1.6.7 it
                    // landed as a new ✨ card. FACTS, not prose (L12#4): the
                    // landing renders these with the language live when the
                    // result lands.
                    Ok((img, note, out, RetouchKind::Layer))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_adjust(&mut self) {
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let Some(path) = self.editor.active_source_path() else { return };
        if !self.editor.can_adjust(self.workspace, ) || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang;
        let prompt = self.workspace.genai().adjust_prompt.trim().to_string();
        let mask_png = self.workspace.export_area_png(BrushOwner::Adjust);
        let region = mask_png.is_some();
        let Some(out) = unique_out(&src, "adjust") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        self.workspace.set_busy(true);
        self.workspace.set_status(tr(lang, K_ACTIONS_ADJUSTING_GENERATED_IMAGE_VIA_GPT_IMAGE_HIGH_QUA).into());
        let quality = ["high", "medium", "low"][self.workspace.genai().adjust_quality.min(2)].to_string();
        let recipe = self.editor.doc().recipe.clone();
        let edge = self.editor.canvas_edge(self.workspace);
        let out_claim = out.clone();
        let out_panic = out.clone();
        let (epoch, flag) = self.workspace.arm_cancel();
        let ptx = self.services.tx.clone();
        self.services.spawn_worker(
            move || {
                autoshade::generative::set_worker_hooks(Some(autoshade::generative::WorkerHooks {
                    progress: Box::new(move |m| {
                        let _ = ptx.send(Msg::Progress(epoch, m));
                    }),
                    cancel: flag,
                }));
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path)));
                let res = (|| -> RetouchDone {
                    let cfg = autoshade::config::Config::load();
                    let base = || developed_card_pixels(&path, &recipe, false);
                    let divergence = if let Some(mask_png) = mask_png {
                        let mask_tmp = gui_tmp_png("adjust");
                        std::fs::write(&mask_tmp, &mask_png)?;
                        let r = autoshade::generative::retouch_onto(
                            &cfg, &src, base, "this card's look",
                            &autoshade::generative::FillJob {
                                mask_path: &mask_tmp, prompt: &prompt, quality: &quality, out: &out, layer: false,
                            },
                        );
                        let _ = std::fs::remove_file(&mask_tmp);
                        r?;
                        None
                    } else {
                        let report = autoshade::generative::adjust_onto(
                            &cfg, &src, base, "this card's look",
                            &autoshade::generative::AdjustJob {
                                prompt: &prompt, quality: &quality, out: &out,
                            },
                        )?;
                        Some(report.divergence)
                    };
                    // baked-by-construction: the ./out PNG master this adjust just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    Ok((img, RetouchNote::Adjusted { out: out.clone(), region, divergence }, out, RetouchKind::NewGenerated))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // A panic skips the normal tail; return its empty claim too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_heal(&mut self, use_mask: bool, limit: Option<PathBuf>) {
        // Heal the ACTIVE variant's pixels (Generated → its origin PNG).
        let Some(path) = self.editor.active_source_path() else { return };
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        let mask_png = if use_mask {
            match self.workspace.export_area_png(BrushOwner::Heal) {
                Some(b) => Some(b),
                None => {
                    self.workspace.set_status(tr(lang, K_ACTIONS_PAINT_THE_SPOTS_FIRST_PAINT_AREA_IN_THIS_SECTION).into());
                    return;
                }
            }
        } else {
            None
        };
        let Some(out) = unique_out(&path, "heal") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = if use_mask {
            tr(lang, K_ACTIONS_HEALING_PAINTED_AREA).into()
        } else if limit.is_some() {
            tr(lang, K_ACTIONS_REMOVING_BLEMISHES_ON_THE_FACE_10_30S).into()
        } else {
            tr(lang, K_ACTIONS_AI_HEALING_10_30S).into()
        };
        #[cfg(test)]
        {
            self.workspace.portrait_mut().heal_asked = Some((use_mask, limit.clone()));
        }
        #[cfg(test)]
        let runner = self.workspace.portrait().heal_runner;
        #[cfg(not(test))]
        let runner: Option<crate::panels::portrait::HealRunner> = None;
        let in_face = limit.is_some();
        let full_res = self.workspace.genai().heal_fullres;
        let edge = self.editor.canvas_edge(self.workspace); // bake at the CANVAS's res (canvas_edge)
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        // The master is baked over the canvas's own base (v1.17.0): its
        // recipe's camera profile and Look table, which the card's preview
        // develop cannot add afterwards.
        let part = autoshade::render::BasePart::of(&self.editor.doc().recipe);
        let (epoch, _flag) = self.workspace.arm_cancel(); // local compute: Cancel = abandon (epoch discard)
        self.services.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                let res = (|| -> RetouchDone {
                    let mask_tmp = match mask_png {
                        Some(bytes) => {
                            let t = gui_tmp_png("heal");
                            std::fs::write(&t, &bytes)?;
                            Some(t)
                        }
                        None => None,
                    };
                    // Test seam (`PortraitState::heal_runner`): a headless
                    // test stands in for the billed detect + heal.
                    let rep = match runner {
                        Some(run) => run(&path, limit.as_deref(), &out),
                        None => autoshade::retouch::heal(&path, &part, mask_tmp.as_deref(), !use_mask, full_res, limit.as_deref(), &out),
                    };
                    if let Some(t) = &mask_tmp {
                        let _ = std::fs::remove_file(t);
                    }
                    let rep = rep?;
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    // The report's rationale (AI-detect fallback, budget
                    // skips) must reach the status — stderr is invisible in
                    // the windowed GUI (L08 rule). Split per the L12#2B
                    // suffix contract: typed notes render localized at the
                    // landing; on a mismatch the whole string rides as prose.
                    let det = autoshade::rationale::render_en(&rep.notes);
                    let (ai_prose, notes) = match rep.rationale.strip_suffix(det.as_str()) {
                        Some(p) => (p.to_string(), rep.notes),
                        None => (rep.rationale.clone(), Vec::new()),
                    };
                    // InPlace: bake into the active variant's base + repoint origin.
                    Ok((
                        img,
                        RetouchNote::Healed { n: rep.spots, out: out.clone(), ai_prose, notes, in_face },
                        out,
                        RetouchKind::InPlace,
                    ))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_ai_denoise(&mut self) {
        // Denoise the ACTIVE variant's pixels (a Generated variant → its
        // origin PNG), same source rule as heal/clone.
        let Some(path) = self.editor.active_source_path() else { return };
        // R27 gate (`refuse_pixel_work_on_a_turned_photo`): the ◈ master
        // would land in the EXIF frame, and a baked card cannot be turned.
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        let Some(out) = unique_out(&path, "denoise") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        self.workspace.set_busy(true);
        self.workspace.set_status(tr(lang, K_ACTIONS_AI_DENOISE_FULL_FRAME_GPU_SIDECAR_CAN_TAKE_MINUT).into());
        // Which model the source takes — a FACT for the landing line (L12#4),
        // decided by the same rule the library applies (`decode::is_raw`).
        let on_mosaic = autoshade::decode::is_raw(&path);
        // This fold's OWN dial (the Export fold's 「on export」 has its own):
        // captured at the click, like every other input the worker reads.
        let strength = self.editor.selected_denoise_strength(self.workspace, false);
        #[cfg(test)]
        {
            self.seams.denoise_strength_asked = Some((false, strength));
        }
        let edge = self.editor.canvas_edge(self.workspace); // show at the CANVAS's res (canvas_edge)
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        // The master is baked over the canvas's own base (v1.17.0): its
        // recipe's camera profile and Look table, which the card's preview
        // develop cannot add afterwards.
        let part = autoshade::render::BasePart::of(&self.editor.doc().recipe);
        let (epoch, _flag) = self.workspace.arm_cancel(); // sidecar compute: Cancel = abandon (epoch discard)
        self.services.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                let res = (|| -> RetouchDone {
                    let cfg = autoshade::config::Config::load();
                    let opts =
                        autoshade::denoise::DenoiseOpts::from_config(&cfg, None, strength);
                    autoshade::denoise::denoise_active(&opts, &path, &part, &out)?;
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    // NewDenoised: a new ◈ card hangs off the master.
                    Ok((img, RetouchNote::Denoised { out: out.clone(), on_mosaic }, out, RetouchKind::NewDenoised))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_stack(&mut self, extra: Vec<PathBuf>) {
        let Some(path) = self.editor.active_source_path() else { return };
        // R27 gate (`refuse_pixel_work_on_a_turned_photo`): the ▦ master
        // would land in the EXIF frame, and a baked card cannot be turned.
        if self.workspace.busy() || extra.is_empty() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        let Some(out) = unique_out(&path, "stack") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        // This fold's OWN dials, captured at the click like every other input
        // a worker reads.
        let (kind, tripod) = (self.workspace.tools().stack_kind, self.workspace.tools().stack_tripod);
        let mut inputs = vec![path.clone()];
        inputs.extend(extra);
        let frames = inputs.len();
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = trf(
            lang,
            K_ACTIONS_STACKING_FRAMES_LOCAL_PIXEL_COMPUTE_MINUTES_AT_F,
            &[("n", &frames.to_string())],
        );
        let edge = self.editor.canvas_edge(self.workspace); // show at the CANVAS's res (canvas_edge)
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        // The master is baked over the canvas's own base (v1.17.0): its
        // recipe's camera profile and Look table, which the card's preview
        // develop cannot add afterwards.
        let part = autoshade::render::BasePart::of(&self.editor.doc().recipe);
        let (epoch, _flag) = self.workspace.arm_cancel(); // local compute: Cancel = abandon (epoch discard)
        self.services.spawn_worker(
            move || {
                // Every frame is held decoded at once, so the commit is the
                // one-frame estimate times the number of them.
                let each = crate::budget::estimate_mb(Some(&path));
                let _mem = crate::budget::heavy_permit(each.saturating_mul(frames as u64));
                let res = (|| -> RetouchDone {
                    let opts = autoshade::stack::merge::MergeOptions {
                        kind,
                        // A tripod's frames are registered already, and on
                        // frames that really are registered the aligner has
                        // nothing to find and costs a little (`MergeOptions`).
                        align: (!tripod).then_some(autoshade::stack::align::AlignParams::DEFAULT),
                    };
                    let done = autoshade::stack::stack_files(&inputs, &part, &opts, None, &out)?;
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    let m = &done.merged;
                    Ok((
                        img,
                        RetouchNote::Stacked {
                            out: out.clone(),
                            kind,
                            frames,
                            // The WORST frame's travel: one badly-registered
                            // frame is what makes a stack worth looking at
                            // before trusting it, and an average would hide it.
                            travel: m.travel.iter().copied().fold(0.0f32, f32::max),
                            uncovered: m.uncovered,
                            headroom_ev: m.headroom_ev,
                        },
                        out,
                        RetouchKind::NewStacked,
                    ))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_clone(&mut self) {
        // Clone within the ACTIVE variant's pixels (Generated → its origin PNG).
        let Some(path) = self.editor.active_source_path() else { return };
        // The R27 gate heal and fill take (`refuse_pixel_work_on_a_turned_photo`):
        // `retouch::clone_stamp` re-develops the photo through `render::source_pixels`
        // (the un-turned frame) while the mask comes from the turned canvas, so
        // a clone on a turned photo landed transposed — and baked in place.
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        let Some(src_pt) = self.workspace.tools().clone_src else {
            self.workspace.set_status(tr(lang, K_ACTIONS_ALT_CLICK_TO_SET_THE_CLONE_SOURCE_FIRST).into());
            return;
        };
        let Some(mask_png) = self.workspace.export_area_png(BrushOwner::Stamp) else {
            self.workspace.set_status(tr(lang, K_ACTIONS_BRUSH_THE_AREA_TO_CLONE_OVER_FIRST).into());
            return;
        };
        let Some(out) = unique_out(&path, "clone") else {
            self.workspace.set_status(tr(lang, K_ACTIONS_OVER_999_RETOUCH_MASTERS_FOR_THIS_PHOTO_CLEAN_UP).into());
            return;
        };
        self.workspace.set_busy(true);
        self.workspace.set_status(tr(lang, K_ACTIONS_CLONING_LOCAL_PIXEL_COMPUTE).into());
        let full_res = self.workspace.tools().clone_fullres;
        let edge = self.editor.canvas_edge(self.workspace); // bake at the CANVAS's res (canvas_edge)
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        // The master is baked over the canvas's own base (v1.17.0): its
        // recipe's camera profile and Look table, which the card's preview
        // develop cannot add afterwards.
        let part = autoshade::render::BasePart::of(&self.editor.doc().recipe);
        let (epoch, _flag) = self.workspace.arm_cancel(); // local compute: Cancel = abandon (epoch discard)
        self.services.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                let res = (|| -> RetouchDone {
                    let mask_tmp = gui_tmp_png("clone");
                    std::fs::write(&mask_tmp, &mask_png)?;
                    let rep = autoshade::retouch::clone_stamp(&path, &part, &mask_tmp, src_pt, full_res, &out);
                    let _ = std::fs::remove_file(&mask_tmp);
                    let rep = rep?;
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    // InPlace: a pixel transplant of the current rendition — bake it
                    // into the active variant's base + repoint origin at the artifact.
                    Ok((
                        img,
                        RetouchNote::Cloned { n: rep.spots, out: out.clone() },
                        out,
                        RetouchKind::InPlace,
                    ))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_reimagine(&mut self) {
        // Always reimagine the NEGATIVE, never a generated variant's pixels —
        // regenerating a rendition is the double-cook path the variant model
        // exists to avoid. The negative is the ▣ card's pixel source: the
        // loaded file, or the in-place master it hangs off after a denoise /
        // heal (2026-09-13 — the model used to be sent the un-retouched file
        // while every card beside it showed the retouched one). Names still
        // key on the photo: each call gets a UNIQUE ./out PNG so two
        // Generated variants never alias the same origin (which would
        // cross-wire their export / reverse-fit).
        let Some(path) = self.editor.photo().src_path.clone() else { return };
        let Some(negative) = self.negative_path() else { return };
        // R27 gate (`refuse_pixel_work_on_a_turned_photo`): the ✨ card's
        // raster would land in the EXIF frame, and a baked card cannot be turned.
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        // First FREE ./out name (shared `unique_out` probe) so
        // delete-then-reimagine can't reuse a number whose PNG a surviving
        // variant still points at. Refuse past the cap rather than alias:
        // reusing an existing origin would cross-wire two variants'
        // export / reverse-fit.
        let Some(out) = unique_out(&path, "reimagine") else {
            self.workspace.set_status(tr(self.lang, K_ACTIONS_OVER_999_GENERATED_VARIANTS_FOR_THIS_PHOTO_CLEAN).into());
            return;
        };
        let prompt = {
            // The Reimagine section's OWN prompt field (no longer the shared
            // Direction — each prompt entry pairs with its own trigger).
            let g = self.workspace.genai().reimagine_prompt.trim();
            if g.is_empty() {
                "Develop this photo into a finished, natural-looking edit: balanced exposure and \
                 contrast, pleasing realistic colour; keep the scene true to the original."
                    .to_string()
            } else {
                g.to_string()
            }
        };
        self.workspace.set_busy(true);
        let lang = self.lang;
        self.workspace.set_status(tr(lang, K_ACTIONS_AI_GENERATING_GPT_IMAGE_HIGH_QUALITY_CAN_RUN_MIN).into());
        // The PREFERENCE here, not `canvas_edge`: this is the one bake that
        // renders from src_path (never the canvas — see above) and lands as a
        // NEW variant, so nothing is composited under it and there is no
        // raster to match. Following a stale canvas — a background 1280 bake
        // clicked while the preference sat at 4096 — pinned the new variant
        // below the resolution the user asked for, behind a refusal whose own
        // remedy (save) Ctrl+S REFUSES for a generated variant.
        let edge = self.workspace.genai().preview_edge.clamp(640, 8192);
        let retry = self.workspace.fit().reimagine_retry; // paid opt-in, captured at click time
        let out_claim = out.clone(); // release the claim on failure (worker tail)
        let out_panic = out.clone(); // …and on a worker panic (see the error closure)
        let (epoch, flag) = self.workspace.arm_cancel();
        let ptx = self.services.tx.clone();
        self.services.spawn_worker(
            move || {
                // Progress heartbeats → status bar; the cancel flag stops the
                // stream between events. Installed on THIS worker thread only.
                autoshade::generative::set_worker_hooks(Some(autoshade::generative::WorkerHooks {
                    progress: Box::new(move |m| {
                        let _ = ptx.send(Msg::Progress(epoch, m));
                    }),
                    cancel: flag,
                }));
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&negative))); // full-frame commit budget (budget.rs)
                let res = (|| -> RetouchDone {
                    let cfg = autoshade::config::Config::load();
                    // fidelity "high": the library composes the faithfulness
                    // scaffold onto the prompt and measures D afterwards.
                    let report = autoshade::generative::reimagine(
                        &cfg, &negative, &prompt, "high", &cfg.openai_image_quality, retry, &out,
                    )?;
                    // baked-by-construction: the pixel master this job just wrote.
                    let img = autoshade::render::thumbnail(&autoshade::decode::load_image(&out)?, edge, edge);
                    // NewGenerated: a whole-frame rendition → a new Generated variant.
                    Ok((
                        img,
                        RetouchNote::Reimagined {
                            out: out.clone(),
                            divergence: report.divergence.d,
                            discarded: report.first_divergence,
                        },
                        out,
                        RetouchKind::NewGenerated,
                    ))
                })();
                if res.is_err() {
                    release_empty_claim(&out_claim);
                }
                Msg::Retouched(epoch, Box::new(res))
            },
            move |e| {
                // The worker PANICKED (caught by spawn_worker), so the tail
                // above never ran — release here too.
                release_empty_claim(&out_panic);
                Msg::Retouched(epoch, Box::new(Err(e)))
            },
        );
    }

pub(crate) fn start_style_prompt(&mut self) {
        let (Some(base), Some(tgt)) = (self.editor.photo().source_preview.clone(), self.editor.fit_target(self.workspace))
        else {
            return;
        };
        if self.workspace.busy() {
            return;
        }
        let src_path = self.editor.photo().src_path.clone();
        // No lang capture: the worker returns FACTS; the landing localises
        // them with the language live at landing time (L12#4).
        self.workspace.set_busy(true);
        self.workspace.set_status(tr(self.lang, K_ACTIONS_EXTRACTING_STYLE_PROMPT_VISION_5_20S).into());
        self.services.spawn_worker(
            move || {
                let res = (|| -> anyhow::Result<(String, StyleNote)> {
                    let jpg = |img: &image::DynamicImage| -> anyhow::Result<Vec<u8>> {
                        let mut buf = Vec::new();
                        image::DynamicImage::ImageRgb8(autoshade::render::thumbnail(img, 768, 768).to_rgb8())
                            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)?;
                        Ok(buf)
                    };
                    // baked-by-construction: fit_target is a Generated variant's ./out raster.
                    let target = autoshade::decode::load_image(&tgt)?;
                    let prompt = autoshade::advisor::describe_style(&jpg(&base)?, &jpg(&target)?)?;
                    // The side-file is a convenience copy — its write failing
                    // must not discard the vision result the user paid for.
                    // The note tells the truth either way (the old fixed
                    // message claimed the file was saved unconditionally).
                    let note = match &src_path {
                        Some(p) => {
                            let out = autoshade::pipeline::default_out(p, "style", "txt");
                            // Same protocol as every deliverable (W33): the
                            // library guard, then stage + rename — a direct
                            // fs::write truncated the previous prompt in
                            // place and followed a leaf symlink.
                            match autoshade::pipeline::guard_readonly(&out, p)
                                .and_then(|()| autoshade::pipeline::ensure_parent(&out))
                                .and_then(|()| {
                                    autoshade::render::stage_and_publish(&out, |staged| {
                                        Ok(std::fs::write(staged, &prompt)?)
                                    })
                                })
                            {
                                Ok(()) => StyleNote::SavedCopy,
                                Err(e) => StyleNote::SaveFailed(e.to_string()),
                            }
                        }
                        None => StyleNote::NotSaved,
                    };
                    Ok((prompt, note))
                })();
                Msg::Styled(Box::new(res))
            },
            |e| Msg::Styled(Box::new(Err(e))),
        );
    }
}
