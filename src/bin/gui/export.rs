//! Export & save: XMP, rendered files, batch, paste projection.

use super::*;

/// The batch renderer's develop resolution — the OPEN path's precedence
/// (`persist::read_saved_develop_locked`), mirrored onto a
/// [`autoshade::store::DevelopSnapshot`] so the two surfaces answer alike
/// (L13#1: the batch read recipe.json only — an LR-only or XMP-only photo
/// exported neutral, and an older recipe out-ranked a newer Lightroom
/// edit). Order: Lightroom's own sidecar when newest → recipe.json (a
/// NEUTRAL recipe falls through, the open rule) → the store's XMP
/// projection. XMP-restored recipes get the same fresh-calibration stamp
/// the open path applies (`persist::stamp_calibration` shape: knots when
/// non-empty, lens, as-shot pinned only if None). The RAW's embedded packet
/// answers LAST, exactly as on open (`store::embedded_packet_for_restore`
/// carries the clear gate; the snapshot read it under the develop lock).
/// `Ok(None)` = no saved develop (the caller's neutral + base-look fallback).
pub(crate) fn resolve_snapshot_develop(
    p: &std::path::Path,
    snap: &autoshade::store::DevelopSnapshot,
    warns: &mut Vec<String>,
) -> anyhow::Result<Option<(EditRecipe, &'static str)>> {
    // `warns` (L16-2): the disclosures the OPEN path surfaces for the same
    // develop — silent-neutral XMP values, out-of-range clamp drops — used
    // to vanish entirely on this twin: a batch export silently applied a
    // degraded develop the interactive open would have warned about. The
    // caller prefixes each entry with the photo and ships them on the batch
    // outcome. (Full unification with persist::read_saved_develop_locked
    // stays registered on the structure track — the WARNING parity is what
    // this closes.)
    if let Some((text, kind)) = snap.lr_xmp.as_ref()
        && let Some(hit) = xmp_arm(p, text, kind, warns)
    {
        return Ok(Some(hit));
    }
    if let Some((text, from)) = &snap.recipe {
        let mut r = serde_json::from_str::<EditRecipe>(text)?;
        if r.is_noop() {
            // A neutral recipe.json restores NOTHING — on the open path and
            // here alike (2026-09-13): the projection beside it is derived
            // from it and can only disagree by being stale, and the snapshot
            // never carries the embedded packet past a store file. The photo
            // renders the fresh baseline (not the neutral recipe's stale
            // calibration).
            return Ok(None);
        }
        // The one restore path that never went through clamp: a stored
        // recipe with extreme-but-finite geometry rendered NaN weights into
        // a published export. render_to_file now clamps too — this keeps
        // the batch recipe equal to what OPENING the photo would show.
        clamp_disclosed(&mut r, Default::default(), warns);
        // Rasters re-anchor to whichever dir the recipe was read from
        // (central store first, else a legacy ./out sidecar).
        if let Some(base) = from.parent() {
            autoshade::store::resolve_mask_paths(&mut r, base);
        }
        // The batch twin of the open path's coordinate-frame migration: this
        // is the only place a batch export reads a recipe FILE, and without it
        // a portrait RAW exported with its crop and masks on the wrong axis
        // while the interactive open showed them correctly — the exact
        // cross-surface divergence `warns` exists to close.
        match autoshade::pipeline::migrate_recipe_coord_frame(p, &mut r) {
            Ok(Some(c)) => warns.push(autoshade::pipeline::coord_migration_note(c)),
            Err(e) => warns.push(autoshade::pipeline::coord_migration_failure_note(&e.to_string())),
            Ok(None) => {}
        }
        return Ok(Some((r, "recipe.json")));
    }
    if let Some(err) = &snap.recipe_err {
        // An EXISTING save that cannot be read is not absence — exporting
        // neutral over it would silently shed the user's edits.
        anyhow::bail!("{err}");
    }
    if let Some((t, k)) = snap.store_xmp.as_ref()
        && let Some(hit) = xmp_arm(p, t, k, warns)
    {
        return Ok(Some(hit));
    }
    if let Some(t) = snap.packet_xmp.as_ref()
        && let Some(hit) = xmp_arm(p, t, "XMP (embedded in the RAW)", warns)
    {
        return Ok(Some(hit));
    }
    Ok(None)
}

/// One XMP source arm of [`resolve_snapshot_develop`]: import, disclose what
/// the import could not read (the open path's A6 rule), stamp the fresh
/// calibration. `None` = a no-op import, fall through.
fn xmp_arm(
    p: &std::path::Path,
    text: &str,
    kind: &'static str,
    warns: &mut Vec<String>,
) -> Option<(EditRecipe, &'static str)> {
    // Clamped door (R28 2b): what the recipe's SIZE caps cut on the way in is
    // loss `import_losses` below cannot see — it reads the document, this
    // reads the recipe the document produced.
    let diag = autoshade::diag::photo(p);
    let (mut r, imported_clamp) = autoshade::xmp::xmp_to_recipe_clamped_with_diag(text, &diag);
    // Collected for EVERY consulted file — a no-op import included (the
    // persist.rs rule, Codex 32-#1 + review R12-11): a sidecar whose ONLY
    // edit is corrupt restores nothing, and the next save overwrites it in
    // silence unless the corruption is named here.
    let bad = autoshade::xmp::unparsable_crs_numbers(text);
    if !bad.is_empty() {
        warns.push(format!(
            "{} numeric XMP setting(s) unreadable ({}) — restored as neutral",
            bad.len(),
            bad.join(", ")
        ));
    }
    // …and the open path's second disclosure too (review R12-11): what the
    // sidecar's mask corrections cost on the way in.
    //
    // R25 P1 turned this from a COUNT into the reader's own named verdicts.
    // "N unsupported Lightroom correction(s) not applied" was, on any real
    // Lightroom file, a count of the user's ENTIRE mask catalog — refused
    // over a rotation angle and a default blend mode — with nothing to say
    // which of the six import gates had fired. The masks import now, and the
    // line names whatever did not come with them. English, like every other
    // entry in this batch channel; the window's own localized twin is
    // `util::xmp_import_line`.
    if let Some(line) =
        autoshade::xmp::describe_import_losses(
            r.masks.len(),
            &autoshade::xmp::import_losses_for_photo(text, p),
        )
    {
        warns.push(line);
    }
    // R24-5 M0: and the GLOBAL half of that same disclosure, which this path
    // never had — the sidecar's own crs: properties the engine does not model.
    // Preserved by the merge, invisible in the render, and until now unsaid.
    let carried = autoshade::xmp::unmodelled_global_crs(text);
    if !carried.is_empty() {
        let shown = carried.len().min(4);
        let more = carried.len() - shown;
        warns.push(format!(
            "{} Lightroom global setting(s) kept but not rendered ({}{})",
            carried.len(),
            carried[..shown].join(", "),
            if more > 0 { format!(", +{more} more") } else { String::new() }
        ));
    }
    if r.is_noop() {
        return None;
    }
    clamp_disclosed(&mut r, imported_clamp, warns);
    // The import's START (v1.17.0): its own camera profile when it names one,
    // else the photo's fresh start — the open's rule (`stamp_calibration`).
    autoshade::pipeline::stamp_import_start(p, &mut r, &autoshade::diag::photo(p));
    // The SIDECAR-aware form (R29 Batch-3): this is the one stamp site that
    // holds the document and the photograph at the same moment, so it is the
    // one that can tell "no warp because Lightroom drew no correction" from
    // "no warp because nobody could solve one". The other stamp sites reach
    // `fresh_lens_profile` with no document in hand and correctly say the
    // second thing.
    r.lens_profile = autoshade::pipeline::fresh_lens_profile_for_sidecar(p, Some(text));
    if r.as_shot_k.is_none() {
        let (ask, ast) = autoshade::pipeline::fresh_as_shot_wb(p);
        r.as_shot_k = ask;
        r.as_shot_tint = ast;
    }
    Some((r, kind))
}

/// Clamp with the open path's disclosure — a batch drop was silent (L16-2).
///
/// What clamping this recipe cost, said once. `already` folds in a summary the
/// caller has ALREADY paid — the XMP reader clamps on its way out
/// (`xmp::xmp_to_recipe_clamped`), so `r.clamp()` here sees an already-cut
/// recipe and reports nothing about it (R28 2b, adjudication F5). A caller
/// with no earlier clamp passes `Default::default()`; absorbing cannot double
/// count, since the second clamp is idempotent over the first.
fn clamp_disclosed(
    r: &mut EditRecipe,
    already: autoshade::recipe::ClampSummary,
    warns: &mut Vec<String>,
) {
    let mut dropped = already;
    dropped.absorb(r.clamp());
    if !dropped.is_empty() {
        warns.push(format!("out-of-range values discarded ({})", dropped.describe()));
    }
}

/// The destination DIRECTORY a [`ExportDest`] setting resolves to — the one
/// authority the single export, the batch render, the toolbar summary and the
/// Export section's echo all read (R22-7).
///
/// PROBES no filesystem, deliberately — no `is_dir()`, no `exists()`, no
/// `canonicalize`. (Not "touches nothing": the `OutFolder` arm reads the
/// delivery root, and resolving THAT reads the settings file once per process
/// on a cold memo — `config::delivery_root`, which owns the memo and its
/// invalidation.) The toolbar hover resolves this on every frame, and a
/// per-frame `is_dir()` is the very thing `pixels_on_disk`
/// exists to avoid for the ● indicator. It also keeps display and behaviour
/// identical: a remembered folder that has since been deleted is shown, and
/// then RE-CREATED by the render (`start_render_to` create_dir_all's its
/// parent, exactly as it does for a missing `./out`) — an export to a named
/// folder should not silently become a dialog because the folder was tidied
/// away. A genuinely unreachable path (an unplugged drive) fails the render
/// with the real reason, which beats a prompt that explains nothing. The
/// `is_dir()` probe survives only where it is really required: rfd's
/// `set_directory`, which needs an existing folder to open at.
///
/// `None` = the setting has to ask. Two states answer that: `Ask` always, and
/// `LastUsed` before anything has been exported — asking once there seeds the
/// memory, after which every export lands silently in that folder. Falling
/// back to `./out` instead would make "last used folder" mean "./out forever"
/// for anyone who picked it before their first export.
pub(crate) fn export_dest_dir(dest: ExportDest, last: Option<&std::path::Path>) -> Option<PathBuf> {
    match dest {
        // The DELIVERY ROOT (M8), not a literal `./out`: this destination and
        // the CLI / web / batch surfaces name one place, and Settings can move
        // it. Unset, it resolves to the same `./out` this arm always returned.
        ExportDest::OutFolder => Some(autoshade::config::delivery_root()),
        ExportDest::LastUsed => last.map(|d| d.to_path_buf()),
        ExportDest::Ask => None,
    }
}

/// What a paste CARRIES, decided in one place: the clipboard recipe shaped for
/// the photo it came FROM and for any OTHER photo.
///
/// Pulled out of `start_paste` (R25 P8) because this is a data-safety rule and
/// `start_paste` cannot be exercised without spawning the write worker — the
/// two strips below were live for releases with nothing asserting either.
pub(crate) struct PastePayload {
    /// For the clipboard's OWN photo: everything it copied. Pasting back onto
    /// itself used to silently destroy valid AI selections, so this arm keeps
    /// the per-photo state that the foreign arm has to drop.
    pub own: EditRecipe,
    /// For any OTHER photo: the user's EDIT, and nothing that belongs to the
    /// source document.
    pub foreign: EditRecipe,
    /// Masks in the payload whose PIXELS belong to the source photo, for the
    /// caller's toast (they are dropped from `foreign`, and a silent drop is
    /// what the toast exists to prevent).
    pub unpastable_masks: usize,
    /// Whether a colour field was dropped from `foreign`, for the same toast
    /// and the same reason.
    pub colour_field: bool,
    /// Fill layers dropped from `foreign` (2026-10-01): each is pixels of the
    /// source frame, painted where the source's content was.
    pub layers: usize,
    /// Whether a lens blur was dropped from `foreign` (2026-10-13): its depth
    /// map measures the source photo's scene.
    pub lens_blur: bool,
}

/// The two payloads for `src`. `paste_geometry` off strips crop + straighten
/// from BOTH arms — composition rarely transfers between frames, and that
/// includes back onto the source.
pub(crate) fn paste_payload(src: EditRecipe, paste_geometry: bool) -> PastePayload {
    let mut own = src;
    if !paste_geometry {
        own.crop = None;
        own.straighten_deg = 0.0;
    }
    // The foreign arm's rule lives in `recipe_library::portable` — the library
    // saves entries through the same function, so a paste and a library entry
    // carry exactly what the other would (the bitmap masks and the reverse-fit
    // zones whose rasters belong to one photo, the pass-through map, the
    // per-photo colour field, the source frame's fill layers, and the lens
    // blur whose depth map measures the source scene).
    let (foreign, losses) = autoshade::recipe_library::portable(&own, paste_geometry);
    PastePayload {
        own,
        foreign,
        unpastable_masks: losses.masks,
        colour_field: losses.colour_field,
        layers: losses.layers,
        lens_blur: losses.lens_blur,
    }
}

impl Commands<'_> {






    /// Render the full-resolution develop to `out` on a worker thread (16-bit
    /// TIFF, or 8-bit JPEG when the path ends in .jpg). Renders the ACTIVE
    /// variant's pixel source (a Generated variant → its full-res reimagine PNG,
    /// developed by the recipe), so what exports matches what's on screen.
    ///
    /// Returns whether a render actually STARTED. The ask-dialog caller
    /// remembers the chosen folder only then (R22-7): the read-only-library
    /// guard below can refuse a target, and `ExportDest::LastUsed` would
    /// otherwise memorise a folder every future export is refused for.
    pub(crate) fn start_render_to(&mut self, out: PathBuf) -> bool {
        // A ◈ card's master IS the denoised negative: the export-time denoise
        // would run SCUNet over it a second time, so it sits out and the
        // status says so (`export_denoise_applies`, 2026-09-15).
        let denoise = self.editor.export_denoise_applies(&self.workspace);
        // The Export fold's OWN dial (the Detail fold's 「AI Denoise now」 has
        // its own), captured at the click like every other export input.
        let denoise_strength = self.editor.selected_denoise_strength(&self.workspace, true);
        let export = self.workspace.export_opts();
        self.start_render_with(out, export, denoise.then_some(denoise_strength), self.workspace.exp().save_denoise, OutName::Guarded)
    }

    /// [`start_render_to`](Self::start_render_to) with the delivery options
    /// given rather than read off the Export section — Normal's Save
    /// group (`panels/normal.rs`) delivers a jpg at its own quality step.
    /// `denoise` is the AI denoise strength when it runs; `denoise_asked`
    /// says the switch was on even though it sits out (a ◈ card).
    /// `name` says what a file already at `out` means ([`OutName`]): the
    /// guard's refusal to replace an earlier photo stands for the
    /// workspace; a name Normal made is numbered instead (the first free
    /// `-2`, `-3`, … beside it, `pipeline::claim_numbered`); a file the user
    /// picked in the save window is replaced. Every other refusal of the
    /// guard stands for all three.
    pub(crate) fn start_render_with(
        &mut self,
        out: PathBuf,
        export: autoshade::render::ExportOpts,
        denoise: Option<f32>,
        denoise_asked: bool,
        name: OutName,
    ) -> bool {
        let Some(path) = self.editor.active_source_path() else { return false };
        if self.workspace.busy() {
            return false;
        }
        let lang = self.lang;
        // The same guard every CLI export runs: it refuses a destination that
        // would REPLACE an existing photo (a new name anywhere is writable,
        // including beside the source). Guard BOTH anchors: `path` is the
        // render source, which for a Generated variant is the ./out PNG —
        // guarding only that would leave the ORIGINAL photo unprotected
        // exactly when a generated variant is active.
        let mut anchors = vec![path.clone()];
        if let Some(orig) = self.editor.photo().src_path.clone()
            && orig != path
        {
            anchors.push(orig);
        }
        for a in &anchors {
            if let Err(e) = autoshade::pipeline::guard_readonly(&out, a) {
                // A numbered save does not replace the file the guard
                // protects (it takes a fresh name below); a chosen one is
                // the user's confirmed replace.
                if name != OutName::Guarded && e.is::<autoshade::pipeline::OverwriteRefused>() {
                    continue;
                }
                let t = e.to_string();
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
                return false;
            }
        }
        // Claimed on this thread, before the status names it: the line and
        // the landing name the file actually written.
        let numbered = name == OutName::Numbered;
        let out = if numbered {
            match autoshade::pipeline::claim_numbered(&out) {
                Ok(p) => p,
                Err(e) => {
                    let t = format!("{e:#}");
                    self.workspace.set_status(t.clone());
                    self.workspace.toast(ToastKind::Error, t);
                    return false;
                }
            }
        } else {
            out
        };
        self.workspace.set_busy(true);
        // ABSOLUTE, both here and at the landing (workers.rs): `./out` is
        // relative to the directory the app was launched from, so a bare
        // "out/DSC0001.developed.tif" named a place the user had no way to
        // locate from inside the window. `absolute` is lexical — it needs no
        // existing file (this one is about to be created) and, unlike
        // `canonicalize`, it does not hand back a `\\?\` verbatim prefix.
        let shown = abs_display(&out);
        *self.workspace.status_mut() = if denoise.is_some() {
            trf(
                lang,
                K_EXPORT_RENDERING_AI_DENOISE_GPU_SIDECAR_CAN_TAKE_MINUTE,
                &[("path", &shown)],
            )
        } else if denoise_asked {
            trf(
                lang,
                K_EXPORT_RENDERING_FULL_RESOLUTION_THIS_CARD_IS_ALREADY_D,
                &[("path", &shown)],
            )
        } else {
            trf(lang, K_EXPORT_RENDERING_FULL_RESOLUTION, &[("path", &shown)])
        };
        let recipe = self.editor.doc().recipe.clone();
        let src_photo = self.editor.photo().src_path.clone();
        let ai_masks = self.ai_mask_runner();
        self.services.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                let claim = numbered.then(|| out.clone());
                let res = (|| {
                    if let Some(p) = out.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    // Every deliverable repairs (the batch worker's rule):
                    // the canvas normally arrives repaired at open, but when
                    // that repair was an INABILITY (a then-locked file) the
                    // canvas holds the washed curve — and this export shipped
                    // it while a batch export of the SAME canvas repaired it.
                    // Anchored on the PHOTO, like every sibling site: `path`
                    // is the RENDER SOURCE, which for an in-place heal/clone
                    // is the baked master .png — never a RAW — so anchoring
                    // there left the repair permanently dead for exactly the
                    // canvases whose curve still renders (a generated canvas
                    // is stripped and no-ops either way). Off the UI thread,
                    // memo-bounded.
                    let mut recipe = recipe;
                    let relook = src_photo
                        .as_deref()
                        .and_then(|p| {
                            autoshade::pipeline::repair_pre_era_base_curve(p, &mut recipe)
                        })
                        .is_some();
                    // A Lightroom AI mask still without an alpha, or a face
                    // intent without a raster, is drawn here, on the export's
                    // own copy (the render skips it otherwise): keyed on the
                    // PHOTO, on the pixels this render uses.
                    let mut warn = None;
                    if let Some(p) = src_photo.as_deref()
                        && crate::persist::unresolved_develop_masks(&recipe) > 0
                    {
                        let res = ai_masks(p, &path, &mut recipe);
                        if let Some(line) = res.ai.describe() {
                            eprintln!("ai mask : {line}");
                        }
                        if let Some(line) = res.faces.describe() {
                            eprintln!("face mask: {line}");
                        }
                        warn = res.warning();
                    }
                    // SCUNet AI denoise (python sidecar) runs before the develop when on.
                    let opts = denoise.map(|strength| {
                        autoshade::denoise::DenoiseOpts::from_config(
                            &autoshade::config::Config::load(),
                            None,
                            strength,
                        )
                    });
                    autoshade::render::render_to_file(&path, &recipe, &out, opts.as_ref(), Some(&export), autoshade::diag::stderr())?;
                    // FACTS (L12#4): the landing renders the relook note in
                    // the landing-time language.
                    Ok::<ExportOutcome, anyhow::Error>(ExportOutcome::Single {
                        out,
                        relooked: relook,
                        warn,
                    })
                })();
                // A failed numbered save leaves no empty claim behind.
                if res.is_err()
                    && let Some(c) = &claim
                {
                    release_empty_claim(c);
                }
                Msg::Exported(res)
            },
            |e| Msg::Exported(Err(e)),
        );
        true
    }


    /// Batch-render every Ctrl+click-selected photo through its own saved
    /// recipe.json (central store, else legacy ./out; neutral develop when
    /// none exists) with the current export options — "export selected".
    /// Sequential on one worker: each full-res develop is already multi-second
    /// and memory-heavy (61 MP frames), so parallelism would thrash, not speed
    /// up. AI denoise is deliberately excluded (minutes per photo via the GPU
    /// sidecar — run it per-photo from the export panel instead).
    pub(crate) fn start_batch_render(&mut self) {
        if self.workspace.busy() {
            return;
        }
        let targets: Vec<PathBuf> = {
            let mut idx: Vec<usize> = self.workspace.gal().multi_sel.iter().copied().collect();
            idx.sort_unstable(); // report in gallery order, not hash order
            idx.into_iter().filter_map(|i| self.workspace.gal().gallery.get(i).cloned()).collect()
        };
        if targets.is_empty() {
            return;
        }
        let ext = self.workspace.exp().format.ext().to_string();
        let export = self.workspace.export_opts();
        let Some(dest) = self.workspace.export_dest_dir().or_else(|| self.workspace.pick_batch_folder()) else { return };
        self.start_batch_render_for(targets, ext, export, dest, false);
    }


    /// [`start_batch_render`](Self::start_batch_render) over `targets` into
    /// `dest`, in the container `ext` with `export`'s options — Normal's
    /// 「apply this look to every photo in the folder」
    /// (`panels/normal.rs`) renders every photo of the folder as a jpg without
    /// touching the Export section, into the folder it resolved before its
    /// paste. `numbered` is [`start_render_with`](Self::start_render_with)'s:
    /// a photo whose jpg is already in `dest` gets a `-2`, `-3`, … copy, and
    /// each such name is disclosed in `renamed`.
    pub(crate) fn start_batch_render_for(
        &mut self,
        targets: Vec<PathBuf>,
        ext: String,
        export: autoshade::render::ExportOpts,
        dest: PathBuf,
        numbered: bool,
    ) {
        if self.workspace.busy() || targets.is_empty() {
            return;
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = trf(
            lang,
            K_EXPORT_BATCH_RENDERING_PHOTOS,
            &[("n", &targets.len().to_string()), ("path", &abs_display(&dest))],
        );
        // The root travels to the landing as a FACT (L12#4) alongside the
        // BatchNames that consumes it.
        let dest_fact = dest.clone();
        self.workspace.exp_mut().batch_progress = Some((0, targets.len())); // the top-bar progress bar
        // Interim BatchProgress ticks flow through this extra clone; the
        // TERMINAL Msg::Exported is owned by spawn_worker (panic-safe).
        let tx = self.services.tx.clone();
        // In-memory state outranks disk WHOLESALE — recipes AND pixel
        // identities, for every photo that has any: the nav stash holds work
        // navigated away from, and the open photo's live canvas outranks even
        // its own stash entry. Without this, batching a photo you edited (or
        // are looking at) renders the stale (or absent) recipe.json / stale
        // saved pixels, visibly diverging from the screen.
        // path → (recipe, Some((master, generated)) | None = source pixels).
        // The bool is the `pixels.json` flag — `VariantKind::on_ai_pixels`,
        // the axis the flag records (a ✨ card and its ✎ cards sit on the
        // same raster).
        type BatchOverride = (EditRecipe, Option<(PathBuf, bool)>);
        let ai_masks = self.ai_mask_runner();
        let mut overrides: std::collections::HashMap<PathBuf, BatchOverride> = self.workspace.open().nav_stash
            .iter()
            .map(|(p, st)| {
                (
                    p.clone(),
                    (
                        st.recipe.clone(),
                        st.origin.clone().map(|o| (o, st.kind.on_ai_pixels())),
                    ),
                )
            })
            .collect();
        if let Some(p) = self.editor.photo().src_path.clone() {
            let pix = self.editor.active_variant()
                .and_then(|v| v.origin.clone())
                .map(|o| (o, self.editor.active_on_ai_pixels()));
            overrides.insert(p, (self.editor.doc().recipe.clone(), pix));
        }
        self.services.spawn_worker(
            move || {
                // ONE permit across the whole batch: the loop is sequential, so
                // its peak is one photo's (budget.rs).
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(None));
                // A plain block, not an IIFE: since the summary became typed
                // facts (L12#4) nothing early-returns at this level — the
                // per-photo closure below owns the `?` scope.
                let res = {
                    let total = targets.len();
                    let (mut okn, mut errs) = (0usize, Vec::<String>::new());
                    // A selection can hold two same-stem photos (different
                    // folders). Batch-scope claims keep their deliverables
                    // apart; the summary disloses which photo took which
                    // name. Single exports stay stem-keyed (re-export
                    // replaces in place) — the dedup is batch-level by
                    // decision.
                    let mut names = autoshade::pipeline::BatchNames::rooted(dest);
                    // Disclosure counter, like `names.renamed`: this worker
                    // was the one reader that repaired with nobody watching.
                    let mut relooked = 0usize;
                    // Per-photo develop disclosures (L16-2) — the open
                    // path's warnings, shipped on the batch outcome.
                    let mut warns: Vec<String> = Vec::new();
                    // The numbered run's fresh names, disclosed with the
                    // same-stem ones.
                    let mut numbered_names: Vec<String> = Vec::new();
                    for p in &targets {
                        let mut photo_warns: Vec<String> = Vec::new();
                        let mut claimed: Option<PathBuf> = None;
                        let one = (|| -> anyhow::Result<()> {
                            // ONE locked snapshot per photo when DISK decides
                            // (L01): recipe text, pixel source and the .bak
                            // recovery that must PRECEDE recipe selection all
                            // come from a single develop-lock acquisition —
                            // four independent unlocked reads let a
                            // mid-compound save interleave (recipe.json is
                            // retired to .bak for its whole staged publish,
                            // so exists() read "no develop" mid-save) and
                            // shipped a neutral render of an edited photo, or
                            // an old recipe over new baked pixels. The render
                            // below runs OUTSIDE the lock, on the snapshot —
                            // the CLI's documented contract. A develop whose
                            // pixels are a baked retouch master renders FROM
                            // that master (the recipe composes on top, the
                            // same InPlace contract the canvas uses); the
                            // override's pixel identity wins over disk — an
                            // unsaved retouch/denoise must export exactly
                            // like its canvas.
                            let (recipe, pix) = if let Some((lr, pix)) = overrides.get(p) {
                                (lr.clone(), pix.clone())
                            } else {
                                let snap = autoshade::store::read_develop_snapshot(p)?;
                                if let Some(why) = &snap.lr_unreadable {
                                    // Unreadable is not absent (L08): stderr
                                    // for the log AND the batch outcome for
                                    // the user (L16-2 — the windowed build
                                    // has no console).
                                    eprintln!(
                                        "⚠ {}: a Lightroom sidecar sits beside this photo but could not be read ({why}) — the stored develop decides",
                                        autoshade::pipeline::stem(p)
                                    );
                                    photo_warns
                                        .push(format!("Lightroom sidecar unreadable ({why})"));
                                }
                                if let Some(why) = &snap.packet_unreadable {
                                    // Same rule for the packet INSIDE the RAW.
                                    eprintln!(
                                        "⚠ {}: this RAW carries an embedded XMP develop that could not be read ({why}) — it is NOT reflected",
                                        autoshade::pipeline::stem(p)
                                    );
                                    photo_warns.push(format!("embedded XMP unreadable ({why})"));
                                }
                                let recipe = match resolve_snapshot_develop(p, &snap, &mut photo_warns)? {
                                    Some((r, _kind)) => r,
                                    // No saved develop → export what the canvas
                                    // WOULD show: neutral sliders on the
                                    // photo's fresh start (v1.17.0: Lightroom's
                                    // canvas, or the camera-matched base look
                                    // where it cannot render — one extra
                                    // develop per photo then; without it the
                                    // batch export of an unedited RAW comes out
                                    // on the dark base while its open canvas
                                    // shows the bright one).
                                    None => {
                                        let mut fresh = EditRecipe {
                                            lens_profile: autoshade::pipeline::fresh_lens_profile(p),
                                            ..Default::default()
                                        };
                                        autoshade::pipeline::fresh_canvas_start(
                                            p,
                                            &autoshade::diag::photo(p),
                                        )
                                        .stamp(&mut fresh);
                                        fresh
                                    }
                                };
                                // A recorded-but-unhonourable master:
                                // exporting would silently drop the retouch —
                                // fail THIS photo with the cause instead (the
                                // summary lists it). Checked BEFORE the name
                                // claim now, so a refused photo no longer
                                // consumes a same-stem "(2)" slot.
                                if snap.pixel_source.is_none() && snap.pixel_recorded {
                                    anyhow::bail!(
                                        "the saved retouch master could not be loaded — the export would silently drop the retouch (open the photo for the cause, then re-save or clear it)"
                                    );
                                }
                                (recipe, snap.pixel_source)
                            };
                            let named = names.claim(p, "developed", &ext);
                            // The read-only-library gate the single export has
                            // always run, now needed HERE too (R22-7): the
                            // batch root became user-choosable, so a
                            // destination inside the photo library must be
                            // refused instead of being safe by construction
                            // (the old hardcoded ./out). Anchored on the PHOTO
                            // — that is the library folder being protected; a
                            // baked master's own home is ./out, never a
                            // library.
                            // A numbered run steps past an earlier jpg of
                            // the same name instead of refusing it, and
                            // claims its own (`start_render_with`'s rule).
                            let out = match autoshade::pipeline::guard_readonly(&named, p) {
                                Err(e) if !(numbered && e.is::<autoshade::pipeline::OverwriteRefused>()) => return Err(e),
                                _ if numbered => {
                                    let out = autoshade::pipeline::claim_numbered(&named)?;
                                    if out != named {
                                        numbered_names.push(format!("{} ← {}", out.display(), p.display()));
                                    }
                                    claimed = Some(out.clone());
                                    out
                                }
                                _ => named,
                            };
                            autoshade::pipeline::ensure_parent(&out)?;
                            let mut recipe = recipe;
                            if pix.as_ref().is_some_and(|(_, generated)| *generated) {
                                // A GENERATED master's look (camera curve AND
                                // lens corrections) already lives in its
                                // pixels — the stamped disk recipe keeps them
                                // for the RAW's own sake, but rendering the
                                // master through them would cook both twice
                                // (the same strip rule the canvas applies).
                                recipe.strip_baked_start();
                                // The anchor follows the strip rule: baked
                                // pixels carry their WB (relative model).
                                recipe.as_shot_k = None;
                                recipe.as_shot_tint = None;
                            }
                            // Repair AFTER the strip, BEFORE the render — the
                            // load_version ordering, for the same reason: a
                            // generated master's curve is deleted either way,
                            // so repairing first paid a decode for an estimate
                            // nothing used, and the summary then claimed a
                            // correction that never reached a pixel. Counted
                            // only when the export LANDS, so a failure summary
                            // cannot claim corrections it did not ship.
                            // (render_to_file does not go through
                            // store::render_source_checked — batch 60 put the
                            // repair there and this path never saw it.)
                            let repaired = autoshade::pipeline::repair_pre_era_base_curve(
                                p, &mut recipe,
                            )
                            .is_some();
                            let src = pix.map(|(m, _)| m).unwrap_or_else(|| p.clone());
                            // Lightroom AI masks without an alpha and face
                            // intents without a raster: drawn here (cached), as
                            // the single export does; what could not be drawn
                            // joins this photo's develop warnings.
                            if crate::persist::unresolved_develop_masks(&recipe) > 0 {
                                let res = ai_masks(p, &src, &mut recipe);
                                let stem = autoshade::pipeline::stem(p);
                                if let Some(line) = res.ai.describe() {
                                    eprintln!("ai mask : {stem}: {line}");
                                }
                                if let Some(line) = res.faces.describe() {
                                    eprintln!("face mask: {stem}: {line}");
                                }
                                photo_warns.extend(res.warning());
                            }
                            autoshade::render::render_to_file(&src, &recipe, &out, None, Some(&export), autoshade::diag::stderr())?;
                            if repaired {
                                relooked += 1;
                            }
                            Ok(())
                        })();
                        match one {
                            Ok(()) => okn += 1,
                            Err(e) => {
                                if let Some(c) = &claimed {
                                    release_empty_claim(c);
                                }
                                // Typed, before the text is all that is left.
                                errs.push(format!("{}: {e}", autoshade::pipeline::stem(p)));
                            }
                        }
                        if !photo_warns.is_empty() {
                            warns.push(format!(
                                "{}: {}",
                                autoshade::pipeline::stem(p),
                                photo_warns.join("; ")
                            ));
                        }
                        let _ = tx.send(Msg::BatchProgress { done: okn + errs.len(), total });
                    }
                    // FACTS (L12#4): the same-stem renames, the relook count
                    // and the per-photo errors all render at the landing,
                    // with the language live THERE — a batch runs minutes
                    // and the language picker stays reachable throughout.
                    let mut renamed = names.renamed;
                    renamed.extend(numbered_names);
                    Ok(ExportOutcome::Batch {
                        ok: okn,
                        errs,
                        renamed,
                        relooked,
                        warns,
                        // Where this run actually delivered — carried, not
                        // re-derived at the landing (the Destination combo
                        // stays reachable while a batch runs).
                        dest: dest_fact,
                    })
                };
                Msg::Exported(res)
            },
            |e| Msg::Exported(Err(e)),
        );
    }

    /// The Export button / Ctrl+Shift+E: deliver to whatever the Destination
    /// setting says. ONE entry point for both arms (R22-7) — the old pair of
    /// toolbar buttons differed only in where the path came from, so "ask"
    /// became a value of the setting instead of a second button.
    pub(crate) fn start_export(&mut self) {
        match self.editor.export_route(&self.workspace, ) {
            ExportRoute::Render(out) => {
                self.start_render_to(out);
            }
            ExportRoute::Ask => self.export_to_chosen_path(),
        }
    }

    /// Ask for a path and render there — the ▾ menu's 「Export to…」 and the
    /// 「Ask every time」 destination share this ONE body. The chosen folder is
    /// remembered (so `ExportDest::LastUsed` has something to point at) only
    /// when the render actually started: a target the read-only-library guard
    /// refuses must not become the remembered destination.
    pub(crate) fn export_to_chosen_path(&mut self) {
        let ext = self.workspace.exp().format.ext();
        let mut dlg =
            rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(self.editor.export_file_name(&self.workspace, ));
        // Reopen where the last export landed (阶段4) — persisted in Prefs, so
        // it survives restarts.
        if let Some(d) = self.workspace.exp().last_dir.clone().filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        let Some(p) = dlg.save_file() else { return };
        // The format dropdown owns the container — the typed name only picks
        // the stem (normalize_export_target).
        let p = normalize_export_target(p, ext);
        let dir = p.parent().map(|d| d.to_path_buf());
        if self.start_render_to(p) {
            self.workspace.exp_mut().last_dir = dir;
        }
    }



    /// Save this photo's develop to the central store: recipe.json for every
    /// source type + the Lightroom / Camera-Raw XMP projection for RAW.
    /// An XMP reproduces a look via develop PARAMETERS; a Generated variant's
    /// look lives in its pixels, not the recipe, so there's nothing faithful to
    /// write — steer the user to 反推 (which produces a Fitted variant whose XMP
    /// IS the look). Always keyed to the original RAW `src_path`.
    pub(crate) fn save_xmp(&mut self) {
        // Flush every typed-but-uncommitted name FIRST (U10): a save entry
        // point (Ctrl+S, the Save develop button) must write the names the
        // user sees in the boxes, not the pre-focus snapshots — and the
        // strip record this save publishes carries the card names, so the
        // card box has to land before the record is built.
        self.commit_pending_names();
        // The immutability rule's boundary discipline: an edit the frame
        // hook has not moved yet forks NOW, so the strip this save publishes
        // names the ✎ card as its holder and the ✨ card as pristine. (Until
        // 2026-09-13 Ctrl+S REFUSED a generated card outright, and the only
        // way its develop reached disk was the quit-time Save-all — with the
        // stale projection that refusal left standing.)
        self.fork_edited_card();
        let lang = self.lang;
        let Some(path) = self.editor.photo().src_path.clone() else { return };
        // Reset-then-save means "clear my edits": writing a neutral pair would
        // only pin a misleading ● badge with no in-app way to remove it —
        // delete the working sidecars instead (version snapshots are kept).
        // BOTH homes are cleared: the central store AND any legacy ./out file
        // a pre-store build left behind — a surviving legacy sidecar would
        // resurrect the "cleared" edits through the read fallbacks.
        // A baked pixel retouch IS an edit even under a neutral recipe — a
        // heal on an untouched photo must take the SAVE path (recipe.json +
        // pixels.json), not report "nothing to save" while deleting the very
        // linkage the retouch needs to survive a reopen.
        // A NON-TRIVIAL STRIP is an edit too: clear_develop removes
        // variants.json with everything else, so a neutral canvas sitting in
        // a multi-variant strip must take the save path (noop recipe + strip
        // record) — "clear my edits" must not silently destroy the OTHER
        // cards.
        if self.editor.doc().recipe.is_noop()
            && self.editor.active_variant().is_none_or(|v| v.origin.is_none())
            && self.editor.current_strip_record().is_none()
        {
            // NoWait wrapper: clear_develop locks internally, but with Wait —
            // on this UI thread a develop held by another process must fail
            // into the error arm below, not freeze the window.
            match autoshade::store::with_develop_lock(
                &path,
                autoshade::store::DevelopLockMode::NoWait,
                || autoshade::store::clear_develop(&path),
            ) {
                Ok(o) => {
                    self.workspace.gal_mut().edited_badge.clear();
                    // The CANVAS is the baseline, not default(): Reset leaves
                    // the stamped lens profile (and knots) on the canvas, and
                    // a default baseline compared dirty on lens_profile
                    // forever — a permanent ● with quit prompts (R4-8).
                    self.editor.session_mut().hist.saved_recipe = self.editor.doc().recipe.clone();
                    self.editor.session_mut().hist.pixels_on_disk = None;
                    self.editor.session_mut().hist.saved_strip = None;
                    self.workspace.open_mut().nav_stash.remove(&path);
                    self.editor.forget_open_base(&mut self.workspace);
                    match o.marker_warning {
                        // NEVER a plain success: the store copies are gone, but
                        // without the marker a projection copied beside the RAW
                        // out-ranks this clear and restores the edits on reopen.
                        Some(w) => {
                            let t = trf(
                                lang,
                                K_EXPORT_SAVED_EDITS_CLEARED_BUT_THE_CLEAR_COULD_NOT_BE_M,
                                &[("err", &w)],
                            );
                            self.workspace.set_status(t.clone());
                            self.workspace.toast(ToastKind::Error, t);
                        }
                        None => {
                            *self.workspace.status_mut() = if o.removed {
                                tr(lang, K_EXPORT_NEUTRAL_RECIPE_SAVED_EDITS_CLEARED_SAVED_FILES_R)
                                    .into()
                            } else {
                                tr(lang, K_EXPORT_NEUTRAL_RECIPE_NOTHING_TO_SAVE).into()
                            };
                        }
                    }
                }
                Err(e) => {
                    self.workspace.persist_postponed(self.lang, &e, "could not clear the saved edits: {err}", &[]);
                }
            }
            return;
        }
        // recipe.json is the source of truth for EVERY source type (the badge,
        // batch render and reopening all read it; the XMP alone is lossy — no
        // bitmap masks / exact recolour gains). The XMP is the RAW-only Lightroom
        // projection on top. The recipe write ALONE decides the saved state:
        // once it lands, reopening restores it regardless of the XMP — so the
        // ● baseline must follow it even when the XMP half fails.
        let raw = autoshade::decode::is_raw(&path);
        // The WRITER's rule (api_xmp, produce_recipe, match, Save-all): a
        // washed pre-era curve must not be re-persisted verbatim. The canvas
        // normally arrives repaired at open, but an open-time INABILITY (a
        // then-locked file) leaves it washed — and Ctrl+S then froze the
        // defect on disk while a single export from the SAME canvas repaired
        // and disclosed. The canvas itself heals here: what is written is
        // what is shown (a canvas on AI pixels carries no curve by invariant,
        // and the repair declines an empty one).
        // ...and the HISTORY follows it — the Arc-repoint rule (see the
        // preview-resolution repoint below in this file): calibration is not
        // an edit, so every step holding the exact pre-repair pair is
        // re-stamped in place. Without this, commit_if_settled pushed the
        // washed step onto the undo stack and ONE Ctrl+Z after Ctrl+S handed
        // the washed curve back to the canvas invisibly (dirty_vs neutralises
        // both fields, so no ● would have said so) — while disk and every
        // deliverable stayed repaired.
        let before = (self.editor.doc().recipe.version, self.editor.doc().recipe.base_curve.clone());
        let relooked =
            autoshade::pipeline::repair_pre_era_base_curve(&path, &mut self.editor.doc_mut().recipe).is_some();
        if relooked {
            let (after_ver, after_curve) =
                (self.editor.doc().recipe.version, self.editor.doc().recipe.base_curve.clone());
            let restamp = |r: &mut EditRecipe| {
                if r.version == before.0 && r.base_curve == before.1 {
                    r.version = after_ver;
                    r.base_curve = after_curve.clone();
                }
            };
            restamp(&mut self.editor.session_mut().hist.committed.recipe);
            self.editor.session_mut().hist.undo_stack.iter_mut().for_each(|s| restamp(&mut s.recipe));
            self.editor.session_mut().hist.redo_stack.iter_mut().for_each(|s| restamp(&mut s.recipe));
            // The strip entry that IS the canvas follows it, and the preview
            // re-develops under the healed curve.
            self.editor.sync_active_recipe();
            // ...and the SIBLINGS follow too (the stash-retry rule): the
            // estimate is already paid, so healing the rest of the strip is
            // a memo hit apiece — a save must not leave the photo split
            // into a bright canvas over washed sibling RECIPES. A healed
            // sibling also drops its CARD: the thumb is a stale render of
            // the washed recipe, and keeping it showed a stops-dark
            // thumbnail beside a bright canvas until the next rebuild.
            let active = self.editor.doc().active;
            for (i, v) in self.editor.doc_mut().variants.iter_mut().enumerate() {
                if i != active
                    && v.kind.is_source_based()
                    && autoshade::pipeline::repair_pre_era_base_curve(&path, &mut v.recipe)
                        .is_some()
                {
                    v.thumb = None;
                }
            }
            self.editor.session_mut().dirty = true;
        }
        // ONE lock across the whole compound save: recipe.json, the
        // pixels.json link, the strip record and the XMP projection publish
        // as a unit, so another process cannot interleave between the halves.
        // NoWait — a busy develop postpones the save, ● stays lit, the canvas
        // loses nothing, and Ctrl+S retries.
        let locked_save = autoshade::store::with_develop_lock(
            &path,
            autoshade::store::DevelopLockMode::NoWait,
            || -> std::io::Result<()> {
        if self.workspace.open().open_unresolved {
            // The baseline came from an open that could not READ the saved
            // develop — snapshot whatever is on disk before overwriting it
            // (v<N> copy). Refusing on a failed snapshot beats destroying a
            // save nobody ever looked at.
            if let Err(e) = autoshade::store::backup_saved_develop(&path, Some(&self.editor.doc().recipe)) {
                return Err(std::io::Error::other(format!(
                    "the unread develop could not be backed up ({e}) — nothing was overwritten"
                )));
            }
        }
        // ONE single-generation commit (L03): recipe.json, the pixels.json
        // link and the strip record stage together and publish past ONE
        // durable marker — no kill point between them can leave a new recipe
        // over an old (or half-cleared) master link, or a new pair under a
        // stale strip. All-or-nothing replaces the old per-member degrade
        // notes: the save either lands whole (the notes below describe
        // SEMANTICS — what a reopen restores — not write outcomes) or fails
        // whole, with every unsaved protection still armed.
        let origin = self.editor.active_variant().and_then(|v| v.origin.clone());
        let strip_rec = self.editor.current_strip_record();
        let generated = self.editor.active_on_ai_pixels();
        // A canvas on AI pixels is the STRIPPED form — no calibration, the
        // raster carries it. The DISK form keeps the RAW's (the Analyze
        // saver's and Save-all's rule, from the same one-snapshot source):
        // a master that later fails to decode falls back to a corrected
        // develop on the negative, not a dark one. Memo-cheap after the open.
        let disk = if generated {
            // Every calibration half, the start (v1.17.0) included, through
            // the one stamper.
            let mut d = self.editor.doc().recipe.clone();
            autoshade::pipeline::stamp_fit_calibration(&mut d, autoshade::pipeline::photo_calibration(&path));
            d
        } else {
            self.editor.doc().recipe.clone()
        };
        // The Lightroom projection is a MEMBER of this commit (2026-09-13):
        // it lands or clears with the recipe it projects, never after it.
        // Built here so its merge base (the sidecar beside the RAW) is read
        // under the same lock; a projection this build cannot produce
        // degrades to Keep plus the failure toast below — the recipe write
        // alone still decides the saved state.
        let (xmp_member, xmp_outcome) = match autoshade::pipeline::xmp_projection_member(
            &path,
            &disk,
            !generated,
            autoshade::diag::stderr(),
        ) {
            Ok((m, note, losses)) => (m, Ok((note, losses))),
            Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
        };
        let committed: anyhow::Result<()> = (|| {
            let recipe_bytes = autoshade::pipeline::recipe_store_bytes(&path, &disk, autoshade::diag::stderr())?;
            let pixels = match &origin {
                // An in-place heal/clone/fill bakes pixels into the variant's
                // origin raster; parametric recipe/XMP cannot carry them, so
                // the store records the master's path and reopening restores
                // the retouched canvas.
                Some(o) => autoshade::store::CommitMember::Write(
                    autoshade::store::pixel_source_record_bytes(&path, o, generated)?,
                ),
                // A parametric-only save CLEARS any stale record — a
                // surviving one would resurrect an obsolete retouched canvas
                // on the next open.
                None => autoshade::store::CommitMember::Clear,
            };
            // The strip half through the ONE owner (R24-4
            // `store::variants_member`): Ctrl+S is the writer that OWNS the
            // whole strip, so it hands the live record over verbatim — and
            // a trivial strip still clears the record, exactly as before.
            let variants = autoshade::store::variants_member(
                &path,
                autoshade::store::ActiveWrite::Strip(strip_rec.as_ref()),
            )?;
            autoshade::store::commit_develop(
                &path,
                autoshade::store::DevelopCommit {
                    recipe: Some(recipe_bytes),
                    pixels,
                    variants,
                    xmp: xmp_member,
                },
            )?;
            Ok(())
        })();
        match committed {
            Ok(()) => {
                self.workspace.open_mut().open_unresolved = false;
                let rp = autoshade::store::recipe_target(&path);
                let pixel_note: Option<String> = origin.is_some().then(|| {
                    tr(
                        lang,
                        K_EXPORT_RETOUCHED_PIXELS_MASTER_LINKED_REOPENING_RESTORE,
                    )
                    .to_string()
                });
                // The strip mirror advances WITH the commit (all-or-nothing:
                // a failed commit leaves it untouched and the
                // background-variant unsaved protection armed).
                self.editor.session_mut().hist.saved_strip = strip_rec;
                self.workspace.gal_mut().edited_badge.clear(); // the open photo just gained its badge
                self.editor.session_mut().hist.saved_recipe = self.editor.doc().recipe.clone();
                self.workspace.open_mut().nav_stash.remove(&path);
                self.editor.session_mut().hist.pixels_on_disk = origin;
                let mut s = if generated {
                    // No sidecar reproduces AI pixels: the projection member
                    // CLEARED, and the line says so instead of "XMP + recipe".
                    trf(
                        lang,
                        K_EXPORT_RECIPE_SAVED_NO_LIGHTROOM_XMP_THIS_CARD_S_LOOK_S,
                        &[("path", &rp.display().to_string())],
                    )
                } else if raw {
                    match xmp_outcome {
                        Ok((merge_note, losses)) => {
                            let p = autoshade::pipeline::xmp_target(&path);
                            // A sidecar we could not MERGE was regenerated, and
                            // that drops the user's Lightroom-only properties.
                            // Saying only "saved" is how that loss stayed
                            // invisible until they reopened the catalog.
                            if let Some(m) = &merge_note {
                                self.workspace.toast(ToastKind::Error, m.clone());
                            }
                            // M6a: the projection's own lossy edges — assembled
                            // from what the WRITER just skipped/degraded, so the
                            // counts describe this very file. Empty ⇒ silent.
                            // R24-5 M0 adds the GLOBAL half beside the mask
                            // half: an active control the engine renders and
                            // the sidecar has no property for (the camera base
                            // curve, the lens-profile correction) used to leave
                            // no trace at all, and the difference only showed up
                            // in Lightroom's render.
                            //
                            // WHETHER it interrupts is `xmp_loss_interrupts`'
                            // call (R24 round-end MED-2): a base curve every
                            // RAW open stamps is a real loss the user cannot
                            // act on, and an error toast on every single
                            // Ctrl+S is how the mask half — which they CAN act
                            // on — stopped being read.
                            let globals = autoshade::xmp::global_export_losses(&self.editor.doc().recipe);
                            let interrupts = xmp_loss_interrupts(&losses, &globals);
                            let mask_note = xmp_loss_line(lang, &losses, &globals);
                            if let Some(m) = &mask_note
                                && interrupts
                            {
                                self.workspace.toast(ToastKind::Error, m.clone());
                            }
                            let base = trf(
                                lang,
                                K_EXPORT_XMP_RECIPE_SAVED,
                                &[("path", &p.display().to_string())],
                            );
                            let mut s = match merge_note {
                                Some(m) => format!("{base} — ⚠ {m}"),
                                None => base,
                            };
                            if let Some(m) = mask_note {
                                // The quiet channel is the one the pixel-link
                                // and re-look notes already use on this very
                                // line: a middle dot, no ⚠.
                                s.push_str(&if interrupts {
                                    format!(" — ⚠ {m}")
                                } else {
                                    format!(" · {m}")
                                });
                            }
                            // R25 B4: the OPPOSITE corner, on the same line.
                            // The sidecar just written is complete — it is the
                            // canvas that is missing something, so this is
                            // never a loss, never a toast, never a ⚠. Said
                            // here because this is the moment the file goes to
                            // Lightroom, which is when the difference between
                            // the two renders starts to matter.
                            if let Some(m) = render_gap_line(
                                lang,
                                &autoshade::xmp::global_render_gaps(&self.editor.doc().recipe),
                            ) {
                                s.push_str(&format!(" · {m}"));
                            }
                            s
                        }
                        Err(e) => {
                            let t = trf(
                                lang,
                                K_EXPORT_RECIPE_SAVED_BUT_THE_LIGHTROOM_XMP_FAILED,
                                &[("err", &e.to_string())],
                            );
                            self.workspace.toast(ToastKind::Error, t.clone());
                            t
                        }
                    }
                } else {
                    trf(
                        lang,
                        K_EXPORT_RECIPE_SAVED_XMP_APPLIES_TO_RAW_ONLY,
                        &[("path", &rp.display().to_string())],
                    )
                };
                if let Some(n) = pixel_note {
                    s.push_str(&n);
                }
                if relooked {
                    s.push_str(&format!(
                        " · {}",
                        tr(
                            lang,
                            K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                        )
                    ));
                }
                self.editor.forget_open_base(&mut self.workspace);
                self.workspace.set_status(s);
            }
            Err(e) => {
                let mut t = trf(lang, K_ACTIONS_SAVE_FAILED, &[("err", &e.to_string())]);
                if relooked {
                    // The disclosure travels with the MUTATION, not the
                    // success: the canvas healed above even though the write
                    // failed, and a silently brighter photo under an
                    // unrelated error reads as a glitch.
                    t.push_str(&format!(
                        " · {}",
                        tr(
                            lang,
                            K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                        )
                    ));
                }
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t); // a failed save must be seen
            }
        }
                Ok(())
            },
        );
        if let Err(e) = locked_save {
            self.workspace.persist_postponed(self.lang, 
                &e,
                "save postponed: this photo is being changed by another AutoShade process ({err}); your canvas remains unsaved — retry",
                &[],
            );
        }
    }

    /// Paste the copied recipe onto every Ctrl+click-selected photo on a worker
    /// thread — Lightroom's "sync settings", without rendering anything: a
    /// recipe.json per photo (central store) plus an XMP sidecar for RAWs.
    /// Geometry (crop/straighten) is stripped unless `paste_geometry` is on,
    /// because composition rarely transfers between frames. Library files are
    /// never touched (write_recipe / write_xmp land in the store, never beside
    /// the photo).
    pub(crate) fn start_paste(&mut self) {
        let Some(src) = self.workspace.genai().copied.clone() else { return };
        if self.workspace.busy() {
            return;
        }
        let targets: Vec<PathBuf> = {
            let mut idx: Vec<usize> = self.workspace.gal().multi_sel.iter().copied().collect();
            idx.sort_unstable(); // report in gallery order, not hash order
            idx.into_iter().filter_map(|i| self.workspace.gal().gallery.get(i).cloned()).collect()
        };
        if targets.is_empty() {
            return;
        }
        let copied_from = self.workspace.genai().copied_from.clone();
        self.start_paste_to(src, copied_from, targets, self.workspace.genai().paste_geometry);
    }

    /// [`start_paste`](Self::start_paste) with its inputs given: the recipe
    /// `src` copied from `copied_from`, onto `targets`, with or without the
    /// crop. Normal's 「apply this look to every photo in the folder」
    /// (`panels/normal.rs`) pastes the open photo's look onto the whole folder
    /// this way — the clipboard and the selection are left as they are.
    pub(crate) fn start_paste_to(
        &mut self,
        src: EditRecipe,
        copied_from: Option<PathBuf>,
        targets: Vec<PathBuf>,
        geometry: bool,
    ) {
        if self.workspace.busy() || targets.is_empty() {
            return;
        }
        let PastePayload {
            own: recipe_full,
            foreign: recipe,
            unpastable_masks: n_raster,
            colour_field: dropped_field,
            layers: n_layers,
            lens_blur: dropped_blur,
        } = paste_payload(src, geometry);
        let has_foreign_target = targets.iter().any(|t| Some(t) != copied_from.as_ref());
        // "Keep skin natural" (Portraits) holds each target's write where it
        // is written, by that target's own switch (`skin_held_for`).
        let skin_held: std::collections::HashSet<PathBuf> =
            targets.iter().filter(|t| self.editor.skin_held_for(&self.workspace, t)).cloned().collect();
        if n_raster > 0 && has_foreign_target {
            self.workspace.toast(
                ToastKind::Error,
                trf(
                    self.lang,
                    K_EXPORT_RASTER_MASK_S_NOT_PASTED_THEIR_RASTERS_BELONG_TO,
                    &[("n", &n_raster.to_string())],
                ),
            );
        }
        if dropped_field && has_foreign_target {
            self.workspace.toast(
                ToastKind::Error,
                tr(
                    self.lang,
                    K_EXPORT_THE_COLOUR_FIELD_WAS_NOT_PASTED_ITS_CELLS_ARE_ME,
                ),
            );
        }
        if dropped_blur && has_foreign_target {
            self.workspace.toast(
                ToastKind::Error,
                tr(self.lang, K_EXPORT_THE_LENS_BLUR_WAS_NOT_PASTED_ITS_DEPTH_MAP_WAS_M),
            );
        }
        if n_layers > 0 && has_foreign_target {
            self.workspace.toast(
                ToastKind::Error,
                trf(
                    self.lang,
                    K_EXPORT_FILL_LAYER_S_NOT_PASTED_THEY_ARE_THE_SOURCE_PHOT,
                    &[("n", &n_layers.to_string())],
                ),
            );
        }
        // If the open photo is one of the targets, take the paste live in the
        // editor too (undo-able through the usual committed-snapshot step).
        // Remember the pair so Msg::Pasted can advance the ● baseline: the
        // worker writes this exact recipe to the open photo's store, so
        // leaving `saved_recipe` behind kept a false "● unsaved" lit for
        // edits that were already on disk.
        self.workspace.open_mut().pasted_open = None;
        if let Some(open) = self.editor.photo().src_path.clone()
            && targets.iter().any(|t| t == &open)
        {
            // Same base_curve rule the worker applies to every target below,
            // mirrored here so `pasted_open` equals the bytes the worker
            // writes and the ● baseline can advance on success.
            let chosen = if Some(&open) == copied_from.as_ref() { &recipe_full } else { &recipe };
            let mut live = paste_recipe_for(&open, chosen);
            if skin_held.contains(&open) {
                autoshade::face::keep_skin_natural_in(&mut live);
            }
            // The DISK form keeps its calibration (the worker writes it
            // below), but an active GENERATED canvas must not render it —
            // its pixels already carry the look (the load_version /
            // open-restore strip rule). The ● baseline (pasted_open) is
            // normalized the same way: canvas coordinates.
            if self.editor.active_on_ai_pixels() {
                live.strip_baked_start();
                // Anchor follows the strip rule (baked WB → relative model).
                live.as_shot_k = None;
                live.as_shot_tint = None;
            }
            self.editor.doc_mut().recipe = live.clone();
            self.editor.session_mut().dirty = true;
            // A paste onto the pristine ✨ card is an edit: it lands on a new
            // ✎ card now (the immutability rule), so the ● baseline below
            // belongs to that card's canvas.
            self.fork_edited_card();
            // Wholesale recipe replacement: disarm index-carrying tools and
            // refresh derived display, like every other whole-swap path.
            self.editor.resync_recipe_display(&mut self.workspace);
            self.workspace.open_mut().pasted_open = Some((open, live));
        }
        let lang = self.lang; // pre-spawn UI statuses only; results land as FACTS (L12#4)
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = trf(
            lang,
            K_EXPORT_PASTING_RECIPE_TO_PHOTOS,
            &[("n", &targets.len().to_string())],
        );
        self.services.spawn_worker(
            move || {
                // A plain block, not an IIFE: the typed outcome (L12#4)
                // removed the early bail — the per-target `step` closure
                // owns the `?` scope.
                let res: anyhow::Result<PasteOutcome> = {
                    let (mut okn, mut xmpn) = (0usize, 0usize);
                    let mut errs: Vec<String> = Vec::new();
                    // XMP-half failures are partial successes (recipe-write-
                    // decides), but their REASON used to reach stderr only —
                    // the status said "n XMP" and left the user to notice the
                    // shortfall by subtraction.
                    let mut xmp_fails: Vec<String> = Vec::new();
                    let mut xmp_notes: Vec<String> = Vec::new();
                    let mut written: Vec<PathBuf> = Vec::new();
                    for path in &targets {
                        let mut step = || -> anyhow::Result<bool> {
                            // Worker thread ⇒ Wait: queue behind whichever
                            // process holds this target's develop, exactly
                            // like the CLI. ONE lock across gate → recipe →
                            // XMP so the pasted pair publishes as a unit.
                            autoshade::store::with_develop_lock(
                                path,
                                autoshade::store::DevelopLockMode::Wait,
                                || {
                            // Per-target base-look resolution (paste_recipe_for):
                            // paste copies the user edit, never the source
                            // photo's camera calibration over a saved one. The
                            // clipboard's own photo keeps its bitmap masks.
                            let chosen =
                                if Some(path) == copied_from.as_ref() { &recipe_full } else { &recipe };
                            let mut r = paste_recipe_for(path, chosen);
                            if skin_held.contains(path) {
                                autoshade::face::keep_skin_natural_in(&mut r);
                            }
                            // The programmatic-overwrite gate, like Analyze /
                            // serve / the CLI: one click overwrites MANY saved
                            // develops the user is not even looking at — a
                            // failed snapshot refuses that target.
                            autoshade::store::backup_saved_develop(path, Some(&r))
                                .map_err(|e| anyhow::anyhow!(
                                    "refusing to overwrite the saved develop: backing it up failed ({e})"
                                ))?;
                            // The projection is a member of the same
                            // generation (2026-09-13). A paste replaces the
                            // develop inside whatever card the target is on:
                            // a persisted GENERATED master says that card
                            // sits on AI pixels, and no sidecar may project
                            // a develop over those — the member clears
                            // instead of lying.
                            let ai_master = autoshade::store::recorded_pixel_source(path)
                                .and_then(|(o, g)| g.then_some(o));
                            let on_source = ai_master.is_none();
                            let (xmp_member, xmp_outcome) =
                                match autoshade::pipeline::xmp_projection_member(
                                    path,
                                    &r,
                                    on_source,
                                    autoshade::diag::stderr(),
                                ) {
                                    Ok((m, note, _)) => (m, Ok(note)),
                                    Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
                                };
                            // ONE single-generation commit, like every other
                            // develop writer (R24-4): the paste used to call
                            // `pipeline::write_recipe` DIRECTLY — the fifth
                            // production write of an active develop, and the
                            // only one with no `variants` member at all, so
                            // an invariant patrolling `commit_develop` call
                            // sites could never see it. `pixels: Keep` is
                            // today's behaviour stated out loud: a paste
                            // copies develop PARAMETERS and never touches the
                            // target's baked master link. The strip member
                            // goes through the shared primitive: over source
                            // pixels as `Unknown` — a paste replaces the
                            // develop inside whatever card the target is on
                            // without learning anything about that card, and
                            // it must not claim the base negative's slot (a
                            // second Original card can never be deleted
                            // again); over a GENERATED master as
                            // `DevelopOnAiPixels` — the record's word follows
                            // the develop (the immutability rule: a pasted
                            // develop is an ✎ card, the ✨ card stays).
                            autoshade::store::commit_develop(
                                path,
                                autoshade::store::DevelopCommit {
                                    recipe: Some(autoshade::pipeline::recipe_store_bytes(
                                        path, &r,
                                        autoshade::diag::stderr(),
                                    )?),
                                    pixels: autoshade::store::CommitMember::Keep,
                                    variants: autoshade::store::variants_member(
                                        path,
                                        match &ai_master {
                                            Some(m) => autoshade::store::ActiveWrite::DevelopOnAiPixels {
                                                recipe: &r,
                                                master: m,
                                            },
                                            None => autoshade::store::ActiveWrite::Unknown,
                                        },
                                    )?,
                                    xmp: xmp_member,
                                },
                            )?;
                            if autoshade::decode::is_raw(path) && on_source {
                                // Recipe-write-decides: an XMP failure after
                                // the recipe landed is a partial success —
                                // reopening restores the paste regardless.
                                // The third member (the M6a mask-loss list) is
                                // deliberately dropped HERE: `xmp_notes` is
                                // rendered under one fixed sentence about
                                // regenerated-not-merged sidecars, so folding
                                // another kind of loss into it would mislabel
                                // both. A foreign target has had its bitmap
                                // masks stripped (and said so) before this
                                // worker started; the remaining per-target
                                // losses reach stderr from write_xmp_doc. A
                                // per-target UI list of them needs its own
                                // PasteOutcome member — R23 work, not a
                                // mislabelled fold into this one.
                                match xmp_outcome {
                                    Ok(None) => {}
                                    // Regenerated-not-merged loses LR-only
                                    // properties — collected for the paste
                                    // summary (stderr already heard it in
                                    // the builder).
                                    Ok(Some(m)) => xmp_notes.push(format!(
                                        "{}: {m}",
                                        autoshade::pipeline::stem(path)
                                    )),
                                    Err(e) => {
                                        eprintln!(
                                            "⚠ {}: recipe pasted, but the Lightroom XMP failed: {e}",
                                            autoshade::pipeline::stem(path)
                                        );
                                        xmp_fails.push(format!(
                                            "{}: {e}",
                                            autoshade::pipeline::stem(path)
                                        ));
                                        return Ok(false);
                                    }
                                }
                                return Ok(true);
                            }
                            Ok(false)
                                },
                            )
                        };
                        match step() {
                            Ok(wrote_xmp) => {
                                okn += 1;
                                xmpn += usize::from(wrote_xmp);
                                written.push(path.clone());
                            }
                            Err(e) => errs.push(format!("{}: {e}", autoshade::pipeline::stem(path))),
                        }
                    }
                    // FACTS (L12#4): counts and detail lists render at the
                    // landing; a partial failure keeps riding the error
                    // channel there, so it never reads as a clean success.
                    Ok(PasteOutcome { ok: okn, xmp: xmpn, errs, xmp_fails, xmp_notes, written })
                };
                Msg::Pasted(res)
            },
            |e| Msg::Pasted(Err(e)),
        );
    }
}
