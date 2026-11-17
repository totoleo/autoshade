//! Snapshot (version) commands: path naming, mask-index remapping when a list
//! changes, the version list refresh, save/load and the delete confirm. Same
//! `AutoShadeApp` impl as `actions.rs`, split by domain.

use crate::*;

impl Commands<'_> {


    /// Rescan the photo's develop dir for version snapshots (cached in
    /// `self.workspace.cards().versions`; called on photo open and after saving a version — NOT
    /// every frame). The advisory name/provenance sidecar is read in the same
    /// pass and RESTRICTED to numbers that are actually listed: a crash
    /// between a delete's sweep and its metadata drop can leave a record for
    /// a number nothing lists any more, and that record must never surface as
    /// a phantom row (it can never be re-attached either — the number is
    /// burned in the deleted-version registry).
    pub(crate) fn refresh_versions(&mut self) {
        self.workspace.cards_mut().versions.clear();
        self.workspace.cards_mut().version_meta.clear();
        let Some(src) = self.editor.photo().src_path.as_deref() else { return };
        // ONE store query for the photo's whole edit-state list (R24-4
        // `store::list_edits`), which joins the version family with its
        // advisory metadata — including the listed-numbers-only restriction
        // this used to spell for itself. The VARIANT half is deliberately
        // dropped here: it describes the LAST SAVED strip, while
        // `self.editor.doc().variants` is the live one (pushes, deletes and renames that
        // have not been saved yet), and an editor must never renumber its own
        // cards from a stale record. For every non-GUI surface that half IS
        // the list.
        for e in autoshade::store::list_edits(src) {
            if let autoshade::store::EditStateKind::Version { n, from_kind, from_id, source } =
                e.state
            {
                self.workspace.cards_mut().versions.push(n);
                self.workspace.cards_mut().version_meta.insert(
                    n,
                    autoshade::store::VersionMetaEntry {
                        n,
                        name: e.name,
                        from_kind,
                        from_id,
                        origin: source,
                    },
                );
            }
        }
    }



    /// Save the CURRENT develop as the next numbered version snapshot.
    pub(crate) fn save_version(&mut self) {
        // A typed-but-uncommitted name belongs in the snapshot — every
        // persistence entry point flushes all of them (the U10 rule; L27).
        self.commit_pending_names();
        // The immutability rule's boundary discipline: an edit the frame
        // hook has not moved yet forks NOW, so the snapshot is attributed
        // to the ✎ card that holds it.
        self.fork_edited_card();
        let lang = self.lang;
        // The pristine ✨ card has no develop to snapshot — its recipe is
        // neutral by the immutability rule, so "＋ Save as version" would
        // store an empty recipe that restores to nothing (R24-2). An ✎ card
        // does: its develop over the AI pixels is what the snapshot holds
        // (stripped of calibration like every snapshot taken on AI pixels,
        // and re-stamped if it ever lands on the negative — R24-3).
        if self.editor.active_variant().is_some_and(|v| v.kind == VariantKind::Generated) {
            let t = tr(
                lang,
                K_ACTIONS_A_PRISTINE_AI_GENERATED_CARD_HAS_NO_DEVELOP_TO_S,
            );
            self.workspace.set_status(t.into());
            self.workspace.toast(ToastKind::Error, t);
            return;
        }
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        // Attribution for the record below, read before the lock closure
        // borrows `self`: which card this snapshot is a picture OF (R24-2).
        let from_kind = self.editor.active_variant().map(|v| v.kind.store_str().to_string());
        let from_id = self.editor.active_variant().and_then(|v| variant_id_field(&v.id));
        // ONE NoWait lock across claim → raster freeze → recipe write:
        // another process's delete_version sweeping the just-claimed slot
        // mid-snapshot is a real interleave — and this is a foreground
        // button, so busy must report, not hang the UI.
        let locked = autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || -> std::io::Result<()> {
        // ATOMICALLY reserved number (store::claim_version): disk-list+1
        // raced the auto-backup (and the web/CLI) to the same N — the later
        // publish silently replaced the earlier snapshot.
        let (n, vpath) = match autoshade::store::claim_version(&src) {
            Ok(v) => v,
            Err(e) => {
                let t = trf(lang, K_ACTIONS_SAVE_VERSION_FAILED, &[("err", &e.to_string())]);
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
                return Ok(());
            }
        };
        // Freeze referenced rasters under THIS version (the backup gate's
        // pair): a version whose recipe pointed at another version's frozen
        // raster (load v3 → save as v4) broke the moment v3 was deleted.
        let dev = autoshade::store::develop_dir(&src);
        let mut snap = self.editor.doc().recipe.clone();
        if let Err(e) = autoshade::store::snapshot_rasters(&mut snap, &dev, n) {
            let _ = std::fs::remove_file(&vpath); // release the claim
            let t = trf(lang, K_ACTIONS_SAVE_VERSION_FAILED, &[("err", &e.to_string())]);
            self.workspace.set_status(t.clone());
            self.workspace.toast(ToastKind::Error, t);
            return Ok(());
        }
        let res = autoshade::pipeline::write_recipe(&src, &snap, Some(vpath.clone()), autoshade::diag::stderr());
        if res.is_err() {
            // Release the claimed slot AND this call's frozen rasters — an
            // empty version must not pollute the list after a failed write.
            autoshade::store::rollback_frozen_rasters(&dev, n);
            let _ = std::fs::remove_file(&vpath);
        }
        match res {
            Ok(p) => {
                // Provenance BEFORE the rescan, so the fresh row already
                // carries「· 来自 …」. Best-effort by contract (R24-2): the
                // snapshot is on disk and complete — an advisory record that
                // could not be written must not turn a successful save into a
                // failure, it only leaves the version unattributed. The
                // develop lock is already held (reentrant).
                let _ = autoshade::store::record_version_meta(
                    &src,
                    &autoshade::store::VersionMetaEntry {
                        n,
                        name: None,
                        from_kind: from_kind.clone(),
                        from_id: from_id.clone(),
                        origin: Some(autoshade::store::VERSION_ORIGIN_USER.to_string()),
                    },
                );
                self.refresh_versions();
                *self.workspace.status_mut() = trf(
                    lang,
                    K_ACTIONS_VERSION_V_SAVED,
                    &[("n", &n.to_string()), ("path", &p.display().to_string())],
                );
            }
            Err(e) => {
                let t = trf(lang, K_ACTIONS_SAVE_VERSION_FAILED, &[("err", &e.to_string())]);
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
            }
        }
                Ok(())
            },
        );
        if let Err(e) = locked {
            self.workspace.persist_postponed(self.lang, &e, "Save version failed: {err}", &[]);
        }
    }

    /// Load version `n` as the working recipe (one undo step, like AI Analyze).
    pub(crate) fn load_version(&mut self, n: u32) {
        let lang = self.lang;
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let p = version_path(&src, n);
        // ONE NoWait lock across the whole snapshot read: the version file,
        // the mask re-anchor probes and the raster detach copies are separate
        // file touches, and delete_version sweeps v<N>.* rasters FIRST and
        // removes the recipe LAST — an unlocked load could parse a recipe
        // whose rasters were already swept and the next Ctrl+S then persists
        // dangling paths. Foreground button ⇒ NoWait (the save_version rule);
        // a busy develop reports and leaves the canvas untouched. The clamp
        // toast and the pre-era repair (a seconds-long RAW decode) run
        // OUTSIDE the lock — the read-under-lock / decide-outside split
        // read_saved_develop already draws.
        let locked = autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || -> std::io::Result<anyhow::Result<(EditRecipe, autoshade::recipe::ClampSummary)>> {
                Ok((|| {
                    // A stale UI row can still name a half-deleted version:
                    // complete pending recovery (killed deletes included)
                    // under OUR lock before reading — reentrant, so this
                    // costs no second kernel acquisition (L03).
                    let _ = autoshade::store::recover_orphan_baks(&src);
                    let s =
                        autoshade::store::read_text_capped(&p, autoshade::store::MAX_STORE_JSON)?;
                    let mut r = serde_json::from_str::<EditRecipe>(&s)?;
                    let dropped = r.clamp();
                    // Snapshots name their rasters by bare file name, like
                    // the working recipe — re-anchor them to the develop dir.
                    if let Some(base) = p.parent() {
                        autoshade::store::resolve_mask_paths(&mut r, base);
                    }
                    // Then DETACH from the snapshot: the frozen v<N>.*.png
                    // files belong to that version and `delete_version`
                    // sweeps them, so a canvas pointing at them lost its
                    // masks the moment the user deleted the version it was
                    // loaded from — and the next save persisted the dangling
                    // path. The loaded state gets its own claimed copies.
                    autoshade::store::detach_rasters(&src, &mut r, "mask-restored");
                    Ok((r, dropped))
                })())
            },
        );
        let outcome = match locked {
            Ok(inner) => inner,
            Err(e) => {
                // The uniform busy/IO mapper every foreground compound uses.
                return self.workspace.persist_postponed(self.lang, 
                    &e,
                    "Load v{n} failed: {err}",
                    &[("n", &n.to_string())],
                );
            }
        };
        match outcome {
            Ok((mut r, dropped)) => {
                if !dropped.is_empty() {
                    // Same W20 disclosure as the open restore: a snapshot
                    // past the caps loads minus edits, never silently —
                    // and all four loss kinds, like the open toast.
                    let t = trf(
                        lang,
                        K_CANVAS_RECIPE_LIMITS_DISCARDED_MASK_S_COMPONENT_S_CURVE,
                        &[
                            ("n", &dropped.dropped_masks.to_string()),
                            ("m", &dropped.dropped_components.to_string()),
                            ("c", &dropped.truncated_curve_points.to_string()),
                            ("s", &dropped.truncated_string_bytes.to_string()),
                        ],
                    );
                    self.workspace.toast(ToastKind::Error, t);
                }
                // A Generated variant's pixels already carry the camera look
                // AND the lens corrections — a source-based snapshot's
                // calibration would cook both twice (same strip rule as the
                // open-restore and Analyze paths).
                // BOTH directions of the calibration rule, through their one
                // owner (R24-3 `reconcile_snapshot_calibration`): stripped
                // onto a pixel-state card, and — the direction nothing
                // covered — re-stamped when a snapshot taken OFF one lands on
                // the negative with no camera base look at all. The photo's
                // estimate is a RAW decode, so it is paid lazily, inside the
                // branch that needs it.
                let restamped = reconcile_snapshot_calibration(
                    &mut r,
                    self.editor.active_on_ai_pixels(),
                    || {
                        let (k, t) = autoshade::pipeline::fresh_as_shot_wb(&src);
                        (
                            autoshade::pipeline::fresh_canvas_start(&src, &autoshade::diag::photo(&src)),
                            autoshade::pipeline::fresh_lens_profile(&src),
                            k.zip(t),
                        )
                    },
                );
                // A version snapshot is a saved recipe like any other, and
                // it predates the era stamp by definition — loading one put
                // an unrepaired washed curve straight onto the canvas and
                // into everything exported from it. AFTER the generated
                // strip: a generated canvas keeps no base curve, so there is
                // nothing to repair (paying the estimate just to discard it,
                // and disclosing a re-estimate the strip then deleted, were
                // both wrong). The note rides the SAME status write as the
                // load announcement — a separate earlier write was replaced
                // one statement later, so the promised disclosure never
                // survived to a single rendered frame.
                let relook =
                    autoshade::pipeline::repair_pre_era_base_curve(&src, &mut r).is_some();
                // …and the coordinate frame, for the same reason and from the
                // same file: a `v<N>.recipe.json` written before v0.30.0 holds
                // its crop and masks in the SENSOR frame of a rotated RAW.
                // Ungated by the generated strip above (a pixel-state card
                // still has geometry) and toasted rather than folded into the
                // status line, so the raster caveat has room.
                let reframe = autoshade::pipeline::migrate_recipe_coord_frame(&src, &mut r);
                self.editor.doc_mut().recipe = r;
                // Read from disk: "Keep skin natural" is decided afresh for
                // this card (`panels/portrait.rs`) — opening never changes a
                // value.
                self.editor.decide_keep_skin_for_card(&mut self.workspace);
                self.editor.resync_recipe_display(&mut self.workspace);
                self.editor.session_mut().dirty = true;
                // A snapshot loaded onto the pristine ✨ card is an edit like
                // any other: it lands on a new ✎ card (the immutability rule),
                // and the load announcement below names that card's canvas.
                self.fork_edited_card();
                let loaded = trf(lang, K_ACTIONS_LOADED_VERSION_V_CTRL_Z_RETURNS_TO_BEFORE_THE_LO, &[("n", &n.to_string())]);
                if restamped {
                    // Said out loud, in the same channel the other
                    // calibration correction uses: the snapshot on disk holds
                    // no camera base look, and the canvas now does.
                    self.workspace.toast(
                        ToastKind::Success,
                        tr(
                            lang,
                            K_ACTIONS_THIS_SNAPSHOT_WAS_TAKEN_ON_A_GENERATED_VARIANT_W,
                        )
                        .to_string(),
                    );
                }
                match reframe {
                    Ok(Some(c)) => self.workspace.toast(ToastKind::Success, coord_migration_sentence(lang, c)),
                    // Said as an error: the snapshot loaded, but its geometry
                    // is off the picture until the raster can be re-written.
                    Err(e) => self.workspace.toast(
                        ToastKind::Error,
                        coord_migration_failure_sentence(lang, &e.to_string()),
                    ),
                    Ok(None) => {}
                }
                *self.workspace.status_mut() = if relook {
                    // The GUI's OWN sentence, localized: the engine note is
                    // English prose meant for the CLI/HTTP surfaces, and
                    // embedding it verbatim put raw English into a localized
                    // status line.
                    format!(
                        "{loaded} — {}",
                        tr(
                            lang,
                            K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                        )
                    )
                } else {
                    loaded
                };
            }
            Err(e) => {
                let t = trf(
                    lang,
                    K_ACTIONS_LOAD_V_FAILED,
                    &[("n", &n.to_string()), ("err", &e.to_string())],
                );
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
            }
        }
    }

    /// Delete one version snapshot. The develop store is locked NO-WAIT: a
    /// foreground 🗑 click must fail into the error arm when another process
    /// holds the develop, not hang.
    pub(crate) fn delete_version_clicked(&mut self, lang: Lang, src: PathBuf, n: u32) {
        match autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || autoshade::store::delete_version(&src, n),
        ) {
            Ok(()) => {
                self.refresh_versions();
                self.workspace.set_status(trf(lang, K_ACTIONS_VERSION_V_DELETED, &[("n", &n.to_string())]));
            }
            Err(e) => {
                self.workspace.persist_postponed(self.lang, &e, "Delete v{n} failed: {err}", &[("n", &n.to_string())]);
            }
        }
    }
}

/// Recipe snapshot path for version `n` — `v<n>.recipe.json` in the photo's
/// central develop dir (gap batch G, ≈ Lightroom virtual copies: cheap
/// parametric versions, never touching the library or the working
/// `recipe.json`).
pub(crate) fn version_path(src: &std::path::Path, n: u32) -> PathBuf {
    autoshade::store::version_target(src, n)
}

/// Does version metadata `m` name the given variant as its source
/// (R24-2)? The ID is authoritative when both sides have one — a card
/// renamed or re-kinded since the snapshot still matches. KIND is the
/// fallback for a record written before ids existed on this photo (or
/// for a card whose own id was never recorded). A record with neither —
/// or no record at all — is UNATTRIBUTED and matches nothing: the filter
/// is "only this variant", and quietly including everything unknown
/// would make it a checkbox that does not filter.
pub(crate) fn version_is_from(
    m: Option<&autoshade::store::VersionMetaEntry>,
    active_id: &str,
    active_kind: Option<VariantKind>,
) -> bool {
    crate::logic::version::is_from(m, active_id, active_kind.map(|k| k.store_str()))
}
