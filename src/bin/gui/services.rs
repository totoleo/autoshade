//! The app's infrastructure services: the worker channel and the egui context a
//! worker wakes. They are NOT photo state — replacing the executor or the event
//! loop should not touch the document, the panels or the history.

use crate::view::*;

use std::sync::mpsc::{Receiver, Sender};

use crate::model::Msg;

/// What the app hands to background work: the completion channel and the
/// event-loop context a completion repaints through (a plain mpsc send does not
/// wake egui).
pub(crate) struct Services {
    pub(crate) rx: Option<Receiver<Msg>>,
    pub(crate) tx: Sender<Msg>,
    pub(crate) view_ctx: Context,
}

impl Default for Services {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { rx: Some(rx), tx, view_ctx: Context::default() }
    }
}

impl Services {
    /// Spawn a worker whose PANIC still delivers a terminal message. Every
    /// worker's last act is sending the `Msg` that clears `busy` (or an
    /// inflight counter); a panic before that send unwinds the thread, the
    /// message never arrives, and the whole app soft-locks — every action
    /// gates on `!busy`, and only killing the process recovers. One decode
    /// panic inside `rawler`/`image` on a malformed file is enough. So: run
    /// the body under `catch_unwind` and synthesize the site's failure `Msg`
    /// from the panic payload. `AssertUnwindSafe` is sound here — all
    /// captured state is moved in and dropped on unwind; the UI only ever
    /// observes the channel message.
    pub(crate) fn spawn_worker(
        &self,
        body: impl FnOnce() -> Msg + Send + 'static,
        on_panic: impl FnOnce(anyhow::Error) -> Msg + Send + 'static,
    ) {
        let tx = self.tx.clone();
        let ctx = self.view_ctx.clone();
        // Starting a worker schedules the next frame itself: the repaint
        // pump's gate (`poll_workers`, at the top of update) has already run
        // by the time a panel can start one in the same frame, so without
        // this the "fetching…"/busy state is not drawn and the 100 ms pump
        // never re-arms until the next input event.
        self.view_ctx.request_repaint();
        std::thread::spawn(move || {
            // This body's panic is CONTAINED by the `catch_unwind` below.
            // Declared, so the panic hook does not announce a crash over an
            // app that is about to report the failure and carry on
            // (`autoshade::panic_guard`).
            let _contained = autoshade::panic_guard::Recoverable::enter();
            let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
                .unwrap_or_else(|p| {
                    let s = p
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    on_panic(anyhow::anyhow!("worker panicked: {s}"))
                });
            let _ = tx.send(msg);
            // An mpsc send does not wake egui: with the pointer held still, a
            // completed result used to sit unread in the channel until the
            // next mouse or key event happened to produce a frame.
            ctx.request_repaint();
        });
    }
}
