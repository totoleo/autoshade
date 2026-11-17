//! The transient command context: the state one verb may touch.
//!
//! `AutoShadeApp` is the composition root. It owns the two state aggregates
//! ([`Editor`], [`WorkspaceView`]), the worker channel ([`Services`]) and the
//! persisted AI preferences; it also draws the frame. A verb — open a photo,
//! start a look, land a worker's answer — needs those pieces *together*, but
//! it does not own them.
//!
//! This projection is what a verb runs on: the exact five things it may read or
//! write, borrowed for the length of one call. It is the behaviour-side twin of
//! [`PanelView`], which does the same for a panel draw. A verb cannot reach the
//! frame, another window or the eframe hooks; the root cannot grow a method per
//! verb. Field names match the root's on purpose: the verbs moved here verbatim.
//!
//! Test seams ride along under `cfg(test)` because a few spawners have a test
//! stand-in ([`TestSeams`]) the window tests install through the root.

use crate::*;

pub(crate) struct Commands<'a> {
    pub(crate) editor: &'a mut Editor,
    pub(crate) workspace: &'a mut WorkspaceView,
    pub(crate) services: &'a Services,
    pub(crate) aip: &'a mut AiPrefs,
    pub(crate) lang: Lang,
    #[cfg(test)]
    pub(crate) seams: &'a mut TestSeams,
}

impl Commands<'_> {
    /// Run one panel-only draw helper through the projection a panel is drawn
    /// with, then execute the intents it reported. The bridge exists while the
    /// large panels still draw through the command context; a panel that owns
    /// its own [`PanelView`] calls the helper directly.
    pub(crate) fn with_panel<R>(&mut self, ctx: &egui::Context, f: impl FnOnce(&mut PanelView<'_>) -> R) -> R {
        let mut view = PanelView {
            editor: &mut *self.editor,
            workspace: &mut *self.workspace,
            lang: self.lang,
            aip: &mut *self.aip,
            gallery: Vec::new(),
            settings: Vec::new(),
            lens: Vec::new(),
            develop: Vec::new(),
            variants: Vec::new(),
            #[cfg(test)]
            seams: &mut *self.seams,
        };
        let out = f(&mut view);
        let develop = std::mem::take(&mut view.develop);
        let variants = std::mem::take(&mut view.variants);
        drop(view);
        for action in develop {
            self.apply_develop(action);
        }
        for action in variants {
            self.dispatch_variant_action(action, ctx);
        }
        out
    }
}
