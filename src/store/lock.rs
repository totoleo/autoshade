//! The develop and settings locks: the lock mode, the guard, the per-path lock, and the OS lock primitives.

use super::*;

/// How a surface responds when another process is mutating this photo.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevelopLockMode {
    /// CLI, server, and worker threads queue behind the current mutation.
    Wait,
    /// Foreground GUI actions return `WouldBlock` and leave the canvas dirty.
    NoWait,
}

thread_local! {
    /// OS locks are not recursively lockable through independent handles.
    /// Compound surface saves call lower-level store writers (and a locked
    /// settings writer's load can hit the corrupt-file rescue), so
    /// same-thread nesting reuses the outer lock while other threads and
    /// processes still contend through the kernel. Keyed by lock-file path:
    /// develop locks and the settings lock share this set.
    static HELD_FILE_LOCKS: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

struct DevelopLockGuard {
    path: PathBuf,
    file: File,
}

impl Drop for DevelopLockGuard {
    fn drop(&mut self) {
        os_develop_lock::unlock(&self.file);
        HELD_FILE_LOCKS.with(|held| {
            held.borrow_mut().remove(&self.path);
        });
    }
}

/// Run one coherent per-photo store operation under a kernel-owned file lock.
/// The `.develop.lock` directory entry is persistent, but lock ownership is
/// not: the OS releases it when a process exits or is killed, so a crash
/// cannot leave a stale PID file or a permanent wait.
pub fn with_develop_lock<T, E>(
    src: &Path,
    mode: DevelopLockMode,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<io::Error>,
{
    with_develop_lock_in(&store_root(), src, mode, f)
}

pub(super) fn with_develop_lock_in<T, E>(
    root: &Path,
    src: &Path,
    mode: DevelopLockMode,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<io::Error>,
{
    // A session that fell back to the LEXICAL key (lock contention, or a
    // failed adoption — both deliberately stable for the session) can hold a
    // dir a concurrent process has since adopted away: every write after
    // that would land in the frozen alias backup and never be seen again.
    // The superseded marker is only trustworthy INSIDE the lock (its writer
    // holds this same lock), so probe it here and re-resolve once — the
    // canonical dir is never superseded, so one retry terminates.
    let mut f = Some(f);
    for _ in 0..2 {
        let dev = develop_dir_in(root, src);
        std::fs::create_dir_all(&dev).map_err(E::from)?;
        enum Gate<R> {
            Ran(R),
            Superseded,
            Fenced,
        }
        let ran: Result<Gate<Result<T, E>>, io::Error> =
            with_path_lock(dev.join(".develop.lock"), mode, || {
                if dev.join("superseded-by.txt").exists() {
                    return Ok(Gate::Superseded);
                }
                // The fence is checked AT THE AUTHORITY GATE, not only by the
                // memoized resolver probe (review R12-01): a session that
                // probed "no marker" before a concurrent adoption started —
                // and memoized it — must still refuse to run against the
                // half-copied dir. The resume runs OUTSIDE this lock (it
                // takes source→dest in the fixed order, in this caller's own
                // lock mode — a Wait surface queues behind a resume another
                // process is running) and the loop retries.
                if dev.join("adopting-from.txt").exists() {
                    return Ok(Gate::Fenced);
                }
                let body = f.take().expect("the develop-lock body runs once");
                Ok(Gate::Ran(body()))
            });
        match ran {
            Ok(Gate::Ran(out)) => return out,
            Ok(Gate::Superseded) => forget_resolved_key(root, src),
            Ok(Gate::Fenced) => {
                if let Err(e) = resume_marked_adoption(&dev, mode) {
                    return Err(E::from(io::Error::new(
                        e.kind(),
                        format!(
                            "this photo's develop dir is mid-adoption and finishing it failed \
                             ({e}) — the touch is refused rather than run against a fenced dir"
                        ),
                    )));
                }
            }
            Err(e) => return Err(E::from(e)),
        }
    }
    Err(E::from(io::Error::other(
        "this photo's develop directory is marked superseded by an adoption and re-resolving \
         did not converge - remove its superseded-by.txt by hand to proceed",
    )))
}

/// The shared engine of [`with_develop_lock`] and [`with_settings_lock`]: run
/// `f` under the kernel lock on `path`. Same-thread nesting on the same lock
/// path re-enters through the thread-local held set; other threads and other
/// processes contend through the kernel.
pub(super) fn with_path_lock<T, E>(
    path: PathBuf,
    mode: DevelopLockMode,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<io::Error>,
{
    if HELD_FILE_LOCKS.with(|held| held.borrow().contains(&path)) {
        return f();
    }

    // truncate(false) is the INTENT, not a default: the lock lives in the
    // kernel, not in the bytes, so this file's content is irrelevant and
    // truncating it would be a pointless write against a path another process
    // may hold open. `write(true)` is required because Windows needs write
    // access on the handle it locks.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(E::from)?;
    os_develop_lock::lock(&file, mode).map_err(E::from)?;
    HELD_FILE_LOCKS.with(|held| {
        held.borrow_mut().insert(path.clone());
    });
    let _guard = DevelopLockGuard { path, file };
    f()
}

/// Run one settings-file operation — a writer's read-modify-write cycle, or
/// the corrupt-file rescue — under a cross-process kernel lock
/// (`.settings.lock` in the store root). serve's old in-process
/// `SETTINGS_LOCK` Mutex serialized only its own threads: the GUI process and
/// the serve process each load-merge-save the same `autoshade.local.json`, so
/// one process's save landing between the other's load and rename was
/// silently erased — and the file carries the API keys (L01). Kernel-owned
/// like the develop lock: a crash releases it, so no stale-lock cleanup
/// exists or is needed.
pub fn with_settings_lock<T, E>(
    mode: DevelopLockMode,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<io::Error>,
{
    with_settings_lock_in(&store_root(), mode, f)
}

pub(super) fn with_settings_lock_in<T, E>(
    root: &Path,
    mode: DevelopLockMode,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<io::Error>,
{
    std::fs::create_dir_all(root).map_err(E::from)?;
    with_path_lock(root.join(".settings.lock"), mode, f)
}

#[cfg(unix)]
mod os_develop_lock {
    use super::{DevelopLockMode, File};
    use std::{
        io,
        os::fd::{AsRawFd as _, RawFd},
    };

    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;

    #[link(name = "c")]
    unsafe extern "C" {
        #[link_name = "flock"]
        fn libc_flock(fd: RawFd, operation: i32) -> i32;
    }

    pub(super) fn lock(file: &File, mode: DevelopLockMode) -> io::Result<()> {
        let operation =
            LOCK_EX | if mode == DevelopLockMode::NoWait { LOCK_NB } else { 0 };
        loop {
            // SAFETY: the descriptor remains owned by `DevelopLockGuard` for
            // the complete lock lifetime and `flock` does not retain pointers.
            if unsafe { libc_flock(file.as_raw_fd(), operation) } == 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    pub(super) fn unlock(file: &File) {
        // SAFETY: this is the same live descriptor successfully locked above.
        let _ = unsafe { libc_flock(file.as_raw_fd(), LOCK_UN) };
    }
}

#[cfg(windows)]
mod os_develop_lock {
    use super::{DevelopLockMode, File};
    use std::{
        ffi::c_void,
        io,
        os::windows::io::AsRawHandle as _,
        ptr,
    };

    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 1;
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 2;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    #[repr(C)]
    struct Overlapped {
        _internal: usize,
        _internal_high: usize,
        _offset: u32,
        _offset_high: u32,
        _event: *mut c_void,
    }

    impl Overlapped {
        fn zeroed() -> Self {
            Self {
                _internal: 0,
                _internal_high: 0,
                _offset: 0,
                _offset_high: 0,
                _event: ptr::null_mut(),
            }
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "LockFileEx"]
        fn lock_file_ex(
            file: *mut c_void,
            flags: u32,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        #[link_name = "UnlockFileEx"]
        fn unlock_file_ex(
            file: *mut c_void,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    pub(super) fn lock(file: &File, mode: DevelopLockMode) -> io::Result<()> {
        let mut overlapped = Overlapped::zeroed();
        let flags = LOCKFILE_EXCLUSIVE_LOCK
            | if mode == DevelopLockMode::NoWait {
                LOCKFILE_FAIL_IMMEDIATELY
            } else {
                0
            };
        // SAFETY: the handle and OVERLAPPED remain valid for the synchronous
        // call; the guard keeps the handle open until the matching unlock.
        let ok = unsafe {
            lock_file_ex(
                file.as_raw_handle(),
                flags,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if mode == DevelopLockMode::NoWait && e.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "this photo is being saved by another AutoShade process",
            ));
        }
        Err(e)
    }

    pub(super) fn unlock(file: &File) {
        let mut overlapped = Overlapped::zeroed();
        // SAFETY: this is the same live handle and byte range locked above.
        let _ = unsafe {
            unlock_file_ex(
                file.as_raw_handle(),
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };
    }
}

#[cfg(not(any(unix, windows)))]
mod os_develop_lock {
    use super::{DevelopLockMode, File};
    use std::io;

    pub(super) fn lock(_: &File, _: DevelopLockMode) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "develop locking is unsupported on this platform",
        ))
    }

    pub(super) fn unlock(_: &File) {}
}
