//! The canvas landings that belong to opening and re-developing the base:
//! the photo-open landing, the camera-profile re-develop and the thumbnail
//! landing. `workers.rs` keeps the message pump and the worker starts.

use crate::*;

impl Commands<'_> {
pub(crate) fn on_opened(&mut self, ctx: &Ctx, lang: Lang, boxed: Box<anyhow::Result<OpenedBase>>) {
                // `keep` distinguishes a fresh open from a preview-resolution
                // re-decode (the px combo): consumed whether the open
                // succeeds or fails so a failure can't leak it into a later
                // open.
                // The KEEP request is honoured only when the recorded
                // FACT agrees the flight was same-path: a stale request
                // must never graft the outgoing photo's whole canvas,
                // strip and history onto an incoming photo.
                let keep = std::mem::take(&mut self.workspace.cards_mut().keep_recipe) && self.workspace.open().open_same_path;
                let edge_revert = self.workspace.open_mut().edge_before_flight.take();
                self.workspace.open_mut().open_in_flight = false; // both arms: the transition ended
                match *boxed {
                Ok((base, (start, part), lens, as_shot, baked, src_ident, baked_note)) => {
                    self.workspace.set_busy(false);
                    // Surfaced BEFORE anything else can early-return
                    // (L11-3): the worker degraded to the un-retouched
                    // source, and stderr is invisible in the windowed
                    // build. Same channel as the MasterLoaded failure.
                    if let Some(err) = &baked_note {
                        let t = trf(
                            lang,
                            K_CANVAS_THIS_VARIANT_S_SAVED_MASTER_COULD_NOT_BE_LOADED,
                            &[("err", err)],
                        );
                        self.workspace.toast(ToastKind::Error, t);
                    }
                    if let Some(p) = self.editor.photo().src_path.clone() {
                        self.workspace.remember_base(
                            &p,
                            &(
                                base.clone(),
                                (start.clone(), part.clone()),
                                lens.clone(),
                                as_shot,
                                baked.clone(),
                                src_ident.clone(),
                                // Never cached: a revisit decodes
                                // nothing, so it must not re-toast.
                                None,
                            ),
                        );
                        // C1/F10 alias disclosure rides the open (once
                        // per photo per session — the store stashes the
                        // note at first key resolution and this consumes
                        // it). Typed note, rendered HERE in the session
                        // language (the worker-closure i18n rule).
                        if !keep && let Some(note) = autoshade::store::take_alias_note(&p) {
                            let t = match note {
                                autoshade::store::AliasNote::Adopted { from } => trf(
                                    lang,
                                    K_CANVAS_EDITS_SAVED_UNDER_AN_OLDER_SPELLING_OF_THIS_PHOT,
                                    &[("path", &from.display().to_string())],
                                ),
                                autoshade::store::AliasNote::SecondDevelop { at } => trf(
                                    lang,
                                    K_CANVAS_A_SECOND_SAVED_DEVELOP_EXISTS_AT_AN_OLDER_SPELLI,
                                    &[("path", &at.display().to_string())],
                                ),
                            };
                            self.workspace.toast(ToastKind::Error, t);
                        }
                    }
                    // Kept for Reset: "reset" = the fresh-open look, so it
                    // needs this photo's knots even when the restored
                    // recipe (legacy save) deliberately carries none.
                    // Deliberately LAST-wins across preview-edge switches
                    // (Reset means the fresh-open look at the CURRENT
                    // edge) while the repair memo pins its FIRST answer
                    // for persistence determinism — the two agree within
                    // the estimator's documented tolerance.
                    self.workspace.open_mut().photo_start = start.clone();
                    self.workspace.open_mut().photo_lens = lens.clone();
                    self.workspace.open_mut().photo_as_shot = as_shot;
                    if keep {
                        // Preview-resolution re-decode: the SOURCE pixels just
                        // changed resolution — keep the whole variant set,
                        // recipe, undo history and view (zoom included: you
                        // switched to 4096px to inspect 1:1, losing the zoom
                        // would defeat the point). Refresh the base a
                        // source-based active variant develops from; hand a
                        // baked-raster variant its own master re-decoded —
                        // and when neither is possible, REFUSE the switch
                        // instead of half-applying it.
                        let (mw, mh) = base.dimensions();
                        // Consume the stash entry the same-path open_path
                        // call just wrote: the KEEP branch preserves the
                        // live canvas in place, so the entry is a stale
                        // duplicate — after an undo-to-clean it would
                        // resurrect the pre-undo edits on the next return.
                        // (Both outcomes below keep the canvas in place, so
                        // this is unconditional.)
                        if let Some(p) = self.editor.photo().src_path.clone() {
                            self.workspace.open_mut().nav_stash.remove(&p);
                        }
                        let active_source =
                            self.editor.active_variant().is_none_or(|v| v.base.is_none());
                        // ONE source of truth for "the worker brought the
                        // master this canvas renders": the branch below
                        // destructures the same `baked`, and a guard that
                        // could drift from it is how a canvas ends up at an
                        // edge nobody recorded.
                        let master_hit = baked.as_ref().is_some_and(|(_, borigin, _)| {
                            self.editor.active_variant().is_some_and(|v| {
                                v.origin.as_deref().is_some_and(|o| same_master(o, borigin))
                            })
                        });
                        if !active_source && !master_hit && let Some(e) = edge_revert {
                            // The switch CANNOT materialize on THIS canvas:
                            // the active variant renders baked pixels
                            // (heal / clone / fill / reimagine) whose master
                            // the worker had nothing to re-decode from —
                            // `pixels.json` is written by SAVE, so an
                            // unsaved retouch records no master at all.
                            // Adopting the new edge anyway was the busy
                            // refusal's residue through the third door: the
                            // status announced a re-decode the canvas never
                            // took, the value persisted into Prefs, and
                            // every bake site would have installed a
                            // new-edge raster under an old-edge canvas. The
                            // re-decoded source is dropped with it, so
                            // canvas and edge cannot disagree in either
                            // direction (a later switch after saving
                            // re-decodes the master properly).
                            self.workspace.genai_mut().preview_edge = e;
                            // The remedy is read off THIS CANVAS, never
                            // off the photo's record: a stale broken
                            // record (an ./out cleanup) plus a NEW unsaved
                            // retouch used to be told its situation was
                            // unfixable and denied the one cure that
                            // works. Three causes, three answers:
                            //  · a GENERATED canvas has no save a user can
                            //    be sent to press: Ctrl+S refuses it
                            //    outright (the Analyze auto-save and
                            //    Save-all DO record generated masters, so
                            //    "cannot be recorded" would be false), and
                            //    the message names the route that always
                            //    works instead;
                            //  · a master no longer ON DISK cannot be
                            //    re-decoded however often it is recorded
                            //    (saving would re-record the same broken
                            //    link and the refusal would repeat);
                            //  · otherwise the master is there and merely
                            //    unrecorded (or superseded since the last
                            //    save), which saving fixes.
                            // Residual, bounded and stderr-visible: a
                            // master that exists but will not DECODE takes
                            // the third arm, and `read_pixel_source` /
                            // the worker already name it on stderr.
                            let canvas_master =
                                self.editor.active_variant().and_then(|v| v.origin.clone());
                            let t = if self.editor.active_on_ai_pixels() {
                                tr(
                                    lang,
                                    K_CANVAS_PREVIEW_RESOLUTION_KEPT_A_GENERATED_VARIANT_S_PI,
                                )
                            } else if !canvas_master.as_deref().is_some_and(|o| o.exists()) {
                                tr(
                                    lang,
                                    K_CANVAS_PREVIEW_RESOLUTION_KEPT_THIS_CANVAS_S_RETOUCH_MA,
                                )
                            } else {
                                tr(
                                    lang,
                                    K_CANVAS_PREVIEW_RESOLUTION_KEPT_THIS_RETOUCHED_CANVAS_HA,
                                )
                            };
                            // The bar still reads "decoding … " (open_path
                            // wrote it and `busy` is already false): every
                            // other outcome of this landing replaces it, so
                            // the refusal must too — a finished decode
                            // announcing itself forever is the same lie one
                            // door along, on the surface that does not
                            // expire.
                            self.workspace.set_status(t.to_string());
                            self.workspace.toast(ToastKind::Error, t);
                        } else {
                            self.editor.photo_mut().source_preview = Some(base.clone());
                            self.workspace.open_mut().source_part = part.clone();
                            self.workspace.open_mut().source_part_inflight = None;
                            // Fresh decode = the EXIF display frame,
                            // i.e. turn 0 (`render_to_image` is given
                            // a NEUTRAL recipe). Declaring it here is
                            // what lets `sync_base_turns` put the plate
                            // into the recipe's own frame on the next
                            // frame — one mover for every path (R27).
                            self.editor.photo_mut().base_turns = 0;
                            if active_source {
                                let start = self.editor.doc().recipe.start_only();
                                self.editor.set_before(&mut self.workspace, ctx, &base, &start);
                                self.workspace.rebind_paint_canvas(self.lang, mw, mh);
                                // The range-mask reference develop was computed
                                // from the OLD base — recipe-only keying would
                                // serve it stale against these new pixels.
                                self.workspace.overlay_mut().overlay_ref = None;
                                self.workspace.overlay_mut().overlay_stale = true;
                                self.editor.photo_mut().base_preview = Some(base);
                            } else if master_hit
                                && let Some((bimg, _, _)) = &baked
                            {
                                // The active variant renders a RECORDED baked
                                // master: hand it the master re-decoded at the
                                // new preview edge too, or 1:1 inspection stays
                                // at the old resolution. History steps holding
                                // the old Arc are repointed — same master, new
                                // resolution — so Ctrl+Z can't flip the canvas
                                // back to the stale-res pixels.
                                let old = self.editor.doc().variants
                                    .get(self.editor.doc().active)
                                    .and_then(|v| v.base.clone());
                                if let Some(v) = self.editor.active_variant_mut() {
                                    v.base = Some(bimg.clone());
                                }
                                // Bound, stated instead of implied: only
                                // the Arc the canvas holds NOW is
                                // re-pointed. A step on a SUPERSEDED master
                                // (an earlier heal, replaced by this one)
                                // has no re-decoded twin — the worker
                                // resolves exactly one master — so undoing
                                // to it restores that raster at ITS own
                                // edge. That is why a bake follows the
                                // canvas (`canvas_edge`), not the
                                // preference.
                                if let Some(old) = old {
                                    let repoint = |s: &mut UndoStep| {
                                        let hit = s
                                            .base
                                            .as_ref()
                                            .is_some_and(|b| Arc::ptr_eq(b, &old));
                                        if hit {
                                            s.base = Some(bimg.clone());
                                        }
                                    };
                                    repoint(&mut self.editor.session_mut().hist.committed);
                                    self.editor.session_mut().hist.undo_stack.iter_mut().for_each(repoint);
                                    self.editor.session_mut().hist.redo_stack.iter_mut().for_each(repoint);
                                }
                                let (bw, bh) = bimg.dimensions();
                                // Canvas-curve Before, same rule as every
                                // restore path: an InPlace master is NEUTRAL
                                // (needs the camera curve); a Generated
                                // canvas recipe carries an empty curve.
                                let start = self.editor.doc().recipe.start_only();
                                self.editor.set_before(&mut self.workspace, ctx, bimg, &start);
                                self.workspace.rebind_paint_canvas(self.lang, bw, bh);
                                self.workspace.overlay_mut().overlay_ref = None;
                                self.workspace.overlay_mut().overlay_stale = true;
                                self.editor.photo_mut().base_preview = Some(bimg.clone());
                            }
                            self.workspace.chrome_mut().last_rgb = None; // resolution changed under the frame
                            self.editor.session_mut().dirty = true;
                            *self.workspace.status_mut() = trf(
                                lang,
                                K_CANVAS_PREVIEW_RESOLUTION_PX_RE_DECODED,
                                &[("px", &self.workspace.genai().preview_edge.to_string())],
                            );
                        }
                    } else {
                        // Fresh open: a single Original variant, all
                        // per-photo state reset. The recipe starts from the
                        // photo's SAVED develop when a ./out sidecar exists
                        // — the gallery badge already promises "● edited",
                        // so opening must honour it — neutral otherwise.
                        let (mw, mh) = base.dimensions();
                        // before_tex is built AFTER the recipe is settled
                        // below — the Before pane carries the canvas
                        // recipe's own base_curve.
                        self.editor.photo_mut().source_preview = Some(base.clone());
                        self.workspace.open_mut().source_part = part.clone();
                        self.workspace.open_mut().source_part_inflight = None;
                            // Fresh decode = the EXIF display frame,
                            // i.e. turn 0 (`render_to_image` is given
                            // a NEUTRAL recipe). Declaring it here is
                            // what lets `sync_base_turns` put the plate
                            // into the recipe's own frame on the next
                            // frame — one mover for every path (R27).
                            self.editor.photo_mut().base_turns = 0;
                        self.editor.photo_mut().base_preview = Some(base);
                        let RestoredDevelop {
                            saved,
                            xmp_bad,
                            mask_import,
                            imported_masks,
                            carried_globals,
                            clamp: clamp_dropped,
                            lr_unreadable,
                            packet_unreadable,
                        } = self.editor.photo().src_path
                            .as_deref()
                            .map(read_saved_develop)
                            .unwrap_or_default();
                        // W20: restore-time sanitisation is not allowed to
                        // be silent loss — a stored recipe past the caps
                        // opens minus edits, and the user must hear it.
                        if !clamp_dropped.is_empty() {
                            // All four loss kinds — a curve-only or
                            // string-only truncation used to toast
                            // "0 mask(s) and 0 component(s)" (16-lane
                            // scan L14/L16).
                            let t = trf(
                                lang,
                                K_CANVAS_RECIPE_LIMITS_DISCARDED_MASK_S_COMPONENT_S_CURVE,
                                &[
                                    ("n", &clamp_dropped.dropped_masks.to_string()),
                                    ("m", &clamp_dropped.dropped_components.to_string()),
                                    ("c", &clamp_dropped.truncated_curve_points.to_string()),
                                    ("s", &clamp_dropped.truncated_string_bytes.to_string()),
                                ],
                            );
                            self.workspace.toast(ToastKind::Error, t);
                        }
                        // What the sidecar's masks cost on the way in.
                        // R25 P1 replaced a COUNT of refusals with the
                        // reader's own named verdicts: the old line said
                        // "N have no engine equivalent" about corrections
                        // that were mostly ordinary radials and
                        // gradients, refused over a rotation angle and a
                        // default blend mode. Now they import, and the
                        // sentence names whatever really did not come
                        // with them. The sidecar is untouched either way.
                        if let Some(t) =
                            crate::util::xmp_import_line(lang, imported_masks, &mask_import)
                        {
                            self.workspace.toast(ToastKind::Error, t);
                        }
                        // R24-5 M0, the GLOBAL half of the same story: the
                        // sidecar's own crs: properties this engine does
                        // not model. The merge keeps every one of them, so
                        // nothing is lost — but the canvas will not match
                        // Lightroom's render, and until now nothing said
                        // why. Named, not counted: 「Texture」 is the whole
                        // answer, 「3 settings」 is not. Capped like the
                        // mask-loss line, for the same reason.
                        if !carried_globals.is_empty() {
                            let shown = carried_globals.len().min(4);
                            let more = carried_globals.len() - shown;
                            let mut list = carried_globals[..shown].join(", ");
                            if more > 0 {
                                list.push_str(&format!(
                                    ", {}",
                                    trf(lang, K_CANVAS_MORE, &[("n", &more.to_string())])
                                ));
                            }
                            let t = trf(
                                lang,
                                K_CANVAS_THIS_LIGHTROOM_SIDECAR_CARRIES_GLOBAL_SETTING_S,
                                &[("n", &carried_globals.len().to_string()), ("list", &list)],
                            );
                            self.workspace.toast(ToastKind::Error, t);
                        }
                        // The decision half lives in persist::resolve_saved_develop —
                        // this handler applies it to the canvas. Two reasons to skip
                        // the repair decode fold here: a GENERATED canvas strips its
                        // calibration below, and a session stash about to override the
                        // canvas wholesale would discard the repair (dirty_vs keeps ●
                        // safe in both directions).
                        let skip_repair = baked.as_ref().is_some_and(|(_, _, g)| *g)
                            || self.editor.photo().src_path
                                .as_ref()
                                .is_some_and(|p| self.workspace.open().nav_stash.contains_key(p));
                        let ResolvedSaved {
                            mut recipe,
                            restored,
                            stamp,
                            mut open_note,
                            unresolved,
                        } = resolve_saved_develop(
                            lang,
                            saved,
                            skip_repair,
                            self.editor.photo().src_path.as_deref(),
                        );
                        if unresolved {
                            self.workspace.open_mut().open_unresolved = true;
                        }
                        // A6 disclosure: numeric settings in the restored
                        // XMP that do not parse imported as SILENT
                        // neutrals — and the next save would write those
                        // neutrals back over the sidecar. Say so on open.
                        if !xmp_bad.is_empty() {
                            let w = trf(
                                lang,
                                K_CANVAS_XMP_NUMERIC_SETTING_S_UNREADABLE_RESTORED_AS_NEU,
                                &[
                                    ("n", &xmp_bad.len().to_string()),
                                    ("list", &xmp_bad.join(", ")),
                                ],
                            );
                            open_note = merge_note(open_note, w);
                        }
                        if let Some(why) = lr_unreadable {
                            let w = trf(
                                lang,
                                K_CANVAS_A_LIGHTROOM_SIDECAR_SITS_BESIDE_THIS_PHOTO_BUT_C,
                                &[("why", why)],
                            );
                            open_note = merge_note(open_note, w);
                        }
                        // Same unreadable-is-not-absent rule for the
                        // packet INSIDE the RAW — only probed when no
                        // other source answered, so this fires exactly
                        // when whatever it holds would have been shown.
                        if let Some(why) = packet_unreadable {
                            let w = trf(
                                lang,
                                K_CANVAS_THIS_RAW_CARRIES_AN_EMBEDDED_XMP_DEVELOP_THAT_CO,
                                &[("why", &why)],
                            );
                            open_note = merge_note(open_note, w);
                        }
                        if stamp {
                            stamp_calibration(&mut recipe, &start, &lens, as_shot);
                        } else if recipe.base_curve.is_empty()
                            && !start.is_empty()
                            && open_note.is_none()
                        {
                            // A legacy save renders exactly as it was
                            // tuned — on the old dark base. Say so, and
                            // point at the way out, or the photo just
                            // looks broken next to fresh opens.
                            open_note = Some(
                                tr(
                                    lang,
                                    K_CANVAS_SAVED_BEFORE_THIS_VERSION_S_STARTING_LOOK_RENDER,
                                )
                                .into(),
                            );
                        }
                        // The saved develop is the ● baseline; a canvas
                        // stashed when the user navigated away THIS session
                        // outranks it — that stash is their newest work.
                        self.editor.session_mut().hist.saved_recipe = recipe.clone();
                        let mut from_stash = false;
                        // The stash (this session's newest work) outranks
                        // disk WHOLESALE — recipe and pixel identity both:
                        // a stashed source-based canvas (e.g. the retouch
                        // was undone) must not resurrect disk pixels.
                        // Disk truth for the per-frame ● comparison —
                        // captured BEFORE the stash override below.
                        self.editor.session_mut().hist.pixels_on_disk = baked.as_ref().map(|(_, o, _)| o.clone());
                        // A RECORDED master that arrived as None failed to
                        // decode/resolve in the worker (it only eprintln'd
                        // there) — a desktop-launched GUI would otherwise
                        // show an apparently normal un-retouched open with
                        // no in-app trace.
                        // has_pixel_source, NOT read_pixel_source: the
                        // latter answers None for "nothing recorded" AND
                        // for "recorded but broken", so asking it again
                        // here made the toast unreachable for the two
                        // commonest causes — a deleted/moved master and a
                        // corrupt pixels.json. Those are precisely the
                        // cases the user must be told about.
                        if baked.is_none()
                            && let Some(p) = self.editor.photo().src_path.as_deref()
                            && autoshade::store::has_pixel_source(p)
                        {
                            let t = tr(
                                lang,
                                K_CANVAS_THE_SAVED_RETOUCH_MASTER_COULD_NOT_BE_LOADED_OPE,
                            );
                            self.workspace.toast(ToastKind::Error, t);
                        }
                        let mut pixels: Option<BakedBase> = baked;
                        // The stashed active card's master when its decode
                        // was still IN FLIGHT at navigate-away: origin
                        // known, pixels not yet — the identity must
                        // survive the reopen even though the image is not
                        // here yet.
                        let mut pending_master: Option<PathBuf> = None;
                        let mut stash_others: Vec<StashedVariant> = Vec::new();
                        let mut stash_active_pos = 0usize;
                        // The ACTIVE card's three-valued kind, whichever
                        // authority supplies it: the session stash below,
                        // or the persisted strip record read further down
                        // (`Fitted` is source-based, so it has no pixels
                        // arm to ride back on — without this the card
                        // reopened renamed 「▣ 原片」).
                        let mut active_kind = VariantKind::Original;
                        // …and its identity + name, from the same two
                        // authorities (R24-2, hop 6 of 6). The default is
                        // the FIXED base-negative id: the common photo
                        // has one card and no strip record at all, and a
                        // freshly minted id there would leave every
                        // version it ever took unattributed on reopen.
                        let mut active_id = ORIGINAL_VARIANT_ID.to_string();
                        let mut active_name: Option<String> = None;
                        if let Some(st) =
                            self.editor.photo().src_path.as_ref().and_then(|p| self.workspace.open_mut().nav_stash.remove(p))
                        {
                            recipe = st.recipe;
                            active_kind = st.kind;
                            active_id = st.id;
                            active_name = st.name;
                            pixels = match (st.base, st.origin) {
                                // The bool is the `pixels.json` flag —
                                // `VariantKind::on_ai_pixels`, the axis
                                // the flag records (a ✨ card and its ✎
                                // cards sit on the same raster).
                                (Some(b), Some(o)) => {
                                    Some((b, o, st.kind.on_ai_pixels()))
                                }
                                // Navigated away while the cold master was
                                // still decoding: the ORIGIN is the pixel
                                // link's identity — dropping it here let
                                // the next Ctrl+S clear pixels.json and
                                // persist a generated recipe as a source-
                                // based develop. The base re-decodes
                                // below; the link and the kind survive.
                                (None, Some(o)) => {
                                    pending_master = Some(o);
                                    None
                                }
                                _ => None,
                            };
                            stash_others = st.others;
                            stash_active_pos = st.active_pos;
                            from_stash = true;
                            // The stash carries the strip VERBATIM — and
                            // when the ORIGINAL open's repair was an
                            // inability, those recipes still hold the
                            // washed era-1 curve. Inabilities exist to
                            // be retried by the next reader, and the
                            // gate above deliberately skipped the
                            // disk-recipe repair in deference to this
                            // override — so the retry happens HERE, on
                            // every recipe that can land on the canvas:
                            // the active one AND each sibling (a strip
                            // entry is one click from BEING the canvas).
                            // The washed-sibling population is ORDINARY,
                            // not residual: push_variant/switch_variant
                            // sync the OUTGOING canvas into the strip,
                            // so a washed Original displaced by an AI
                            // push is a washed sibling (the freshly
                            // PUSHED variants themselves are era-2 and
                            // empty by construction). load_active
                            // repairs on switch as the last line of
                            // defence; this eager pass heals the whole
                            // strip the moment it is restored. ONE
                            // estimate total — the memo keys on the
                            // photo — and already-repaired recipes
                            // short-circuit on their era stamp.
                            // A failed estimate retries on the next
                            // return; the deterministic-inability
                            // population is empty for estimator-written
                            // curves (the too-few-pixels guard is
                            // coeval with base_curve itself). Only
                            // PARAMETRIC cards are repaired (R24-1): a
                            // pixel-state card's curve is empty by
                            // invariant. These two are 2 of the 6 sites
                            // that spelled that binary by hand as
                            // `!= Generated`; they ask
                            // `VariantKind::is_source_based` now, so a
                            // new kind cannot inherit the answer.
                            if let Some(p) = self.editor.photo().src_path.clone() {
                                let mut relooked = active_kind.is_source_based()
                                    && autoshade::pipeline::repair_pre_era_base_curve(
                                        &p, &mut recipe,
                                    )
                                    .is_some();
                                for sv in &mut stash_others {
                                    if sv.kind.is_source_based() {
                                        relooked |=
                                            autoshade::pipeline::repair_pre_era_base_curve(
                                                &p,
                                                &mut sv.recipe,
                                            )
                                            .is_some();
                                    }
                                }
                                if relooked {
                                    let w = tr(
                                        lang,
                                        K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                                    )
                                    .to_string();
                                    open_note = merge_note(open_note, w);
                                }
                            }
                        }
                        // The persisted strip record — the disk half of
                        // the session stash. Read even when the stash
                        // outranks it wholesale: it is also the mirror
                        // the background-variant dirty test compares
                        // against (see `open_dirty_variants`).
                        let disk_strip = match self.editor.photo().src_path
                            .as_deref()
                            .map(autoshade::store::read_variants_checked)
                        {
                            Some(autoshade::store::VariantsRead::Strip(rec)) => Some(rec),
                            Some(autoshade::store::VariantsRead::Unresolved) => {
                                // The store's save floor refuses to touch
                                // an unresolved strip — say so at OPEN,
                                // not first at the failing save.
                                let t = tr(
                                    lang,
                                    K_CANVAS_THIS_PHOTO_S_VARIANT_LIST_VARIANTS_JSON_IN_ITS_D,
                                )
                                .to_string();
                                self.workspace.toast(ToastKind::Error, t);
                                None
                            }
                            _ => None,
                        };
                        if !from_stash && let Some(rec) = &disk_strip {
                            if rec.active_kind == "fitted" {
                                // Fitted is source-based — its pixels arm,
                                // when it has one (the negative's in-place
                                // master, inherited at the fit), says
                                // `inplace` like the ▣ card's and cannot
                                // name the kind; without this the card
                                // cold-reopened renamed 「▣ 原片」. A recorded
                                // "generated" needs no hand here: the baked
                                // pixels arm below upgrades the card exactly
                                // when the master really decoded.
                                active_kind = VariantKind::Fitted;
                            } else if rec.active_kind == "edited" {
                                // An edited card rides the pixels arm
                                // below like the generated card it forked
                                // from (the same AI raster); the arm needs
                                // the WORD to keep it from reopening as
                                // the pristine ✨ card.
                                active_kind = VariantKind::Edited;
                            }
                            // The persisted strip is also where the active
                            // card's identity + name live (R24-2); a record
                            // written before those fields existed mints one.
                            active_id = variant_id_or_mint(
                                rec.active_id.as_ref(),
                            );
                            active_name = rec.active_name.clone();
                        }
                        self.editor.doc_mut().recipe = recipe.clone();
                        self.editor.analysis_mut().rationale = recipe.rationale.clone();
                        self.editor.analysis_mut().rationale_notes.clear(); // disk restore carries no typed notes
                        self.editor.doc_mut().variants = vec![Variant {
                            // A PENDING master keeps its stashed kind: the
                            // Fitted-or-Original collapse renamed a still-
                            // decoding Generated card to 「▣ 原片」, and a
                            // save then persisted that lie.
                            kind: if pending_master.is_some() {
                                active_kind
                            } else if active_kind == VariantKind::Fitted {
                                VariantKind::Fitted
                            } else {
                                VariantKind::Original
                            },
                            id: active_id,
                            name: active_name,
                            recipe,
                            base: None,
                            origin: None,
                            thumb: None,
                        }];
                        self.editor.doc_mut().active = 0;
                        // Before = this photo's starting point under the
                        // canvas recipe's own base calibration: stamped
                        // fresh opens compare bright, legacy saves compare
                        // as originally tuned.
                        if let Some(b) = self.editor.photo().base_preview.clone() {
                            let start = self.editor.doc().recipe.start_only();
                            self.editor.set_before(&mut self.workspace, ctx, &b, &start);
                        }
                        // A fresh, fully-transparent paint mask sized to the
                        // preview — and a brush session armed on the PREVIOUS
                        // photo dies with its plate (this fresh-open arm never
                        // called disarm_tools; the always-visible 「✓ Apply」
                        // would have baked the old photo's strokes into this
                        // photo's mask list).
                        self.workspace.rebind_paint_canvas(self.lang, mw, mh);
                        self.workspace.masks_mut().paint_mode = false;
                        if let Some((bimg, borigin, generated)) = pixels {
                            // Restore the baked pixel master (persisted
                            // pixels.json, or this session's stash): the
                            // canvas sits on the retouched pixels again
                            // with the recipe rendering on top. An
                            // in-place master is a NEUTRAL develop — the
                            // recipe's base_curve legitimately renders the
                            // camera look on top of it; Before compares
                            // curve-less, exactly like the InPlace flow
                            // that made it. A GENERATED master's look
                            // already lives in its pixels, so calibration
                            // must be STRIPPED from the canvas recipe
                            // (same rule as analyzing a baked variant) or
                            // curve + lens geometry would cook it twice.
                            let (bw, bh) = bimg.dimensions();
                            if let Some(v) = self.editor.doc_mut().variants.get_mut(0) {
                                v.base = Some(bimg.clone());
                                v.origin = Some(borigin);
                                if generated {
                                    // The record's word tells the pristine
                                    // ✨ card from a ✎ develop over the
                                    // same raster; a record that predates
                                    // the split is normalised below.
                                    v.kind = if active_kind == VariantKind::Edited {
                                        VariantKind::Edited
                                    } else {
                                        VariantKind::Generated
                                    };
                                }
                            }
                            if generated {
                                self.editor.doc_mut().recipe.strip_baked_start();
                                self.editor.doc_mut().recipe.as_shot_k = None;
                                self.editor.doc_mut().recipe.as_shot_tint = None;
                                let recipe = self.editor.doc().recipe.clone();
                                if let Some(v) = self.editor.doc_mut().variants.get_mut(0) {
                                    v.recipe = recipe;
                                }
                                // The ● baseline lives in CANVAS
                                // coordinates (same rule as the Analyze
                                // saver): the disk recipe keeps its
                                // calibration, but comparing the stripped
                                // canvas against it lit ● on lens_profile
                                // the instant a clean generated save
                                // opened.
                                self.editor.session_mut().hist.saved_recipe.strip_baked_start();
                                self.editor.session_mut().hist.saved_recipe.as_shot_k = None;
                                self.editor.session_mut().hist.saved_recipe.as_shot_tint = None;
                            }
                            // Before under the canvas recipe's own curve:
                            // empty for a Generated master (stripped just
                            // above — its pixels carry the look); the
                            // camera curve for an InPlace master, whose
                            // pixels are a NEUTRAL develop (an empty curve
                            // there compared 0.6–1.4 EV dark).
                            let start = self.editor.doc().recipe.start_only();
                            self.editor.set_before(&mut self.workspace, ctx, &bimg, &start);
                            self.editor.photo_mut().base_preview = Some(bimg);
                            self.workspace.rebind_paint_canvas(self.lang, bw, bh);
                        }
                        if let Some(o) = pending_master.clone() {
                            // Re-attach the surviving identity and re-arm
                            // the worker decode (coalesced if the earlier
                            // one is somehow still in flight). Until it
                            // lands the canvas shows the source develop
                            // under spawn_master_load's disclosure.
                            if let Some(v) = self.editor.doc_mut().variants.get_mut(0) {
                                v.origin = Some(o.clone());
                            }
                            if let Some(photo) = self.editor.photo().src_path.clone() {
                                self.spawn_master_load(ctx, photo, o);
                            }
                        }
                        // Restore the BACKGROUND variants the stash
                        // carried (H4): the strip used to collapse to
                        // the active canvas alone, silently dropping
                        // every other variant's unsaved work.
                        if from_stash && !stash_others.is_empty() {
                            let active_v = self.editor.doc_mut().variants.remove(0);
                            let mut strip: Vec<Variant> = stash_others
                                .drain(..)
                                .map(|sv| Variant {
                                    kind: sv.kind,
                                    // Hop 4 of 6 (R24-2): the stash back
                                    // into the live strip.
                                    id: sv.id,
                                    name: sv.name,
                                    recipe: sv.recipe,
                                    base: sv.base,
                                    origin: sv.origin,
                                    thumb: None,
                                })
                                .collect();
                            let pos = stash_active_pos.min(strip.len());
                            strip.insert(pos, active_v);
                            self.editor.doc_mut().variants = strip;
                            self.editor.doc_mut().active = pos;
                        } else if !from_stash
                            && let Some(rec) = &disk_strip
                            && !rec.others.is_empty()
                        {
                            // Cold restore of the persisted strip: the
                            // background variants come back with their
                            // recipes and raster origins; base pixels
                            // re-decode lazily on first switch
                            // (`load_active`), the same deal a stash
                            // restore gets from its held Arcs. Pre-era
                            // curves heal exactly like the stash arm
                            // above — a strip entry is one click from
                            // BEING the canvas. (dirty_vs neutralises
                            // version/base_curve, so healing the live
                            // strip never lights ● against the mirror.)
                            let active_v = self.editor.doc_mut().variants.remove(0);
                            let (mut strip, strip_dropped) =
                                strip_from_record(rec, self.editor.photo().src_path.as_deref());
                            let pos = rec.active_pos.min(strip.len());
                            strip.insert(pos, active_v);
                            self.editor.doc_mut().variants = strip;
                            self.editor.doc_mut().active = pos;
                            // The strip's own W20 disclosure, beside the
                            // active card's above: a card past the caps
                            // is one click from being the canvas.
                            if !strip_dropped.is_empty() {
                                let t = trf(
                                    lang,
                                    K_CANVAS_RECIPE_LIMITS_DISCARDED_MASK_S_COMPONENT_S_CURVE__2,
                                    &[
                                        ("n", &strip_dropped.dropped_masks.to_string()),
                                        ("m", &strip_dropped.dropped_components.to_string()),
                                        ("c", &strip_dropped.truncated_curve_points.to_string()),
                                        ("s", &strip_dropped.truncated_string_bytes.to_string()),
                                    ],
                                );
                                self.workspace.toast(ToastKind::Error, t);
                            }
                        }
                        // The AI-card invariant, restored at the door
                        // (2026-09-13): a record that predates the split
                        // — the ✨ card edited in place, the way every
                        // build through v1.3.2 stored it — is normalised
                        // into the live strip as UNSAVED work (the door
                        // does not write), and said once.
                        if self.normalize_ai_cards() {
                            self.workspace.toast(
                                ToastKind::Success,
                                tr(
                                    lang,
                                    K_CANVAS_THIS_PHOTO_S_AI_IMAGE_AND_THE_EDITS_OVER_IT_NOW,
                                ),
                            );
                        }
                        self.editor.session_mut().hist.saved_strip = disk_strip;
                        // The strip is settled: each card's "Keep skin
                        // natural" is decided against the edit just read
                        // (`panels/portrait.rs`) — opening never changes
                        // a value.
                        self.editor.decide_keep_skin_for_photo(&mut self.workspace);
                        self.workspace.chrome_mut().last_rgb = None; // retained frame was the old photo's
                        self.editor.reset_history(&self.workspace); // a new photo starts a fresh undo history
                        self.workspace.overlay_mut().region = None; // and a fresh local-edit selection
                        self.workspace.overlay_mut().region_drag = None;
                        // View + tool state is per-photo.
                        self.workspace.view_mut().zoom = 1.0;
                        self.workspace.view_mut().zoom_target = 1.0; // instant — no glide across photos
                        self.workspace.view_mut().pan = vec2(0.5, 0.5);
                        self.workspace.disarm_tools(&mut self.editor);
                        // The \-latch is per-photo transient state:
                        // carried across an open, the new photo arrived
                        // with every tool locked out (M21).
                        self.workspace.view_mut().before = false;
                        self.workspace.masks_mut().sel_mask = None;
                        self.workspace.masks_mut().sel_component = None;
                        self.workspace.overlay_mut().overlay_ref = None; // the reference develop belongs to ONE base
                        self.workspace.overlay_mut().overlay_stale = true;
                        self.workspace.set_curve_drag(None); // curve_channel is a UI pref, keep it
                        self.workspace.tools_mut().wb_picking = false;
                        self.workspace.tools_mut().range_picking = None;
                        self.workspace.tools_mut().clone_mode = false;
                        self.workspace.tools_mut().clone_src = None;
                        self.editor.analysis_mut().verdict = None;
                        // R23-6 B: the chosen reverse-fit reference names
                        // a rendition of THE PREVIOUS photo. Carried
                        // across an open it would silently fit this photo
                        // against the last one's export — the exact
                        // "reference is not this frame" case the fit can
                        // now only warn about.
                        self.workspace.fit_mut().fit_ref = None;
                        self.workspace.fit_mut().fit_from = None; // …and so does a picked source card
                        self.workspace.fit_mut().fit_prompt = None; // …and the question about one
                        // (rationale already restored alongside the recipe)
                        self.refresh_versions(); // version snapshots are per-photo
                        self.editor.session_mut().dirty = true; // render the (restored or neutral) after
                        *self.workspace.status_mut() = if from_stash {
                            tr(
                                lang,
                                K_CANVAS_READY_RESTORED_THIS_SESSION_S_UNSAVED_EDITS_NOT,
                            )
                            .into()
                        } else {
                            match restored {
                                Some(kind) => trf(
                                    lang,
                                    K_CANVAS_READY_RESTORED_SAVED_EDITS_RESET_RETURNS_TO_NEUT,
                                    &[("kind", kind)],
                                ),
                                None => {
                                    tr(lang, K_CANVAS_READY_ADJUST_SLIDERS_OR_RUN_AI_ANALYZE).into()
                                }
                            }
                        };
                        // The third door: a stash restore reinstalls the
                        // raster this photo was LEFT with, at its own
                        // edge, while this open decoded the source at the
                        // current preference — which may have moved while
                        // the user was on another photo.
                        if let Some(px) = self.editor.baked_canvas_edge(&self.workspace) {
                            let extra = trf(
                                lang,
                                K_CANVAS_RESTORED_PIXELS_STAY_AT_PX_THEIR_OWN_BAKE,
                                &[("px", &px.to_string())],
                            );
                            self.workspace.status_mut().push_str(&extra);
                        }
                        if let Some(note) = open_note {
                            // Sidecar anomalies must be seen, not scrolled
                            // away in the status line.
                            self.workspace.toast(ToastKind::Error, note);
                        }
                    }
                }
                Err(e) => {
                    self.workspace.fail(tr(lang, K_CANVAS_COULD_NOT_OPEN), e);
                    if !keep {
                        // A FRESH open failed: open_path already re-pointed
                        // src_path to the file that wouldn't decode, but the
                        // previous photo's variants / pixels are still live —
                        // a mismatch that would misdirect a later fit / heal /
                        // XMP (they key off src_path). Drop to a clean
                        // no-photo state so src_path and the variants can never
                        // disagree. (A preview-res re-decode failure — keep —
                        // leaves the still-open photo untouched.)
                        self.editor.photo_mut().src_path = None;
                        self.editor.doc_mut().variants.clear();
                        self.editor.doc_mut().active = 0;
                        self.editor.photo_mut().base_preview = None;
                        self.editor.photo_mut().source_preview = None;
                        self.workspace.open_mut().source_part = Default::default();
                        self.workspace.open_mut().source_part_inflight = None;
                        self.editor.plates_mut().before_tex = None;
                        self.editor.plates_mut().after_tex = None;
                        self.workspace.gal_mut().selected = None;
                        // …and its HISTORY, which this arm used to leave
                        // standing: the steps hold the dead photo's
                        // rasters and its recipe, so an undo here restored
                        // a canvas that no longer exists (the keyboard
                        // path reached it because it checked only `busy`).
                        // Gating the key is the guard; clearing the stack
                        // is the reason there is nothing to guard. The
                        // photo's unsaved work is NOT lost — open_path
                        // stashed it before the flight.
                        self.editor.reset_history(&self.workspace);
                    } else if let Some(p) = self.editor.photo().src_path.clone() {
                        // A preview-res re-decode failed: the photo (and
                        // its live canvas) is untouched — consume the
                        // stash entry the same-path open_path call just
                        // wrote, exactly like the successful keep branch.
                        // Left behind, an undo-to-clean before the next
                        // navigation resurrected these pre-undo edits on
                        // the way back (CX4-4).
                        self.workspace.open_mut().nav_stash.remove(&p);
                        // ...and the switch never materialized: the
                        // canvas kept the OLD edge, so preview_edge
                        // reverts with it (the busy-refusal rule) —
                        // leaving the new value displayed an unreachable
                        // resolution and fed it to every bake site under
                        // an old-edge canvas.
                        if let Some(e) = edge_revert {
                            self.workspace.genai_mut().preview_edge = e;
                        }
                    }
                }
            }
}


pub(crate) fn on_base_redeveloped(
    &mut self,
    ctx: &Ctx,
    path: PathBuf,
    part: autoshade::render::BasePart,
    img: anyhow::Result<Arc<image::DynamicImage>>,
) {
    if self.workspace.open().source_part_inflight.as_ref() == Some(&part) {
        self.workspace.open_mut().source_part_inflight = None;
    }
    if self.editor.photo().src_path.as_deref() != Some(path.as_path()) {
        return;
    }
    let img = match img {
        Ok(i) => i,
        Err(e) => {
            let t = trf(
                self.lang,
                K_CANVAS_THE_CANVAS_COULD_NOT_BE_DEVELOPED_UNDER_THIS_PHO,
                &[("err", &format!("{e:#}"))],
            );
            self.workspace.toast(ToastKind::Error, t);
            // Said once, then the canvas shows over the base there is:
            // asking again on every frame would repeat a develop that
            // has already failed.
            self.workspace.open_mut().source_part = part;
            self.editor.session_mut().dirty = true;
            return;
        }
    };
    let turns = self.editor.photo().base_turns % 4;
    let img = if turns == 0 {
        img
    } else {
        Arc::new(autoshade::render::turn_image((*img).clone(), turns))
    };
    self.editor.photo_mut().source_preview = Some(img.clone());
    self.workspace.open_mut().source_part = part;
    if self.editor.active_variant().is_none_or(|v| v.base.is_none()) {
        self.editor.photo_mut().base_preview = Some(img.clone());
        let start = self.editor.doc().recipe.start_only();
        self.editor.set_before(&mut self.workspace, ctx, &img, &start);
        let (mw, mh) = img.dimensions();
        self.workspace.rebind_paint_canvas(self.lang, mw, mh);
        self.workspace.overlay_mut().overlay_ref = None;
        self.workspace.overlay_mut().overlay_stale = true;
        self.workspace.chrome_mut().last_rgb = None;
    }
    self.editor.session_mut().dirty = true;
}
}
