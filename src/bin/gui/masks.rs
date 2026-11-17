//! Raster mask lifecycle: brush, bake, refine, PNG export.

use super::*;

/// Write a preview to a per-process temp PNG a model sidecar reads — the
/// input half every original-frame sidecar run shares (segmentation, depth).
pub(crate) fn stage_preview_png(base: &image::DynamicImage, tag: &str) -> anyhow::Result<PathBuf> {
    let tmp = std::env::temp_dir().join(format!("autoshade_{tag}_{}.png", std::process::id()));
    base.to_rgb8().save(&tmp).map_err(|e| anyhow::anyhow!("write the {tag} input {}: {e}", tmp.display()))?;
    Ok(tmp)
}

impl Commands<'_> {
    /// Run the AI segmentation sidecar on the ORIGINAL-frame preview and attach
    /// the resulting raster as a Bitmap local mask (gap batch A②). The AI only
    /// picks WHERE — every actual edit stays a deterministic recipe slider.
    pub(crate) fn start_segment(&mut self, target: &'static str, label: &'static str) {
        if self.workspace.busy() {
            return;
        }
        let lang = self.lang;
        // The STATUS shows a localised name; the mask's persisted `name` stays
        // the stable English label — recipe.json / XMP are data and must
        // survive a language switch (a translated name baked into the sidecar
        // can never be matched or re-keyed again).
        let disp = tr(lang, label).to_string();
        // The ORIGINAL frame the recipe stores the mask in (v1.17.0: a
        // corrected base is mapped back first).
        let Some(base) = self.editor.original_frame_base() else { return };
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = trf(
            lang,
            K_GUI_AI_SEGMENTING_FIRST_RUN_AUTO_DOWNLOADS_THE_MODEL,
            &[("what", &disp)],
        );
        self.services.spawn_worker(
            move || {
                let res = (|| -> anyhow::Result<(String, PathBuf, String)> {
                    let cfg = autoshade::config::Config::load();
                    let opts = autoshade::segment::SegmentOpts::from_config(&cfg, target);
                    // The sidecar sees the ORIGINAL-frame preview — the space recipe
                    // masks live in. Preview resolution is enough: the engine samples
                    // the raster bilinearly in normalised coords at any render size.
                    let tmp = stage_preview_png(&base, &format!("seg-{target}"))?;
                    // A FRESH claimed name per run (mask-sky.png, -2, …):
                    // rewriting one fixed file replaced bytes a SAVED recipe
                    // still referenced before any save — the same corruption
                    // class the zoned-fit raster had. The Segmented handler
                    // re-points the existing mask entry by name FAMILY, so a
                    // rerun still refreshes instead of stacking.
                    let mask = autoshade::store::claim_raster(&src, &format!("mask-{target}"))?;
                    let run = autoshade::segment::segment_file(&opts, &tmp, &mask);
                    let _ = std::fs::remove_file(&tmp);
                    let report = match run {
                        Ok(r) => r,
                        Err(e) => {
                            // Release the claimed name: a failed run leaves its
                            // create_new 0-byte slot (segment.py publishes
                            // atomically, so an error means nothing real landed).
                            let _ = std::fs::remove_file(&mask);
                            return Err(e);
                        }
                    };
                    // English label into the recipe (stable data), not `disp`.
                    // The BACKEND rides alongside: `--target subject` has two
                    // possible answers and until now the GUI showed the same
                    // sentence for both, so a photographer whose machine had
                    // quietly degraded to U²-Net could not tell (R29 B4's own
                    // registration). It is NOT persisted — it describes the run,
                    // not the mask, and recipe.json is data.
                    Ok((label.to_string(), mask, report.backend))
                })();
                Msg::Segmented(res)
            },
            |e| Msg::Segmented(Err(e)),
        );
    }










    /// Arm `owner`'s brush: its area comes onto the canvas, every other
    /// canvas tool is swept (the one exclusion, `disarm_tools`), then the
    /// brush flag goes up — the Stamp's `clone_mode`, the others' `paint_mode`.
    pub(crate) fn arm_brush(&mut self, owner: BrushOwner) {
        self.workspace.select_brush_owner(owner);
        self.workspace.disarm_tools(&mut self.editor);
        if owner == BrushOwner::Stamp {
            self.workspace.tools_mut().clone_mode = true;
        } else {
            self.workspace.masks_mut().paint_mode = true;
        }
    }


    /// The K key: toggle the brush of the tool whose area is on the canvas;
    /// during a mask-brush session it is that session's Esc, as before.
    pub(crate) fn toggle_brush_key(&mut self) {
        if self.workspace.masks().mask_brush.is_some() {
            self.workspace.disarm_tools(&mut self.editor);
        } else if self.workspace.brush_armed(self.workspace.masks().paint_owner) {
            self.workspace.disarm_brush();
        } else {
            self.arm_brush(self.workspace.masks().paint_owner);
        }
    }




    /// Arm a mask-brush session: `target = None` paints a NEW Bitmap mask on
    /// Apply; `Some(i)` edits mask `i`'s raster — seeded into the canvas (as
    /// the red display wash) and the greyscale weight buffer, at canvas
    /// resolution, so the session bakes exactly what it shows.
    pub(crate) fn start_mask_brush(&mut self, target: Option<usize>) {
        self.workspace.disarm_tools(&mut self.editor);
        let Some(base) = self.editor.photo().base_preview.as_ref() else { return };
        let (mw, mh) = base.dimensions();
        // The session takes the canvas; the owner's area waits in the store.
        self.workspace.stash_owner_area();
        let mut gray = image::GrayImage::new(mw, mh);
        let mut canvas = image::RgbaImage::new(mw, mh);
        // The raster this session SEEDS from, asked by file and not by variant
        // (`render::geometry_raster_path`): a reverse-fit zone rides Lightroom's
        // Select Sky component now, and a `Bitmap`-only test would have made
        // 「🖌 Edit raster」 open an empty canvas over a mask that has pixels.
        if let Some(i) = target
            && let Some(path) =
                self.editor.doc().recipe.masks.get(i).and_then(|m| autoshade::render::geometry_raster_path(&m.mask))
        {
            // Through THE raster entry, in the sensor frame this canvas
            // paints in (a reverse-fit zone may be drawn lens-corrected).
            let at = std::path::Path::new(path);
            match autoshade::render::open_mask_raster(at).and_then(|r| r.into_sensor_gray(&self.editor.doc().recipe, at)) {
                Ok(img) => {
                    let g = image::imageops::resize(
                        &img,
                        mw,
                        mh,
                        image::imageops::FilterType::Triangle,
                    );
                    for (x, y, p) in g.enumerate_pixels() {
                        if p[0] > 0 {
                            let a = (p[0] as u16 * 160 / 255) as u8;
                            canvas.put_pixel(x, y, image::Rgba([255, 64, 64, a]));
                        }
                    }
                    gray = g;
                }
                Err(e) => {
                    let t = trf(
                        self.lang,
                        K_GUI_COULD_NOT_LOAD_THIS_MASK_S_RASTER_STARTING_FROM,
                        &[("err", &e.to_string())],
                    );
                    self.workspace.toast(ToastKind::Error, t);
                }
            }
        }
        self.workspace.masks_mut().mask_paint = Some(canvas);
        self.workspace.paint_mask_changed(None);
        self.workspace.overlay_mut().mask_dirty_rect = None;
        self.workspace.masks_mut().mask_brush_gray = Some(gray);
        self.workspace.masks_mut().mask_brush = Some((target, false));
        self.workspace.masks_mut().paint_mode = true;
        *self.workspace.status_mut() = tr(
            self.lang,
            K_GUI_BRUSH_MASK_PAINT_TO_SELECT_ERASE_REMOVES_APPLY_B,
        )
        .into();
    }



    /// Refine mask `i`'s raster to FULL resolution: build the full-res GUIDE
    /// pixels in a worker, guided-filter the raster against them
    /// (render::refine_mask_guided), save under a fresh claim. The result is
    /// matched back by the stored raster reference — the strip may have
    /// changed while the worker ran.
    ///
    /// TWO paths, deliberately not one: the guide is the ACTIVE VARIANT's
    /// pixel source (`active_source_path` — a retouched/generated variant
    /// refines against the pixels on screen, not the negative underneath),
    /// while the output raster is claimed against `src_path` so it lands in
    /// THIS photo's develop directory (`store::raster_target` homes a claim by
    /// source; a variant-homed claim would split this photo's rasters away
    /// from its version snapshots and its deleted-version registry).
    /// The guide goes through `render::source_pixels`, the one raw-vs-baked
    /// dispatch — this worker fed a .ARW straight to `decode::load_image`
    /// since v0.22, which is the "AI mask refine failed" report.
    pub(crate) fn start_mask_refine(&mut self, i: usize) {
        // The frame gate the seven pixel doors take (panels/retouch.rs):
        // the guide is read through `source_pixels` in the EXIF frame while
        // the raster being refined was turned into the plate frame by
        // `rotate_recipe`, and a guided filter handed a turned mask and an
        // unturned guide writes a refined raster that fits neither.
        if self.workspace.busy() || self.refuse_pixel_work_on_a_turned_photo() {
            return;
        }
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let Some(guide_src) = self.editor.active_source_path() else { return };
        let Some(path) = self.editor.doc().recipe
            .masks
            .get(i)
            .and_then(|m| autoshade::render::geometry_raster_path(&m.mask))
            .map(str::to_string)
        else {
            return;
        };
        self.workspace.set_busy(true);
        *self.workspace.status_mut() =
            tr(self.lang, K_GUI_REFINING_THE_MASK_TO_FULL_RESOLUTION_DECODING_TH)
                .into();
        let stored_ref = path.clone();
        // The budget precheck below is an AGGREGATE question ("would the refined
        // raster still be loadable next to this recipe's other active
        // rasters?"), so the recipe travels with the job. Spawn-time snapshot by
        // design: it is the same recipe the landing re-validates `stored_ref`
        // against, and the alternative — reading `self.editor.doc().recipe` from the worker —
        // is not available on a background thread anyway.
        let recipe_at_spawn = self.editor.doc().recipe.clone();
        self.services.spawn_worker(
            move || {
                // Budget permit BEFORE the big-decode mutex — the one ordering
                // every taker of both follows (budget.rs).
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&guide_src)));
                let res = (|| -> anyhow::Result<MaskRefineOutcome> {
                    // Through THE raster entry, refined in the sensor frame
                    // the full-resolution guide is in (written unmarked).
                    let at = std::path::Path::new(&path);
                    let mask = autoshade::render::open_mask_raster(at)
                        .and_then(|r| r.into_sensor_gray(&recipe_at_spawn, at))
                        .map_err(|e| anyhow::anyhow!("load mask raster {path}: {e:#}"))?;
                    // The same process-wide permit the master/thumb decodes
                    // take: this arm is a FULL-resolution develop or decode by
                    // definition (no cap — full-resolution edges are the whole
                    // point of refining), so it always counts as a big one.
                    let _big_permit = big_decode_gate().lock().unwrap_or_else(|p| p.into_inner());
                    let full = autoshade::render::source_pixels(&guide_src, None)?;
                    // BEFORE the filter and before any claim: a refined raster
                    // is written at the GUIDE's resolution, and one past the
                    // mask budget could never be read back — the mask would go
                    // silently inert at the next open and at export. Refuse
                    // here, with nothing written, instead of at delivery time.
                    // A typed fact, not a sentence: the landing does the
                    // wording (L12#4).
                    //
                    // The question is the AGGREGATE one the loader actually
                    // asks (R22 H1): this raster PLUS the recipe's other active
                    // rasters, minus the one this write replaces. Asking only
                    // "does this raster fit alone?" passed a second 61 MP
                    // refine and then blew the 256 MB aggregate — hard bail at
                    // export, silent skip in the preview.
                    if !autoshade::render::mask_raster_write_fits_budget(
                        &recipe_at_spawn,
                        Some(stored_ref.as_str()),
                        full.width(),
                        full.height(),
                    ) {
                        return Ok(MaskRefineOutcome::OverBudget {
                            w: full.width(),
                            h: full.height(),
                        });
                    }
                    let long = full.width().max(full.height());
                    let refined = autoshade::render::refine_mask_guided(
                        &mask,
                        &full,
                        (long / 500).max(4) as usize,
                        1e-4,
                    );
                    let out = autoshade::store::claim_raster(&src, "mask-refined")?;
                    refined.save(&out)?;
                    // Same L03 rule as the brush writer: durable first.
                    autoshade::store::durable_adopt(&out)?;
                    Ok(MaskRefineOutcome::Refined(i, stored_ref, out))
                })();
                Msg::MaskRefined(res)
            },
            |e| Msg::MaskRefined(Err(e)),
        );
    }
}

/// PNG bytes of one canvas' EXPORT mask: painted → transparent (regenerate /
/// heal here), unpainted → opaque — the alpha > 10 predicate of `has_painted_mask`.
pub(crate) fn export_png_of(m: &image::RgbaImage) -> Option<Vec<u8>> {
    let (w, h) = (m.width(), m.height());
    let mut out = image::RgbaImage::new(w, h);
    for (x, y, p) in m.enumerate_pixels() {
        let painted = p.0[3] > 10;
        out.put_pixel(x, y, image::Rgba([0, 0, 0, if painted { 0 } else { 255 }]));
    }
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(out)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
}

/// Bake one raster op (feather / expand / contract) on mask `i`'s bitmap:
/// load → transform → claim a fresh raster → repoint (immutability rule —
/// see commit_mask_brush). Synchronous at the raster's own resolution.
pub(crate) fn bake_mask_raster(
editor: &mut Editor,
workspace: &mut WorkspaceView,
lang: Lang,
    i: usize,
    op: impl FnOnce(&image::GrayImage) -> image::GrayImage,
    tag: &str,
) {
    let Some(src) = editor.photo().src_path.clone() else { return };
    let Some(path) = editor.doc().recipe
        .masks
        .get(i)
        .and_then(|m| autoshade::render::geometry_raster_path(&m.mask))
        .map(str::to_string)
    else {
        return;
    };
        // Through THE raster entry, baked in the sensor frame the result is
    // written in (unmarked).
    let at = std::path::Path::new(&path);
    let done = autoshade::render::open_mask_raster(at)
        .and_then(|r| r.into_sensor_gray(&editor.doc().recipe, at))
        .map(|img| op(&img))
        .and_then(|g| {
            let p = autoshade::store::claim_raster(&src, tag).map_err(anyhow::Error::from)?;
            g.save(&p).map_err(anyhow::Error::from)?;
            // Same L03 rule as the brush writer: payload durable first.
            autoshade::store::durable_adopt(&p)?;
            Ok(p)
        });
    match done {
        Ok(p) => {
            // Repoint in place — same rule, same reason as
            // `commit_mask_brush`. The read above already proved this
            // geometry carries a raster, so the slot is there.
            if let Some(slot) =
                autoshade::render::geometry_raster_path_mut(&mut editor.doc_mut().recipe.masks[i].mask)
            {
                *slot = p.to_string_lossy().into_owned();
            }
            editor.session_mut().dirty = true;
            workspace.overlay_mut().overlay_stale = true;
            *workspace.status_mut() =
                tr(lang, K_GUI_MASK_RASTER_UPDATED_ITS_ADJUSTMENTS_NOW_APPLY_TO)
                    .into();
        }
        Err(e) => {
            let t = trf(
                lang,
                K_GUI_COULD_NOT_EDIT_THIS_MASK_S_RASTER,
                &[("err", &e.to_string())],
            );
            workspace.toast(ToastKind::Error, t);
        }
    }
}
