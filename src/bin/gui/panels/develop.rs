//! Develop panel: sliders, sections, variant strip.

use crate::*;

// The section ● predicate and the table it reads (R25 P0) — see
// `develop_panel` and `dev_lens`.
use crate::sections::{section_active, SECTIONS};


impl Commands<'_> {









    pub(crate) fn develop_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        let mut changed = false;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, K_DEVELOP_DEVELOP), false, |_| {});
        // OUTSIDE the editable gate below, deliberately (#4 review point): the
        // histogram is a READOUT. Its only interaction is the pair of clipping
        // triangles, whose click reaches `toggle_clipping` — that writes
        // `show_clipping` + the overlay texture and, on a fresh open with no
        // retained frame, `dirty`. None of those is EDIT state: no recipe field
        // is read into a widget or written back, so the L15-2 input-loss class
        // (edit photo A's sliders while B lands) cannot arise here. The worst
        // mid-open outcome is one preview build whose frame `finish_redevelop`
        // then discards by its base+recipe identity check. Greying the tone
        // readout during every open would be the bigger regression.
        self.with_panel(|p| p.histogram_ui(ui));
        ui.add_space(SPACE_SM);

        // Mid-open (decode in flight) these controls would edit the STASHED
        // photo A while B lands and replaces the whole recipe — silent input
        // loss (L15-2). Busy alone must NOT gate here: a 600 s analyze keeps
        // the panel live; only the open transition freezes it.
        // The AI area carries its OWN copy of this gate (panels/ai.rs) — it
        // used to ride inside this closure as `dev_ai`.
        let editable = !self.workspace.open().open_in_flight;
        ui.add_enabled_ui(editable, |ui| {
        // Lightroom-style grouping: a wall of 16 sliders scans terribly; four
        // titled sections (tone open, the rest by activity) scan at a glance.
        // A section whose values are non-neutral shows a ● so a collapsed
        // active adjustment is never invisible. Flags are snapshot up front —
        // Copy bools, so no borrow spans the section closures (E0500).
        //
        // The section predicates are DERIVED from the control registry's families
        // (R25 P0): each was a hand-written field tuple that had to be widened
        // by hand whenever its section gained a control, and R22 #16 had
        // already found four of them one field short. `family_is_active` reads
        // the same table the AI's tool plan is built from, so a control that
        // joins a family joins its dot.
        // The section table (`sections.rs`) owns the display order, the
        // family set each ● reads, and which sections are registered custom
        // widgets. This loop is the only place the panel enumerates them.
        for section in SECTIONS {
            let active = section_active(&self.editor.doc().recipe, section);
            changed |= match section.draw {
                crate::sections::SectionDraw::App(draw) => draw(self, ui, active),
                crate::sections::SectionDraw::Panel(draw) => {
                    self.with_panel(|p| draw(p, ui, active))
                }
            };
        }
        });

        if changed {
            self.editor.doc_mut().recipe.clamp();
            self.editor.session_mut().dirty = true;
        }
    }

    /// The variant strip (版本条): one card per rendition — 原片 / AI 生成 /
    /// 反推 — with a live developed thumbnail. Click a card to switch (lossless;
    /// each variant keeps its own base + recipe), × to drop one. This is the
    /// selector that makes an AI develop a first-class, non-reverting version.
    pub(crate) fn variant_strip(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let accent = self.workspace.ui().theme.colors().accent_text; // Copy — safe in closures
        // ONE deferred action for the whole strip (R24-5): the five separate
        // `Option`s this replaced were dispatched by an `else if` chain whose
        // exclusivity was a property of the chain; a single value makes it a
        // property of the TYPE, and it is the same value the edit-state list
        // hands to the same owner. Last writer wins where the chain had a
        // fixed priority — reachable only if two widgets were clicked in one
        // frame, which one pointer cannot do.
        let mut act: Option<VariantAction> = None;
        let busy = self.workspace.busy(); // Copy — read before the card closures borrow self
        // The card column is centered by an EXPLICIT top pad, not by
        // cross-align: Align::Center positioned the row before its content
        // grew — egui seeds a horizontal row at interact_size.y, centered
        // that 26 px stub in the panel, and the 84 px column then grew
        // downward from it (37 px of air above, 21 px of column OVERFLOW
        // below the panel; R14 user report). The column height is knowable
        // up front — thumb + item gap + label row (egui rows are at least
        // interact_size.y tall) — so the pad is exact and the geometry test
        // asserts the air above equals the air below.
        // Named STRIP_ to stay clear of model.rs's gallery THUMB_H (40.0),
        // which this local used to SHADOW inside variant_strip — one glob
        // import away from a silent geometry swap in either direction.
        const STRIP_THUMB_H: f32 = 52.0;
        let card_h = STRIP_THUMB_H + ui.spacing().item_spacing.y + ui.spacing().interact_size.y;
        #[cfg(test)]
        {
            self.seams.strip_row_rect = Some(ui.max_rect());
        }
        ui.add_space(((ui.available_height() - card_h) / 2.0).max(0.0));
        // LAYOUT INVARIANT (probed, R14+R15): egui places the scroll area
        // centered by its 26 px DECLARED seed against the row's height at
        // placement time, then grows content DOWNWARD — (current−26)/2 of
        // offset. Keeping every child before the scroll area at ZERO height
        // (pure horizontal spacing) pins that offset at exactly 0, which the
        // geometry test asserts. (Every cross-align variant re-broke it,
        // measured 37/29/11.8 px — all equal to (row_height−26)/2.) The
        // title and divider therefore do not participate in layout at all
        // (R16): they are PAINTED at rects computed from the same knowns as
        // the pad — the one way to center them on the card column without
        // re-breaking the row.
        let row_top = ui.cursor().top();
        let strip_left = ui.max_rect().left();
        let title_galley = ui.painter().layout_no_wrap(
            tr(lang, K_DEVELOP_VARIANTS__2).to_owned(),
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().strong_text_color(),
        );
        let title_w = title_galley.size().x;
        let title_pos = egui::pos2(
            strip_left + SPACE_SM,
            row_top + (card_h - title_galley.size().y) / 2.0,
        );
        #[cfg(test)]
        {
            self.seams.strip_title_rect =
                Some(egui::Rect::from_min_size(title_pos, title_galley.size()));
        }
        ui.painter().galley(title_pos, title_galley, ui.visuals().strong_text_color());
        // The divider, at full card height — the stock separator filled the
        // 26 px seed, a stub over the card's top third.
        let sep_x = strip_left + SPACE_SM + title_w + SPACE_SM + 3.0;
        ui.painter().vline(
            sep_x,
            egui::Rangef::new(row_top, row_top + card_h),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        ui.horizontal(|ui| {
            // Horizontal footprint of the PAINTED title + divider: width
            // only, zero height (the invariant above).
            ui.add_space(SPACE_SM + title_w + SPACE_SM + 6.0 + SPACE_SM);
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for i in 0..self.editor.doc().variants.len() {
                        let active = i == self.editor.doc().active;
                        let kind = self.editor.doc().variants[i].kind;
                        ui.vertical(|ui| {
                            // Developed thumbnail (or a placeholder until the
                            // variant has been developed once).
                            let resp = if let Some(t) = &self.editor.doc().variants[i].thumb {
                                let s = t.size_vec2();
                                let h = STRIP_THUMB_H;
                                let w = (s.x / s.y.max(1.0) * h).clamp(30.0, 104.0);
                                let (rect, resp) =
                                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                                let uv = egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                );
                                // Card chrome tokens (L15-1): the glow is
                                // the chrome accent at low alpha and its
                                // radius is DERIVED — the literal
                                // 0xc9a14a/7.0 copy silently detached from
                                // any theme change.
                                const CARD_RADIUS: f32 = 4.0;
                                const GLOW_EXPAND: f32 = 3.0;
                                let ring = self.workspace.ui().theme.colors().accent_fill;
                                if active {
                                    ui.painter().rect_filled(
                                        rect.expand(GLOW_EXPAND),
                                        CARD_RADIUS + GLOW_EXPAND,
                                        egui::Color32::from_rgba_unmultiplied(
                                            ring.r(),
                                            ring.g(),
                                            ring.b(),
                                            46,
                                        ),
                                    );
                                }
                                ui.painter().image(t.id(), rect, uv, egui::Color32::WHITE);
                                if active {
                                    ui.painter().rect_stroke(
                                        rect,
                                        CARD_RADIUS,
                                        egui::Stroke::new(2.0, ring),
                                        egui::StrokeKind::Middle,
                                    );
                                }
                                resp
                            } else {
                                ui.add_sized([64.0, STRIP_THUMB_H], egui::Button::new("…"))
                            };
                            if resp
                                .on_hover_text(tr(
                                    lang,
                                    K_DEVELOP_CLICK_TO_SWITCH_TO_THIS_VARIANT_LOSSLESS_CTRL_S,
                                ))
                                .clicked()
                            {
                                act = Some(VariantAction::Switch(i));
                            }
                            ui.horizontal(|ui| {
                                let label = Text::new(tr(lang, kind.label())).small();
                                let label = ui.label(if active { label.strong().color(accent) } else { label });
                                // The immutability rule, said where the card
                                // is (2026-09-13): a ✨ card never takes an
                                // edit — the first one continues on a new ✎
                                // card — so the label says so before the
                                // user wonders where their slider went.
                                if kind == VariantKind::Generated {
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_A_GENERATED_IMAGE_STAYS_AS_GENERATED_THE_FIRST_E,
                                    ));
                                } else if kind == VariantKind::Stacked {
                                    // The ▦ card (v1.5.0): what it is and what
                                    // a .xmp from it can carry, said where the
                                    // card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_SEVERAL_FRAMES_OF_THIS_SCENE_MERGED_INTO_THEIR_O,
                                    ));
                                } else if kind == VariantKind::Denoised {
                                    // The ◈ card (2026-09-15): what it is and
                                    // what a .xmp from it can carry, said where
                                    // the card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_THE_NEGATIVE_AI_DENOISED_INTO_ITS_OWN_MASTER_DEV,
                                    ));
                                }
                                // The card's NAME (R24-3), which until now
                                // had a persistence path (variants.json,
                                // R24-2) and no producer. The ACTIVE card
                                // edits it in place — the version rows'
                                // discipline, keyed by the card's own id so a
                                // background push or delete cannot land the
                                // text on a different card; a background card
                                // only DISPLAYS the name it has (offering a
                                // 「Name…」 placeholder on every card would
                                // widen the whole strip for an affordance
                                // that acts on one of them).
                                let card_id = self.editor.doc().variants[i].id.clone();
                                let named = self.editor.doc().variants[i].name.clone();
                                let editing = active
                                    && self.workspace.cards().variant_name_buf.as_ref().is_some_and(|(p, id, ..)| {
                                        *id == card_id
                                            && Some(p.as_path()) == self.editor.photo().src_path.as_deref()
                                    });
                                if editing {
                                    let resp = {
                                        let buf =
                                            &mut self.workspace.cards_mut().variant_name_buf.as_mut().expect("checked just above").3;
                                        ui.add(
                                            // Keyed by the CARD's id, like
                                            // the buffer: an index key made
                                            // egui hand the box a new
                                            // identity (and drop the caret)
                                            // the moment an async push
                                            // renumbered the strip.
                                            egui::TextEdit::singleline(buf)
                                                .id(ui.make_persistent_id((
                                                    "variant_name",
                                                    card_id.as_str(),
                                                )))
                                                .desired_width(90.0)
                                                .hint_text(tr(lang, K_DEVELOP_NAME)),
                                        )
                                    };
                                    // The +1 boundary: this box's own
                                    // lost-focus commit (Enter counts as
                                    // losing focus in egui), and the frame it
                                    // appears in has to claim the caret — the
                                    // click that opened it landed on the
                                    // label that used to be here.
                                    #[cfg(test)]
                                    {
                                        self.seams.strip_name_rect = Some(resp.rect);
                                    }
                                    if resp.lost_focus() {
                                        self.commit_variant_name_buf();
                                        self.workspace.cards_mut().variant_name_buf = None;
                                    } else if !resp.has_focus() {
                                        resp.request_focus();
                                    }
                                } else if active {
                                    let shown = match &named {
                                        Some(n) => Text::new(format!("「{n}」")).small(),
                                        None => Text::new(tr(lang, K_DEVELOP_NAME__2).to_string())
                                            .small()
                                            .weak(),
                                    };
                                    let resp = ui
                                        .add(egui::Label::new(shown).sense(egui::Sense::click()))
                                        .on_hover_text(tr(lang, K_DEVELOP_NAME_THIS_VARIANT));
                                    #[cfg(test)]
                                    {
                                        self.seams.strip_name_rect = Some(resp.rect);
                                    }
                                    if resp.clicked() {
                                        act = Some(VariantAction::Rename(card_id.clone()));
                                    }
                                } else if let Some(n) = &named {
                                    ui.label(Text::new(format!("「{n}」")).small().weak());
                                }
                                if let Some(a) = self.with_panel(|p| p.variant_card_buttons(
                                    ui,
                                    i,
                                    active,
                                    busy,
                                    VariantSurface::Strip,
                                )) {
                                    act = Some(a);
                                }
                            });
                            #[cfg(test)]
                            if i == 0 {
                                self.seams.strip_card_rect = Some(ui.min_rect());
                            }
                        });
                        ui.add_space(SPACE_MD);
                    }
                });
            });
        });
        if let Some(a) = act {
            self.dispatch_variant_action(a, ui.ctx());
        }
    }

























    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_versions(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;

        // --- 版本: recipe snapshots ≈ LR virtual copies (gap batch G) --------
        // #14b, group 4 — DELIVERY (versions + export): what leaves the app,
        // as opposed to what is being edited. This boundary gets the same
        // fence + hairline + breather rhythm the other two group fences use
        // (Detail, Local Masks) — the rhythm is completed here, not changed:
        // Versions previously opened with a bare sibling-section fence while
        // being the head of a different KIND of group.
        group_caption(ui, tr(lang, K_DEVELOP_VERSIONS_EXPORT));
        let n_ver = self.workspace.cards().versions.len();
        let n_ver_s = n_ver.to_string();
        // Deferred like the strip's, and dispatched through the same owner
        // after the section's closures release `self` (R24-5).
        let mut act: Option<VariantAction> = None;
        section_fold(ui, "sec_versions", section_title(&trf(lang, K_DEVELOP_VERSIONS, &[("n", &n_ver_s)]), n_ver > 0), false, false, |ui| {
                // Clearing the list on open (see `open_path`) already stops a
                // STALE row from acting on the incoming photo. This is the
                // second half: while a photo is still decoding, "Save as
                // version" would snapshot the OUTGOING photo's canvas — the
                // fresh recipe has not landed yet — into the incoming photo's
                // develop. Nothing here is meaningful until the open settles,
                // so the whole section goes inert rather than each button
                // carrying its own guard.
                ui.add_enabled_ui(!self.workspace.busy(), |ui| {
                // R24-4 (#1, phase 2 — minimal): ONE place that answers
                // "what edit states does this photo have". A photo is one
                // negative + N variant CARDS + one version history, so the
                // cards are listed first and the numbered snapshots under
                // them, in the vocabulary the data model now uses: the
                // taxonomy's binary (parametric vs pixel-state) and the
                // orthogonal "hangs off a baked master" attribute are the
                // two notes a row can carry (R24-1).
                //
                // R24-5: the rows are no longer a read-only view — each one
                // carries the card's own ＋ / ▣ / ✕, drawn by the SAME owner
                // the strip draws them with (`variant_card_buttons`), so the
                // two surfaces cannot come to mean different things. Clicking
                // a row switches to that card, exactly as clicking its
                // thumbnail does. Renaming stays on the strip on purpose: two
                // `TextEdit`s over one buffer fight for the caret every frame
                // (see `VariantSurface`).
                //
                // The buttons take a SECOND line rather than joining the
                // label row. A 320 px panel already holds a kind label, a
                // 「name」 and a vocabulary note; three more widgets on that
                // line is how a side panel starts growing per frame, which is
                // a defect this repo has shipped twice (R19, R22-3) and now
                // has width-stability tests for.
                let n_var = self.editor.doc().variants.len();
                let busy = self.workspace.busy();
                #[cfg(test)]
                {
                    self.seams.edit_list_rows.clear();
                    self.seams.edit_list_actions.clear();
                }
                if n_var > 0 {
                    let accent = self.workspace.ui().theme.colors().accent_text;
                    ui.label(
                        Text::new(trf(
                            lang,
                            K_DEVELOP_VARIANTS,
                            &[("n", &n_var.to_string())],
                        ))
                        .weak()
                        .small(),
                    );
                    let active_i = self.editor.doc().active;
                    for i in 0..n_var {
                        // Read the three facts the row shows BEFORE the layout
                        // closure borrows `self` (Variant is not Clone, and
                        // making it so for a display row would be the wrong
                        // fix — the card's texture handle rides on it).
                        let (kind, name, on_pixels) = {
                            let v = &self.editor.doc().variants[i];
                            (v.kind, v.name.clone(), v.origin.is_some())
                        };
                        #[cfg(test)]
                        self.seams.edit_list_rows.push(format!("variant:{}", kind.store_str()));
                        let hit = ui
                            .horizontal(|ui| {
                                let label = Text::new(tr(lang, kind.label())).small();
                                ui.label(if i == active_i {
                                    label.strong().color(accent)
                                } else {
                                    label
                                });
                                if let Some(n) = &name {
                                    ui.label(Text::new(format!("「{n}」")).small());
                                }
                                if i == active_i {
                                    ui.label(
                                        Text::new(tr(lang, K_DEVELOP_CURRENT))
                                            .weak()
                                            .small(),
                                    );
                                }
                                if kind == VariantKind::Edited {
                                    ui.label(
                                        Text::new(tr(lang, K_DEVELOP_DEVELOP_OVER_AI_PIXELS_NO_XMP))
                                            .weak()
                                            .small(),
                                    );
                                } else if !kind.is_source_based() {
                                    ui.label(
                                        Text::new(tr(lang, K_DEVELOP_PIXEL_STATE_NO_XMP))
                                            .weak()
                                            .small(),
                                    );
                                } else if on_pixels {
                                    ui.label(
                                        Text::new(tr(lang, K_DEVELOP_ON_BAKED_PIXELS))
                                            .weak()
                                            .small(),
                                    );
                                }
                            })
                            .response
                            .interact(egui::Sense::click());
                        // The whole label row is the switch target — the same
                        // gesture the strip's thumbnail offers, on the row that
                        // names the card. A BACKGROUND row only: re-selecting
                        // the active card would run a switch (and its commit)
                        // for no change.
                        if i != active_i
                            && hit
                                .on_hover_text(tr(
                                    lang,
                                    K_DEVELOP_CLICK_TO_SWITCH_TO_THIS_VARIANT_LOSSLESS,
                                ))
                                .clicked()
                        {
                            act = Some(VariantAction::Switch(i));
                        }
                        // The card's own actions, one owner with the strip.
                        // Indented under the label so the two lines read as
                        // one row rather than as two entries.
                        let has_actions =
                            i == active_i || (n_var > 1 && kind != VariantKind::Original);
                        if has_actions {
                            ui.horizontal(|ui| {
                                ui.add_space(SPACE_LG);
                                if let Some(a) = self.with_panel(|p| p.variant_card_buttons(
                                    ui,
                                    i,
                                    i == active_i,
                                    busy,
                                    VariantSurface::List,
                                )) {
                                    act = Some(a);
                                }
                            });
                        }
                    }
                }
                sub_caption(ui, tr(lang, K_DEVELOP_SNAPSHOT_HISTORY));
                if action_row(ui, true, tr(lang, K_DEVELOP_SAVE_AS_VERSION))
                    .on_hover_text(tr(lang, K_DEVELOP_SNAPSHOT_THIS_CARD_S_DEVELOP_ONLY_AS_A_NUMBERED))
                    .clicked()
                {
                    self.save_version();
                }
                // R24-2: a version row is 「v3 「日落偏暖」· 来自 ◭ 反推」 —
                // the number, the user's own name, and which VARIANT the
                // snapshot was taken from. The filter answers "show me only
                // this card's history"; rows it hides are COUNTED below, so
                // a shrinking list is never mistaken for lost versions.
                let active_id = self.editor.active_variant().map(|v| v.id.clone()).unwrap_or_default();
                let active_kind = self.editor.active_variant().map(|v| v.kind);
                if !self.workspace.cards().versions.is_empty() {
                    ui.checkbox(&mut self.workspace.cards_mut().versions_current_only, tr(lang, K_DEVELOP_ONLY_THIS_VARIANT))
                        .on_hover_text(tr(
                            lang,
                            K_DEVELOP_SHOW_ONLY_SNAPSHOTS_TAKEN_FROM_THE_VARIANT_YOU_A,
                        ));
                }
                let mut load: Option<u32> = None;
                let mut delete: Option<u32> = None;
                let mut rename: Option<u32> = None;
                let mut hidden = 0usize;
                // The list is cloned so the rows may borrow `self` mutably
                // (the rename buffer) — it is a handful of u32.
                let versions = self.workspace.cards().versions.clone();
                let photo = self.editor.photo().src_path.clone();
                for n in versions {
                    let meta = self.workspace.cards().version_meta.get(&n).cloned();
                    if self.workspace.cards().versions_current_only
                        && !version_is_from(meta.as_ref(), &active_id, active_kind)
                    {
                        hidden += 1;
                        continue;
                    }
                    #[cfg(test)]
                    self.seams.edit_list_rows.push(format!("version:{n}"));
                    let named = meta.as_ref().and_then(|m| m.name.clone());
                    let editing = self.workspace.cards().version_name_buf
                        .as_ref()
                        .is_some_and(|(p, j, ..)| *j == n && Some(p) == photo.as_ref());
                    ui.horizontal(|ui| {
                        ui.label(format!("v{n}"));
                        let field_id = ui.make_persistent_id(("version_name", n));
                        if editing {
                            let resp = {
                                let buf = &mut self.workspace.cards_mut().version_name_buf
                                    .as_mut()
                                    .expect("checked just above")
                                    .3;
                                ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .id(field_id)
                                        .desired_width(110.0)
                                        .hint_text(tr(lang, K_DEVELOP_NAME)),
                                )
                            };
                            // The +1 boundary: this row's own lost-focus
                            // commit, the twin of the mask panel's (U10).
                            // Enter counts as losing focus in egui. Leaving
                            // the box also leaves EDIT MODE — a row that
                            // stayed a TextEdit forever would keep asking
                            // for focus below and never give it back.
                            if resp.lost_focus() {
                                self.editor.commit_version_name_buf(&mut self.workspace, self.lang);
                                self.workspace.cards_mut().version_name_buf = None;
                            } else if !resp.has_focus() {
                                // The frame the box first appears in: the
                                // click that opened it landed on the LABEL
                                // that used to be here, so the caret has to
                                // be put here explicitly or the user's next
                                // keystroke goes nowhere.
                                resp.request_focus();
                            }
                        } else {
                            let shown = match &named {
                                Some(name) => format!("「{name}」"),
                                None => tr(lang, K_DEVELOP_NAME__2).to_string(),
                            };
                            let text = match &named {
                                Some(_) => Text::new(shown),
                                None => Text::new(shown).weak(),
                            };
                            if ui
                                .add(egui::Label::new(text).sense(egui::Sense::click()))
                                .on_hover_text(tr(lang, K_DEVELOP_NAME_THIS_SNAPSHOT))
                                .clicked()
                            {
                                rename = Some(n);
                            }
                        }
                        // Provenance, only when it was actually recorded —
                        // a version from before this existed says nothing
                        // rather than guessing.
                        if let Some(m) = &meta {
                            let from = m.from_kind.as_deref().and_then(VariantKind::from_store_str);
                            let note = match (from, m.origin.as_deref()) {
                                // `kind` by name, not `k`: the i18n audit's
                                // dynamic-key registry keys on the receiver
                                // spelling (`kind.label()`), which is how a
                                // new VariantKind without a zh pair is caught.
                                (Some(kind), _) => Some(trf(
                                    lang,
                                    K_DEVELOP_FROM,
                                    &[("kind", tr(lang, kind.label()))],
                                )),
                                (None, Some(autoshade::store::VERSION_ORIGIN_AUTO)) => {
                                    Some(tr(lang, K_DEVELOP_AUTO_ARCHIVED).to_string())
                                }
                                _ => None,
                            };
                            if let Some(note) = note {
                                ui.label(Text::new(note).weak().small());
                            }
                        }
                        if action(ui, true, tr(lang, K_DEVELOP_LOAD)).on_hover_text(tr(lang, K_DEVELOP_REPLACE_CURRENT_PARAMETERS_ONE_CTRL_Z_TO_UNDO)).clicked() {
                            load = Some(n);
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(lang, K_DEVELOP_DELETE_THIS_SNAPSHOT_ITS_FROZEN_MASK_RASTERS_GO))
                            .clicked()
                        {
                            delete = Some(n);
                        }
                    });
                }
                if hidden > 0 {
                    ui.label(
                        Text::new(trf(
                            lang,
                            K_DEVELOP_HIDDEN_SAVED_FROM_ANOTHER_VARIANT,
                            &[("n", &hidden.to_string())],
                        ))
                        .weak()
                        .small(),
                    );
                }
                if let Some(n) = rename
                    && let Some(p) = photo.clone()
                {
                    // Switching rows commits the previous one to ITS OWN
                    // (photo, number) first — the mask panel's cross-commit
                    // rule, made structural by keying on the number.
                    self.editor.commit_version_name_buf(&mut self.workspace, self.lang);
                    let cur = self.workspace.cards().version_meta
                        .get(&n)
                        .and_then(|m| m.name.clone())
                        .unwrap_or_default();
                    self.workspace.cards_mut().version_name_buf = Some((p, n, cur.clone(), cur));
                }
                if let Some(n) = load {
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::LoadVersion(n),
                    )]);
                }
                if let Some(n) = delete
                    && let Some(src) = self.editor.photo().src_path.clone()
                {
                    // A rename buffer aimed at the row being deleted dies
                    // WITH it (R24-2): committing it afterwards would write a
                    // name for a number that no longer exists and can never
                    // be re-issued — a record only the next delete would
                    // clean up.
                    if self.workspace.cards().version_name_buf.as_ref().is_some_and(|(p, j, ..)| *j == n && *p == src)
                    {
                        self.workspace.cards_mut().version_name_buf = None;
                    }
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::DeleteVersion(n),
                    )]);
                }
                if n_ver == 0 {
                    explainer(ui, lang, tr(lang, K_DEVELOP_LIKE_LR_VIRTUAL_COPIES_STORE_MULTIPLE_PARAMETER));
                }
                }); // add_enabled_ui: inert while a photo is still opening
            });
        // The edit-state list's card action, performed through the SAME owner
        // the strip uses — after the section's closures have released `self`.
        if let Some(a) = act {
            self.dispatch_variant_action(a, ui.ctx());
        }
        false
    }

}
