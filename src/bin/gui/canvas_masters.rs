//! The canvas's master cache, source frame and re-develop helpers: which pixel
//! master a card hangs off, the film edge/RAW facts the develop needs, and the
//! re-develop flight. `workers.rs` keeps the message pump.

use crate::*;

impl Commands<'_> {
/// Decode a variant's retouched master off-thread — a 61 MP TIFF takes
/// seconds, so the UI thread never does this inline — and install it at
/// `preview_edge`, like every other master install (open: `load_active`'s
/// worker; retouch: the retouch panel). This door used to keep the raster
/// at SENSOR size: ~0.5 GiB resident, a ~244 MB paint mask from
/// `refresh_active_pixels`, and a Before texture past the 8192 max
/// texture dimension of common backends (L02).
///
/// `preview_edge`, not `canvas_edge()`: at cold-switch time the canvas
/// still holds the OUTGOING variant's raster, so `canvas_edge()` would
/// key the install to a stale edge — and `preview_edge` is exactly what
/// the open path files the same master under, so the two caches agree.
///
/// One in-flight decode per (photo, origin): repeat entries (re-clicks
/// into a still-decoding card, a stash restore) coalesce instead of
/// stacking a fresh decode thread each time; an already-decoded master
/// comes straight from `master_cache`. And `decode::load_image`, not
/// `image::open`: the master rides the same decode budget and
/// orientation handling as every other source.
pub(crate) fn spawn_master_load(
    &mut self,
    ctx: &Context,
    photo: PathBuf,
    origin: PathBuf,
) {
    let edge = self.workspace.genai().preview_edge.clamp(640, 8192);
    // Cache probe first: a revisited card installs synchronously, with
    // no worker and no "loading…" status to flash.
    if let Some(arc) = self.workspace.cached_master(&origin, edge) {
        self.install_master(ctx, &photo, &origin, arc);
        return;
    }
    if !self.workspace.open_mut().master_loads.insert((photo.clone(), origin.clone())) {
        return;
    }
    *self.workspace.status_mut() = tr(
        self.lang,
        K_GUI_LOADING_THIS_VARIANT_S_RETOUCHED_MASTER_SHOWING,
    )
    .into();
    // Identity BEFORE the read (the curve-memo / remember_base rule,
    // L12-2): a master rewritten DURING the decode must MISS on the next
    // probe — stat'ing at landing filed the OLD pixels under the NEW
    // stamp, a false HIT. The spawn edge rides along too (L12-6): the
    // landing used to read the CURRENT preference, so a mid-flight px
    // change filed these pixels under a resolution they never had.
    let stamp = file_stamp(&origin);
    let (p2, o2) = (photo.clone(), origin.clone());
    self.services.spawn_worker(
        move || {
            let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&origin))); // full-frame commit budget (budget.rs)
            // The same >24 MP permit `request_thumb` takes: a master
            // decode is deliberately not `busy`-gated, so N cold cards
            // with distinct origins could otherwise decode N 60 MP
            // rasters at once.
            let _big_permit = image::ImageReader::open(&origin)
                .and_then(|r| r.into_dimensions().map_err(std::io::Error::other))
                .is_ok_and(|(w, h)| w as u64 * h as u64 > 24_000_000)
                .then(|| big_decode_gate().lock().unwrap_or_else(|p| p.into_inner()));
            // baked-by-construction: `origin` is the develop store's baked master.
            let img = autoshade::decode::load_image(&origin).map(|im| {
                // GUARDED shrink: plain `thumbnail` UPSCALES, which would
                // inflate a small master instead of bounding a large one.
                if im.width().max(im.height()) > edge {
                    autoshade::render::thumbnail(&im, edge, edge)
                } else {
                    im
                }
            });
            Msg::MasterLoaded { photo, origin, edge, stamp, img: Box::new(img) }
        },
        move |e| {
            Msg::MasterLoaded { photo: p2, origin: o2, edge, stamp, img: Box::new(Err(e)) }
        },
    );
}



/// Install decoded master pixels into every strip entry that still
/// references this exact master and still awaits pixels — by IDENTITY,
/// never by index, so a delete/reorder mid-decode cannot misfile them.
pub(crate) fn install_master(
    &mut self,
    ctx: &Context,
    photo: &PathBuf,
    origin: &PathBuf,
    arc: Arc<image::DynamicImage>,
) {
    if self.editor.photo().src_path.as_ref() != Some(photo) {
        return;
    }
    let active = self.editor.active();
    let mut hit_active = false;
    for (i, v) in self.editor.doc_mut().variants.iter_mut().enumerate() {
        if v.base.is_none() && v.origin.as_ref() == Some(origin) {
            v.base = Some(arc.clone());
            hit_active |= i == active;
        }
    }
    if hit_active {
        // The canvas was showing the disclosed source stand-in — swap in
        // the real pixels now that they exist.
        self.editor.refresh_active_pixels(&mut self.workspace, self.lang, ctx);
        self.editor.set_canvas_status(&mut self.workspace, self.lang, "restored the canvas pixels");
    }
}

/// Queue a thumbnail decode for `idx` if it isn't cached/queued and we're
/// under the concurrency cap. Uses the camera's embedded preview (fast) — the
/// double-processing concern only applies to the develop base, not a 56px chip.
/// A persistent disk cache (keyed on path+mtime+size) makes every later
/// session — and every scroll-back after texture eviction — a ~1 ms JPEG
/// read instead of a full decode.
pub(crate) fn request_thumb(&mut self, idx: usize) {
    if self.workspace.gal().thumbs.contains_key(&idx) || self.workspace.gal().thumb_requested.contains(&idx) {
        return;
    }
    // A BOUNDED retry, because both extremes are wrong. Dropping the
    // marker on failure re-requested every frame — six decode threads per
    // 100 ms for as long as the row was visible. Keeping it forever
    // blanked the row for the session, and the LRU that was supposed to
    // provide the retry only prunes above THUMB_TEX_CAP, so a folder
    // under that size never retried at all. Three attempts covers the
    // transient causes (AV lock, a slow share) and stops dead on a file
    // that simply cannot be decoded.
    if self.workspace.gal().thumb_fail.get(&idx).is_some_and(|n| *n >= 3) {
        return;
    }
    if self.workspace.gal().thumb_inflight >= MAX_THUMB_INFLIGHT {
        return;
    }
    let Some(path) = self.workspace.gal().gallery.get(idx).cloned() else { return };
    self.workspace.gal_mut().thumb_requested.insert(idx);
    self.workspace.gal_mut().thumb_inflight += 1;
    let generation = self.workspace.gal().gallery_gen;
    self.services.spawn_worker(
        move || {
            let res = (|| -> anyhow::Result<image::DynamicImage> {
                let cache = thumb_cache_file(&path);
                if let Some(p) = &cache
                    && let Ok(img) = image::open(p)
                {
                    return Ok(img);
                }
                // Large baked rasters (the app's own 60 MP TIFF/PNG exports)
                // decode whole before the 160px shrink — ~360 MB each, and
                // MAX_THUMB_INFLIGHT of them at once spiked to ~2 GB. Gate
                // decodes above 24 MP behind one permit; the header read is
                // cheap and RAWs (embedded-JPEG previews) stay concurrent.
                let _big_permit = if !autoshade::decode::is_raw(&path)
                    && image::ImageReader::open(&path)
                        .and_then(|r| r.into_dimensions().map_err(std::io::Error::other))
                        .is_ok_and(|(w, h)| w as u64 * h as u64 > 24_000_000)
                {
                    Some(big_decode_gate().lock().unwrap_or_else(|p| p.into_inner()))
                } else {
                    None
                };
                // In the frame the develop asks for, so a rotated photo
                // reads the same way in the grid as on the canvas and in
                // the export. The same number keys the cache file above —
                // read once here rather than threaded from it, because the
                // cache may legitimately be absent (unstattable source).
                let thumb = autoshade::render::thumbnail(
                    &autoshade::decode::preview_only_turned(&path, autoshade::store::saved_quarter_turns(&path))?,
                    THUMB_EDGE,
                    THUMB_EDGE,
                );
                if let Some(p) = &cache {
                    save_thumb_cache(p, &thumb); // best-effort write-through
                }
                Ok(thumb)
            })();
            Msg::Thumb { generation, idx, img: Box::new(res) }
        },
        // The Err handler decrements thumb_inflight like any decode failure.
        move |e| Msg::Thumb { generation, idx, img: Box::new(Err(e)) },
    );
}

/// Queue the latest preview state on the single CPU worker. Dispatch is O(1):
/// base pixels are Arc-shared and the recipe is small. While a frame is in
/// flight, edits only set `dirty`; no parallel render storm is possible.
pub(crate) fn start_redevelop(&mut self) {
    if self.editor.scheduler().inflight {
        return;
    }
    // The canvas is only right over pixels that carry its recipe's base
    // part (v1.17.0): hold the frame until they do.
    if !self.ensure_source_part() {
        self.editor.session_mut().dirty = false;
        return;
    }
    let Some(base) = self.editor.photo().base_preview.clone() else {
        self.editor.session_mut().dirty = false;
        return;
    };
    // The frame of the pixels (v1.17.0): a master's own marker says
    // whether it is lens-corrected, and a recipe that develops other lens
    // geometry than the one baked into it is refused, as the export
    // refuses it — the canvas keeps its last frame and says why.
    let source = match self.editor.source_frame().and_then(|f| f.check(&self.editor.doc().recipe).map(|()| f)) {
        Ok(f) => f,
        Err(e) => {
            self.editor.session_mut().dirty = false;
            self.workspace.set_status(format!("{}: {e:#}", tr(self.lang, K_GUI_PREVIEW_DEVELOP_FAILED)));
            return;
        }
    };
    let recipe = self.editor.doc().recipe.clone();
    let show_clipping = self.workspace.chrome().show_clipping;
    let film = self.editor.film_short_edge();
    self.editor.scheduler_mut().inflight = true;
    self.editor.session_mut().dirty = false;
    self.services.spawn_worker(
        move || Msg::Developed(Box::new(Ok(build_preview(base, recipe, show_clipping, film, source)))),
        |e| Msg::Developed(Box::new(Err(e))),
    );
}

/// Is the source base the right one for the canvas recipe — `true` — or is
/// a re-develop needed first? (v1.17.0)
///
/// A source-based card develops over `source_preview` with the preview
/// develop, which runs no profile stage: the camera profile and the Look's
/// colour table must already be in those pixels, developed under the
/// recipe's own [`autoshade::render::BasePart`]. The open develops the
/// part the photo's develop starts from, so this answers `true` on the
/// common path; a recipe that names another (an imported sidecar's own
/// profile, a paste, an undo across one, a version) re-develops the
/// source in the background at the canvas's edge and answers `false`
/// until it lands ([`Msg::BaseRedeveloped`]). A baked source, or a card
/// on its own pixels, has nothing to re-develop.
pub(crate) fn ensure_source_part(&mut self) -> bool {
    if !self.editor.source_is_raw() || self.editor.active_variant().is_some_and(|v| v.base.is_some()) {
        return true;
    }
    let want = autoshade::render::BasePart::of(&self.editor.doc().recipe);
    if want == self.workspace.open().source_part {
        return true;
    }
    if self.workspace.open().source_part_inflight.as_ref() == Some(&want) {
        return false;
    }
    let Some(path) = self.editor.photo().src_path.clone() else { return true };
    let edge = self.workspace.genai().preview_edge.clamp(640, 8192);
    self.workspace.open_mut().source_part_inflight = Some(want.clone());
    let (p, part) = (path.clone(), want.clone());
    self.services.spawn_worker(
        move || {
            let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&p)));
            let img = autoshade::render::base_develop(&p, &part, Some(edge)).map(Arc::new);
            Msg::BaseRedeveloped { path: p, part: Box::new(part), img: Box::new(img) }
        },
        move |e| Msg::BaseRedeveloped { path, part: Box::new(want), img: Box::new(Err(e)) },
    );
    false
}

/// Accept one worker-built frame if it still describes the active base +
/// recipe. Old frames are dropped without touching textures; `dirty` remains
/// set by the newer edit and starts next. Texture handles are UPDATED in
/// place, avoiding a manager allocate/free cycle on every slider tick.
pub(crate) fn finish_redevelop(&mut self, ctx: &Context, done: anyhow::Result<PreviewDone>) {
    self.editor.scheduler_mut().inflight = false;
    let frame = match done {
        Ok(frame) => frame,
        // Pure preview work has no ordinary error path; only a caught panic
        // lands here. Keep the last good frame and surface the failure —
        // WITHOUT fail(): that helper also clears the global `busy` flag,
        // which the preview path never set, and a panicking preview worker
        // mid-export used to unlock every busy-gated action.
        Err(e) => {
            let text = format!("{}: {e}", tr(self.lang, K_GUI_PREVIEW_DEVELOP_FAILED));
            self.workspace.set_status(text.clone());
            self.workspace.toast(ToastKind::Error, text);
            return;
        }
    };
    let current = self.editor.photo().base_preview
        .as_ref()
        .is_some_and(|base| Arc::ptr_eq(base, &frame.base))
        && self.editor.doc().recipe == frame.recipe;
    if !current {
        // The frame was solved for a recipe that changed mid-flight. The
        // redispatch gate is `dirty && !develop_inflight`, so a caller
        // that mutated the recipe WITHOUT setting dirty (historically the
        // crop path) would freeze the preview here forever — always
        // re-arm, the gate coalesces to a single fresh develop.
        self.editor.session_mut().dirty = true;
        ctx.request_repaint();
        return;
    }
    self.editor.scheduler_mut().count += 1;
    self.workspace.chrome_mut().histogram = Some(frame.histogram);

    // The ColorImage arrives worker-built (see PreviewDone.after) — the UI
    // thread only uploads. The raw RGB is retained so the clipping toggle
    // can rebuild its overlay later without a redevelop.
    // A frame of the SAME photo and version re-renders in place (a slider
    // drag must not flicker); the first frame of a different one lands on
    // a fresh texture and the old one fades out over it (MOTION_FADE_S).
    let shows = self.editor.photo().src_path.clone().map(|p| (p, self.editor.active()));
    let plates = self.editor.plates_mut();
    match &mut plates.after_tex {
        Some(tex) if plates.after_shows == shows => tex.set(frame.after, TextureOptions::LINEAR),
        old => {
            let fresh = ctx.load_texture("after", frame.after, TextureOptions::LINEAR);
            plates.canvas_fade = Some((old.replace(fresh), Instant::now()));
            ctx.request_repaint();
        }
    }
    self.editor.plates_mut().after_shows = shows;
    // The clipping layer follows the CURRENT toggle, not the one baked at
    // dispatch: pressing J changes no recipe field, so an in-flight frame
    // is still accepted — its stale decision used to blank the layer under
    // a lit ▲ (or resurrect a ghost layer after J-off).
    let clip = if self.workspace.chrome().show_clipping {
        // Crop-blanked like the worker path — the J-mid-flight fallback
        // used to warn outside the crop while the worker's layer did not.
        Some(
            frame
                .clipping
                .unwrap_or_else(|| clipping_overlay_for(&frame.rgb, frame.recipe.crop)),
        )
    } else {
        None
    };
    self.workspace.chrome_mut().last_rgb = Some(frame.rgb);
    match clip {
        Some(clip) => {
            if let Some(tex) = &mut self.workspace.chrome_mut().clip_tex {
                tex.set(clip, TextureOptions::NEAREST);
            } else {
                self.workspace.chrome_mut().clip_tex =
                    Some(ctx.load_texture("clip", clip, TextureOptions::NEAREST));
            }
        }
        None => self.workspace.chrome_mut().clip_tex = None,
    }
    if let Some(v) = self.editor.active_variant_mut() {
        if let Some(tex) = &mut v.thumb {
            tex.set(frame.thumb, TextureOptions::LINEAR);
        } else {
            v.thumb = Some(ctx.load_texture("vthumb", frame.thumb, TextureOptions::LINEAR));
        }
    }
    // A global change can alter a Range Mask's coverage reference. The
    // coverage-aware key makes this a cheap no-op for ordinary local effect
    // sliders, whose coverage is independent of their pixel adjustment.
    self.workspace.overlay_mut().overlay_stale = true;
    ctx.request_repaint();
}
}
