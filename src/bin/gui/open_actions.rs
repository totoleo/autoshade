//! The open path: opening a file (the decode/re-develop hand-off and its
//! guards), a gallery row and a folder. Same `AutoShadeApp` impl as
//! `actions.rs`, split by domain.

use crate::*;

impl Commands<'_> {
    pub(crate) fn open_path(&mut self, path: PathBuf) {
        if self.workspace.busy() {
            // Defence-in-depth: since the px combo refuses BEFORE arming, no
            // live caller reaches this guard with keep_recipe set — but a
            // stale request here once misclassified a genuine cross-photo
            // open as a same-path keep-flight, and any future caller that
            // arms-then-calls gets the same protection instead of relying on
            // its own busy check. Said so, instead of claiming a live
            // scenario.
            self.workspace.cards_mut().keep_recipe = false;
            // Refusal must be visible (the switch_variant rule: a silent
            // no-op reads as a dead UI) — the EMPTY-STATE picker button has
            // no busy gate, so this arm IS user-reachable (L11-4).
            let t = tr(self.lang, K_ACTIONS_BUSY_THE_PHOTO_OPENS_WHEN_THE_CURRENT_TASK_FINIS);
            self.workspace.toast(ToastKind::Error, t);
            return;
        }
        // Flush every typed-but-uncommitted name before the stash below
        // snapshots the strip — a thumbnail click / Ctrl+O / arrow-key nav
        // with a name box focused silently stashed the OLD name (U10). Each
        // buffer carries the photo it was seeded on, so the commits land on
        // the OUTGOING photo even though `src_path` moves below.
        self.commit_pending_names();
        // …and an edit the frame hook has not moved yet lands on its ✎ card
        // before the strip is snapshotted (the immutability rule's boundary
        // discipline — every persist entry point forks first).
        self.fork_edited_card();
        // …and the buffers themselves die with the photo (M15): carried
        // across, a same-index mask / same-number version / same-card rename
        // box on the NEXT photo skipped the reseed and greeted it pre-filled
        // with the previous photo's text.
        self.workspace.masks_mut().mask_name_buf = None;
        self.workspace.cards_mut().version_name_buf = None;
        self.workspace.cards_mut().variant_name_buf = None;
        // An armed strip ✕ dies with the photo too — the index it named
        // belongs to a strip that is about to be replaced (R24-4).
        self.workspace.cards_mut().variant_delete_confirm = None;
        // The mid-open window marker (see confirm_quit_layer): src_path is
        // re-pointed below while recipe/saved_recipe still describe the old
        // photo; cleared in BOTH Msg::Opened arms. `open_same_path` is the
        // FACT the repair gates key on — keep_recipe is only a request, and
        // a stale request must never classify a flight.
        self.workspace.open_mut().open_in_flight = true;
        self.workspace.open_mut().open_same_path = self.editor.photo().src_path.as_ref() == Some(&path);
        // Leaving a photo with unsaved edits: stash the canvas for this
        // session so navigation can never silently destroy work (arrow keys /
        // thumbnail clicks used to drop the whole develop). Ctrl+S still owns
        // disk; returning to the photo restores the stash over the sidecar.
        // "Unsaved" covers PIXELS too: a baked retouch whose master isn't the
        // one recorded in the store's pixels.json would otherwise vanish from
        // the canvas on the way back even though its raster is on disk.
        // SAME-path reopens stash too (no `old != path` gate): clicking the
        // current thumbnail / re-picking the current file takes the fresh
        // branch below, which resets per-photo state and restores stash-then-
        // disk — without the entry, a dirty canvas was silently replaced by
        // the disk state. The stash entry is consumed by that same restore,
        // so a same-path reopen becomes "reload, preserving unsaved work".
        if let Some(old) = self.editor.photo().src_path.clone() {
            let origin = self.editor.active_variant().and_then(|v| v.origin.clone());
            // Background variants count too (H4): their unsaved work has no
            // sidecar to survive in, and the strip used to collapse to the
            // active canvas alone on navigation. THIS photo's strip only:
            // the cross-photo sum (inactive_dirty_variants) belongs to the
            // quit dialog, and using it here chain-stashed clean photos.
            if self.editor.nav_stash_gate_dirty() {
                let others: Vec<StashedVariant> = self.editor.doc().variants
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != self.editor.doc().active)
                    .map(|(_, v)| StashedVariant {
                        kind: v.kind,
                        // Hop 3 of 6 (R24-2): identity + name ride the stash
                        // out; the workers restore is hop 4 back.
                        id: v.id.clone(),
                        name: v.name.clone(),
                        recipe: v.recipe.clone(),
                        base: v.base.clone(),
                        origin: v.origin.clone(),
                    })
                    .collect();
                let active_pos = self.editor.doc().active.min(others.len());
                let active = self.editor.active_variant();
                let recipe = self.editor.doc().recipe.clone();
                let base = active.and_then(|v| v.base.clone());
                let kind = active.map_or(VariantKind::Original, |v| v.kind);
                // The ACTIVE card's identity + name travel with the
                // rest of its state (R24-2): a nav round-trip that
                // re-minted the id would orphan every version taken
                // from this card.
                let id = active
                    .map_or_else(|| ORIGINAL_VARIANT_ID.to_string(), |v| v.id.clone());
                let name = active.and_then(|v| v.name.clone());
                self.workspace.open_mut().nav_stash.insert(
                    old,
                    StashEntry { recipe, base, origin, kind, id, name, others, active_pos },
                );
            }
        }
        let lang = self.lang;
        self.workspace.set_busy(true);
        // Every open starts resolved-until-proven-otherwise; the Opened
        // handler's Unreadable arm re-arms it for THIS photo when the read
        // fails.
        self.workspace.open_mut().open_unresolved = false;
        self.editor.photo_mut().src_path = Some(path.clone());
        // The version list describes the photo `src_path` NAMES, so it dies
        // with the old value of that field — not later, when the decode lands.
        // `refresh_versions` only runs from the completed `Msg::Opened`, and a
        // RAW decode is seconds; for that whole window the Versions panel
        // showed photo A's v1/v2/v3 with live buttons while every action
        // resolved `self.editor.photo().src_path`, which already pointed at photo B. Clicking
        // 🗑 on the stale list called `store::delete_version(B, n)` and
        // PERMANENTLY removed a snapshot of B — recipe and frozen mask
        // rasters, no backup, no undo — then refreshed the list from B so the
        // rows visibly changed and the status line confirmed a delete, hiding
        // what had happened. "＋ Save as version" was the mirror image: it
        // wrote A's canvas into B's develop as B's next version.
        self.workspace.cards_mut().versions.clear();
        // …and so does the armed "overwrite the .xmp beside the photo" confirm
        // (R22-8): it is a claim about THIS photo's neighbouring sidecar, and
        // carrying it across would let one click overwrite the NEXT photo's
        // Lightroom sidecar with no warning at all. Same reasoning as the
        // version list above.
        self.workspace.exp_mut().xmp_beside_confirm = false;
        self.workspace.set_status(trf(lang, K_ACTIONS_DECODING, &[("path", &path.display().to_string())]));
        // Working-preview size is a user choice now (gap batch E): 1280 keeps
        // sliders fluid; 2560/4096 trade tick latency for real 1:1 detail when
        // checking focus / noise.
        let edge = self.workspace.genai().preview_edge.clamp(640, 8192);
        // Decoded-base LRU: gallery culling flips between neighbours constantly,
        // and a hit skips the multi-second full demosaic entirely. Delivered
        // through the ordinary Msg::Opened channel so the handler stays the one
        // place that resets per-photo state.
        if let Some(hit) = self.workspace.cached_base(&path, edge) {
            let _ = self.services.tx.send(Msg::Opened(Box::new(Ok(hit))));
            return;
        }
        self.services.spawn_worker(
            move || {
                // Full-frame commit budget (budget.rs): the demosaic transient
                // pays the corpus peak whatever `edge` caps afterwards.
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path)));
                // Build a CLEAN preview base by developing the RAW sensor data
                // (downscaled), NOT the camera's already-baked 8-bit JPEG preview:
                // re-developing that double-processes it and amplifies its grain when
                // you push tone/clarity. Baked images (PNG/TIFF/JPEG) are their own
                // source. Demosaic is slow, so this runs off the UI thread.
                let res = (|| -> anyhow::Result<OpenedBase> {
                    // The LRU key, captured BEFORE any read (the curve-memo
                    // rule): filing the decode under completion-time state
                    // handed a mid-open replacement the previous content's
                    // pixels and knots under its own key, and a popup-driven
                    // px change mid-flight mislabelled the edge.
                    let src_ident = (edge, file_stamp(&path), pixel_identity(&path));
                    let (thumb, start, lens, as_shot) = if autoshade::decode::is_raw(&path) {
                        // Identity BEFORE the read: the primed answer below
                        // is filed under the content it was computed FROM —
                        // stat'ing after the multi-second develop filed a
                        // mid-open replacement's NEW identity with the OLD
                        // content's answer.
                        let ident = autoshade::pipeline::curve_ident(&path);
                        // The BASE part this photo's develop starts from
                        // (v1.17.0): its saved develop's camera profile and
                        // Look, else the fresh canvas's. Developed under it so
                        // the common open pays ONE develop; a canvas recipe
                        // that settles on another part is re-developed by the
                        // canvas itself (`ensure_source_part`).
                        let part = autoshade::pipeline::photo_base_part(&path);
                        // Developed AT the working edge (the cap runs before
                        // tone/geometry): opening a 61 MP RAW no longer keeps
                        // a full-resolution develop resident just to be
                        // thumbnailed — the knots estimate below is CDF
                        // statistics and reads the same at working res.
                        let full = autoshade::render::base_develop(&path, &part, Some(edge))?;
                        // In-camera lens profile (Sony 0x70xx): one cheap TIFF
                        // parse, stamped all-available-on (the user default).
                        let lens = autoshade::pipeline::fresh_lens_profile(&path);
                        // As-shot WB anchor: one metadata-only decode (no
                        // demosaic) — the Temp slider then speaks absolute
                        // Kelvin for this photo.
                        let as_shot = autoshade::render::as_shot_wb(&path);
                        // The fresh START: the camera-matched base look:
                        // CDF-match the neutral
                        // develop against the camera's own rendition, paired
                        // like with like by the one entry all three open
                        // paths share — the frame the rendition shows, and
                        // the profile's corner lift only when the rendition
                        // shows it (render::camera_base_look).
                        // A RAW with no embedded preview (or a decode hiccup)
                        // just opens without a base look — never a failure.
                        // The decision is the library's one
                        // (`pipeline::fresh_canvas_start_with`); the estimate
                        // below runs only when it is asked for.
                        let estimate = || -> Vec<[f32; 2]> {
                            // The estimate needs the NEUTRAL develop. `full` is
                            // it unless a saved develop names a profile; that
                            // case pays its own neutral develop, and only here.
                            let neutral = if part.is_neutral() {
                                None
                            } else {
                                match autoshade::render::render_to_image(
                                    &path,
                                    &EditRecipe::default(),
                                    None,
                                    Some(edge),
                                ) {
                                    Ok(n) => Some(n),
                                    Err(_) => return Vec::new(),
                                }
                            };
                            match autoshade::decode::embedded_preview(&path) {
                                Ok(Some(cam)) => {
                                    match autoshade::render::camera_base_look(neutral.as_ref().unwrap_or(&full), &lens, &cam) {
                                        // The ANSWER primes the repair memo: the
                                        // open already paid this develop, and
                                        // discarding the result made every later
                                        // repair site (Ctrl+Z, variant switch,
                                        // Ctrl+S) pay a second full decode on
                                        // the UI thread. Answers only — an
                                        // inability stays uncached so the next
                                        // reader retries.
                                        Some(k) => {
                                            autoshade::pipeline::prime_curve_memo(
                                                &path,
                                                ident,
                                                k.clone(),
                                            );
                                            k
                                        }
                                        // None = could not judge. For an OPEN
                                        // that is the same as no base look — the
                                        // repair path is where the distinction
                                        // decides whether a SAVED curve may be
                                        // replaced.
                                        None => Vec::new(),
                                    }
                                }
                                _ => Vec::new(),
                            }
                        };
                        let start = autoshade::pipeline::fresh_canvas_start_with(
                            &path,
                            &autoshade::diag::photo(&path),
                            estimate,
                        );
                        // render_to_image already developed AT the working edge
                        // — `full` IS the thumb. The unconditional
                        // `full.thumbnail(edge, edge)` here duplicated a
                        // ~45 MP frame at the 8192 edge just to copy it; the
                        // resize only runs in the defensive off-spec case.
                        let thumb = if full.width().max(full.height()) > edge {
                            autoshade::render::thumbnail(&full, edge, edge)
                        } else {
                            full
                        };
                        (thumb, (start, part), lens, as_shot)
                    } else {
                        (
                            // baked-by-construction: the !is_raw arm of the open (branch above).
                            autoshade::render::thumbnail(&autoshade::decode::load_image(&path)?, edge, edge),
                            Default::default(),
                            Default::default(),
                            None,
                        )
                    };
                    // The saved develop's baked pixel master (in-place heal /
                    // clone / fill / denoise), when the store records one and
                    // it still resolves — decoded HERE so the UI thread never
                    // loads a 61 MP PNG. A missing/unreadable master degrades
                    // to the plain source open, never a failure.
                    let mut baked_note = None;
                    let baked = autoshade::store::read_pixel_source(&path).and_then(
                        |(origin, generated)| {
                            // baked-by-construction: the store's baked pixel master (a PNG we wrote).
                            let img = match autoshade::decode::load_image(&origin) {
                                Ok(i) => i,
                                Err(e) => {
                                    // Disclosed at LANDING too, not only in a
                                    // log the windowed build cannot show
                                    // (L11-3). Typed detail; on_opened
                                    // localizes.
                                    eprintln!(
                                        "⚠ baked master {} failed to decode ({e}) — opening the un-retouched source",
                                        origin.display()
                                    );
                                    baked_note =
                                        Some(format!("{}: {e:#}", origin.display()));
                                    return None;
                                }
                            };
                            Some((Arc::new(autoshade::render::thumbnail(&img, edge, edge)), origin, generated))
                        },
                    );
                    // Arc once here so every downstream sharer (variants, the
                    // preview worker) is an O(1) refcount bump, not a deep copy.
                    Ok((Arc::new(thumb), start, lens, as_shot, baked, src_ident, baked_note))
                })();
                Msg::Opened(Box::new(res))
            },
            |e| Msg::Opened(Box::new(Err(e))),
        );
    }

    /// Open one of the gallery photos by index (keeps the thumbnail highlighted).
    pub(crate) fn open_gallery_index(&mut self, idx: usize) {
        if self.workspace.busy() {
            return;
        }
        let Some(path) = self.workspace.gal().gallery.get(idx).cloned() else { return };
        self.workspace.gal_mut().selected = Some(idx);
        self.open_path(path);
    }

    /// Scan `dir` (recursively) for sources off the UI thread and replace the
    /// gallery — folders can hold thousands of RAWs, so this never blocks paint.
    pub(crate) fn open_folder(&mut self, dir: PathBuf) {
        if self.workspace.busy() {
            return;
        }
        let lang = self.lang;
        self.workspace.set_busy(true);
        self.workspace.set_status(trf(lang, K_ACTIONS_SCANNING, &[("path", &dir.display().to_string())]));
        self.services.spawn_worker(
            move || {
                let res = autoshade::pipeline::find_sources_counted(&dir)
                    .map(|(list, skipped)| (dir, list, skipped));
                Msg::Folder(Box::new(res))
            },
            |e| Msg::Folder(Box::new(Err(e))),
        );
    }
}
