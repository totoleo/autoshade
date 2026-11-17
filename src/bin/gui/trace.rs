//! The GUI's runtime trace.
//!
//! The debug build's console shows `diag` lines, but a hang or a runaway
//! re-develop needs a TIMELINE a terminal cannot keep, and a windowed release
//! build has no console at all. **Debug builds write this trace by default**
//! to `<develop store>/logs/gui-<pid>.log`; a release build opts in with
//! `AUTOSHADE_GUI_TRACE=1`. `AUTOSHADE_GUI_TRACE=0` quiets a debug build. The
//! headless test run never writes: the gate below excludes `cfg(test)`.
//!
//! What is logged on purpose: the develop dispatch/land/reject cycle (the
//! canvas' one compute loop), the base re-develop flight, the recipe-swap
//! forks, the skin clamp, the coverage-overlay rebuilds and a frame summary
//! every 600 frames. Enough to tell a legitimately busy canvas from a loop,
//! and which input keeps resetting it. The file stops growing at 64 MiB so a
//! genuine loop cannot fill the disk.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static LOG: OnceLock<Option<Mutex<LogFile>>> = OnceLock::new();
static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const MAX_BYTES: u64 = 64 * 1024 * 1024;

struct LogFile {
    file: std::fs::File,
    written: u64,
}

/// Is this run tracing? `AUTOSHADE_GUI_TRACE` wins when set (any non-empty
/// value except `0`/`false` = on, `0`/`false` = off); unset, a debug build
/// traces and a release build does not.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| match std::env::var_os("AUTOSHADE_GUI_TRACE") {
        Some(v) => !v.is_empty() && v != "0" && v != "false",
        None => cfg!(debug_assertions) && !cfg!(test),
    })
}

fn log_file() -> Option<&'static Mutex<LogFile>> {
    LOG.get_or_init(|| {
        let dir = autoshade::store::store_root().join("logs");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("gui-{}.log", std::process::id()));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some(Mutex::new(LogFile { file, written: 0 }))
    })
    .as_ref()
}

/// One line in the trace. Callers usually guard with [`on`] so the arguments
/// are not formatted when tracing is off.
pub(crate) fn event(tag: &str, detail: impl std::fmt::Display) {
    if !on() {
        return;
    }
    let Some(log) = log_file() else { return };
    let start = START.get_or_init(Instant::now);
    if let Ok(mut log) = log.lock() {
        if log.written >= MAX_BYTES {
            return;
        }
        let line = format!("{:>10.3}s [{tag}] {detail}\n", start.elapsed().as_secs_f64());
        if log.file.write_all(line.as_bytes()).is_ok() {
            log.written += line.len() as u64;
            let _ = log.file.flush();
        }
    }
}

/// One frame summary every 600 frames (~10 s at 60 fps): enough to tell a
/// busy canvas from an idle one without flooding the log.
pub(crate) fn frame(detail: impl std::fmt::Display) {
    if !on() {
        return;
    }
    let n = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if n % 600 == 0 {
        event("frame", format_args!("#{n} {detail}"));
    }
}
