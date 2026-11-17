//! Normal's business: the Save group's batch hand-off and landing words, and
//! the noise switch's card dance (the ◈ card, its look history, the undo
//! baseline).
//!
//! The panel draws and emits verbs; this app layer decides.

use crate::panels::normal::DenoisePair;
use crate::*;

/// A folder by its own name (its full path is one hover away).
pub(crate) fn folder_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(|| abs_display(p), |n| n.to_string_lossy().into_owned())
}

impl Commands<'_> {

    /// The paste half of 「apply to every photo」 landed: render them all into
    /// `dest`, or leave the paste's own error standing.
    pub(crate) fn after_apply_all_paste(&mut self, lang: Lang, dest: PathBuf, pasted: bool) {
        if !pasted {
            return;
        }
        // The paste's own sentence is the workspace's; the render's status
        // replaces it below.
        let old = self.workspace.status().clone();
        self.workspace.chrome_mut().toasts.retain(|t| t.text != old);
        let targets = self.workspace.gal().gallery.clone();
        let export = self.workspace.normal_export_opts();
        self.start_batch_render_for(targets, "jpg".into(), export, dest, true);
        if self.workspace.busy() {
            self.workspace.normal_mut().delivering = true;
            self.workspace.set_status(tr(lang, K_ACTIONS_SAVING_EVERY_PHOTO_IN_THE_FOLDER).into());
        }
    }




    /// Put card `idx` under the canvas with the look on screen: the switch
    /// changes the grain, never the look the sliders made — nor the look's
    /// undo history.
    fn show_card_keeping_look(&mut self, idx: usize) {
        let look = self.editor.doc().recipe.clone();
        let history = self.take_look_history();
        let ctx = self.services.egui_ctx.clone();
        self.switch_variant(idx, &ctx);
        if self.editor.doc().active == idx {
            self.editor.doc_mut().recipe = look;
            self.editor.session_mut().dirty = true;
            self.carry_look_history(history);
        }
    }

    /// The look's undo history, before the noise switch changes the card
    /// under it (a card change resets the history — `load_active`). The
    /// look on screen is committed first, so it is the head.
    pub(crate) fn take_look_history(&mut self) -> (Vec<UndoStep>, Vec<UndoStep>) {
        self.editor.commit_now(&self.workspace);
        (self.editor.session().hist.undo_stack.clone(), self.editor.session().hist.redo_stack.clone())
    }

    /// …and back, on the card now on screen: every step keeps its look and
    /// takes this card's pixels (the switch changes the grain, never the
    /// look), and the look on screen is the head.
    pub(crate) fn carry_look_history(&mut self, (undo, redo): (Vec<UndoStep>, Vec<UndoStep>)) {
        let here = self.editor.current_step(&self.workspace);
        let onto = |s: UndoStep| UndoStep { recipe: s.recipe, base: here.base.clone(), origin: here.origin.clone(), keep_skin: s.keep_skin };
        self.editor.session_mut().hist.undo_stack = undo.into_iter().map(onto).collect();
        self.editor.session_mut().hist.redo_stack = redo.into_iter().map(onto).collect();
        self.editor.session_mut().hist.committed = here;
    }

    /// The noise switch. On: the Retouch panel's own AI denoise
    /// (`start_ai_denoise` — the same worker, model and strength; its ◈ card
    /// lands on the canvas), or — when this card was denoised already — that
    /// ◈ card again, with no second run. Off: back to the card it was made
    /// from (the first card that is not a ◈ card when the switch did not
    /// make it).
    pub(crate) fn set_normal_denoise(&mut self, on: bool) {
        let Some(photo) = self.editor.photo().src_path.clone() else { return };
        if self.workspace.busy() || on == self.editor.normal_denoise_on(&self.workspace) {
            return;
        }
        let Some(active) = self.editor.active_variant().map(|v| v.id.clone()) else { return };
        let pair = self.workspace.normal().denoise_pair.clone().filter(|d| d.photo == photo);
        let lang = self.lang;
        if on {
            let made = pair.filter(|d| d.plain == active).and_then(|d| d.denoised).and_then(|id| self.editor.card_index(&id));
            if let Some(idx) = made {
                self.show_card_keeping_look(idx);
                self.workspace.set_status(tr(lang, K_ACTIONS_AI_DENOISE_ON).into());
                return;
            }
            self.workspace.normal_mut().denoise_pair = Some(DenoisePair { photo, plain: active, denoised: None });
            self.start_ai_denoise();
            if self.workspace.busy() {
                self.workspace.set_status(tr(lang, K_ACTIONS_REMOVING_THE_GRAIN_THIS_TAKES_A_MOMENT).into());
            } else {
                // Refused before it ran: its own line stands.
                self.workspace.normal_mut().denoise_pair = None;
            }
        } else {
            let back = pair
                .filter(|d| d.denoised.as_deref() == Some(active.as_str()))
                .and_then(|d| self.editor.card_index(&d.plain))
                .or_else(|| self.editor.doc().variants.iter().position(|v| v.kind != VariantKind::Denoised));
            let Some(idx) = back else { return };
            // Remembered, so turning it on again shows this ◈ card.
            self.workspace.normal_mut().denoise_pair = Some(DenoisePair { photo, plain: self.editor.doc().variants[idx].id.clone(), denoised: Some(active) });
            self.show_card_keeping_look(idx);
            self.workspace.set_status(tr(lang, K_ACTIONS_AI_DENOISE_OFF).into());
        }
    }

    /// Before a retouch lands (`Msg::Retouched`): the look's undo history,
    /// when the landing may be the noise switch's own ◈ card.
    pub(crate) fn normal_denoise_history(&mut self) -> Option<(Vec<UndoStep>, Vec<UndoStep>)> {
        self.workspace.normal()
            .denoise_pair
            .as_ref()
            .is_some_and(|d| d.denoised.is_none() && self.editor.photo().src_path.as_ref() == Some(&d.photo))
            .then(|| self.take_look_history())
    }

    /// After a retouch landed (`Msg::Retouched`): when it was the noise
    /// switch's AI denoise, remember its ◈ card, carry the look's undo
    /// history onto it, and say so plainly.
    pub(crate) fn after_normal_denoise(&mut self, lang: Lang, history: Option<(Vec<UndoStep>, Vec<UndoStep>)>) {
        let Some(d) = self.workspace.normal_mut().denoise_pair.as_mut() else { return };
        if d.denoised.is_some() || self.editor.photo().src_path.as_ref() != Some(&d.photo) {
            return;
        }
        let Some(v) = self.editor.doc().variants.get(self.editor.doc().active).filter(|v| v.kind == VariantKind::Denoised && v.id != d.plain) else {
            return;
        };
        d.denoised = Some(v.id.clone());
        if let Some(h) = history {
            self.carry_look_history(h);
        }
        self.workspace.reword_landing(tr(lang, K_ACTIONS_AI_DENOISE_DONE_TURN_THE_SWITCH_OFF_TO_SEE_THE_P).into());
    }
}
