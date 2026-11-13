//! The managed Python environment the AI sidecars run in.
//!
//! `uv venv` plus `uv pip install -r python/requirements-{cuda,macos}.txt` into
//! `<store root>/python-env`. Prepared by an EXPLICIT action — the desktop
//! app's Settings button, or `autoshade python-env --install` — never by a
//! background download, because the CUDA wheel set is a multi-gigabyte fetch.
//!
//! [`crate::config::Config::sidecar_python`] runs the sidecars in this
//! environment whenever the user has not named an interpreter themselves, so
//! the app never depends on whichever `python` happens to be first on `PATH`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment's directory under the store root.
pub const DIR_NAME: &str = "python-env";

/// The root of the managed environment. Beside the develop store, so
/// `AUTOSHADE_DATA_DIR` moves it with everything else.
pub fn env_dir() -> PathBuf {
    crate::store::store_root().join(DIR_NAME)
}

/// The interpreter inside a venv at `dir` — the venv layout of the platform.
pub fn python_in(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("Scripts").join("python.exe")
    } else {
        dir.join("bin").join("python3")
    }
}

/// What is at the managed environment's location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// The environment's interpreter exists.
    Ready(PathBuf),
    /// Nothing has been installed there yet.
    NotPrepared,
}

pub fn readiness_in(dir: &Path) -> Readiness {
    let python = python_in(dir);
    if python.is_file() { Readiness::Ready(python) } else { Readiness::NotPrepared }
}

pub fn readiness() -> Readiness {
    readiness_in(&env_dir())
}

/// The interpreter the managed environment provides, when it is prepared.
pub fn installed_python() -> Option<PathBuf> {
    readiness_in(&env_dir()).ready().cloned()
}

impl Readiness {
    pub fn ready(&self) -> Option<&PathBuf> {
        match self {
            Readiness::Ready(p) => Some(p),
            Readiness::NotPrepared => None,
        }
    }
}

/// The requirements file a machine installs.
///
/// Windows and Linux share `requirements-cuda.txt`: its torch wheel comes from
/// the CUDA index and runs on the CPU too, and a card-less machine pays nothing
/// but the download. macOS has its own file because the CUDA index has no
/// wheel for it.
pub fn requirements_rel() -> &'static str {
    if cfg!(target_os = "macos") {
        "python/requirements-macos.txt"
    } else {
        "python/requirements-cuda.txt"
    }
}

/// The wheel backend `uv` resolves torch through, matching the requirements
/// file. Without it `uv` takes torch from its default index, which on Windows
/// and Linux is the CPU build — the requirements file's `--extra-index-url` is
/// a pip spelling that loses to the higher version on the PyPI index.
pub fn torch_backend() -> Option<&'static str> {
    if cfg!(target_os = "macos") { None } else { Some("cu128") }
}

/// What the sidecar interpreter is, as a fact for startup reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PythonState {
    /// The managed environment's interpreter, ready.
    Managed(PathBuf),
    /// A Python 3 the user named.
    Interpreter,
    /// No managed environment and no interpreter named: the platform default
    /// answers, but the sidecars' packages are not known to be there.
    UnpreparedDefault,
    /// The resolved interpreter does not run Python 3 (or does not run).
    Unusable,
}

/// The startup check's answer: what the sidecars would run, and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonCheck {
    /// The interpreter the sidecars would launch.
    pub bin: String,
    pub state: PythonState,
    /// The interpreter's major version, when it answered at all.
    pub major: Option<u32>,
}

/// The major version `bin` reports, or `None` when it does not run within five
/// seconds.
///
/// `python --version` succeeds on Python 2 as well, so the check has to ASK the
/// interpreter rather than merely run it.
pub fn python_major(bin: &str) -> Option<u32> {
    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    let mut child = Command::new(bin)
        .args(["-c", "import sys;sys.stdout.write(str(sys.version_info[0]))"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            // Still running after the timeout, or try_wait failed: kill it and
            // report that the interpreter could not be measured.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    {
        use std::io::Read;
        let mut stdout = child.stdout.take()?;
        stdout.read_to_string(&mut out).ok()?;
    }
    out.trim().parse().ok()
}

/// The startup check: does the interpreter the sidecars would run actually run
/// Python 3, and is the managed environment the one answering?
pub fn inspect(cfg: &crate::config::Config) -> PythonCheck {
    inspect_with(
        &cfg.sidecar_python(),
        cfg.python_bin != crate::config::default_python_bin(),
        installed_python(),
    )
}

/// [`inspect`] with its inputs given.
pub fn inspect_with(bin: &str, named_by_user: bool, managed: Option<PathBuf>) -> PythonCheck {
    if let Some(managed) = managed
        && managed.to_string_lossy() == bin
    {
        return PythonCheck { bin: bin.to_string(), state: PythonState::Managed(managed), major: Some(3) };
    }
    let major = python_major(bin);
    let state = if major == Some(3) {
        if named_by_user { PythonState::Interpreter } else { PythonState::UnpreparedDefault }
    } else {
        PythonState::Unusable
    };
    PythonCheck { bin: bin.to_string(), state, major }
}

/// One prepared install: the program to run, where the environment lives and
/// the requirements file to install into it.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The `uv` program.
    pub uv: OsString,
    pub env_dir: PathBuf,
    pub python: PathBuf,
    pub requirements: PathBuf,
}

impl Plan {
    /// Resolve `uv` from `PATH` and the requirements file from this build's
    /// own tree. Every path is checked before anything runs, so a missing
    /// piece is a sentence rather than a spawn failure halfway in.
    pub fn resolve() -> anyhow::Result<Plan> {
        let requirements = PathBuf::from(crate::config::bundled_helper(requirements_rel()));
        if !requirements.is_file() {
            anyhow::bail!(
                "the sidecar requirements file is missing ({}) — this build did not ship python/",
                requirements.display()
            );
        }
        Ok(Plan::resolve_with(OsString::from("uv"), &env_dir(), &requirements))
    }

    /// [`resolve`](Self::resolve) with its inputs given (`uv` from `PATH` in
    /// production; a stand-in program in tests).
    pub fn resolve_with(uv: OsString, env_dir: &Path, requirements: &Path) -> Plan {
        Plan {
            uv,
            env_dir: env_dir.to_path_buf(),
            python: python_in(env_dir),
            requirements: requirements.to_path_buf(),
        }
    }

    /// `uv venv …` — Python 3.12 is what the sidecars were measured on. An
    /// existing environment at `env_dir` is reused (`--allow-existing`), so a
    /// second "prepare" repairs instead of refusing.
    pub fn venv_args(&self) -> Vec<OsString> {
        vec![
            OsString::from("venv"),
            OsString::from("--python"),
            OsString::from("3.12"),
            OsString::from("--allow-existing"),
            self.env_dir.clone().into_os_string(),
        ]
    }

    /// `uv pip install …` — the requirements file by ABSOLUTE path, run from
    /// its own directory so its nested `-r requirements-common.txt` resolves.
    /// `--torch-backend` is what makes the CUDA index win for torch (see
    /// [`torch_backend`]).
    pub fn install_args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("pip"),
            OsString::from("install"),
            OsString::from("--python"),
            self.python.clone().into_os_string(),
        ];
        if let Some(backend) = torch_backend() {
            args.push(OsString::from("--torch-backend"));
            args.push(OsString::from(backend));
        }
        args.push(OsString::from("-r"));
        args.push(self.requirements.clone().into_os_string());
        args
    }

    /// Create the environment and install the requirements into it.
    ///
    /// `uv`'s stderr is forwarded line by line (it is where pip's progress
    /// goes), so a caller can say what is happening during a long download.
    /// Returns the interpreter that is now ready.
    pub fn install(&self, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<PathBuf> {
        self.run(&self.venv_args(), None, on_line)
            .map_err(|e| anyhow::anyhow!("uv venv failed: {e}"))?;
        let cwd = self.requirements.parent();
        self.run(&self.install_args(), cwd, on_line)
            .map_err(|e| anyhow::anyhow!("uv pip install failed: {e}"))?;
        if !self.python.is_file() {
            anyhow::bail!(
                "uv reported success but {} does not exist",
                self.python.display()
            );
        }
        Ok(self.python.clone())
    }

    /// Run one `uv` step, streaming its stderr.
    fn run(
        &self,
        args: &[OsString],
        cwd: Option<&Path>,
        on_line: &mut dyn FnMut(&str),
    ) -> anyhow::Result<()> {
        let mut cmd = Command::new(&self.uv);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "`{}` was not found — install uv (https://docs.astral.sh/uv/) or put it on PATH",
                    self.uv.to_string_lossy()
                )
            } else {
                anyhow::anyhow!("spawn {}: {e}", self.uv.to_string_lossy())
            }
        })?;
        // stderr streams; stdout is drained on its own thread so a chatty
        // child can never fill one pipe while we read the other.
        let mut stderr = child.stderr.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let drained = std::thread::spawn(move || {
            use std::io::Read;
            let mut out = String::new();
            let mut f = std::io::BufReader::new(stdout);
            let _ = f.read_to_string(&mut out);
            out
        });
        {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(&mut stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) if !l.trim().is_empty() => on_line(l.trim_end()),
                    _ => {}
                }
            }
        }
        let status = child.wait()?;
        let out = drained.join().unwrap_or_default();
        if !status.success() {
            let tail: String = out.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            anyhow::bail!(
                "uv exited with {}{}",
                status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into()),
                if tail.trim().is_empty() { String::new() } else { format!(": {tail}") }
            );
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("autoshade-python-env-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_interpreter_follows_the_platform_venv_layout() {
        let dir = Path::new("env");
        let p = python_in(dir);
        if cfg!(windows) {
            assert_eq!(p, dir.join("Scripts").join("python.exe"));
        } else {
            assert_eq!(p, dir.join("bin").join("python3"));
        }
    }

    #[test]
    fn windows_and_linux_install_the_cuda_file_and_macos_its_own() {
        assert_eq!(
            requirements_rel(),
            if cfg!(target_os = "macos") {
                "python/requirements-macos.txt"
            } else {
                "python/requirements-cuda.txt"
            }
        );
    }

    #[test]
    fn a_prepared_environment_is_ready_and_an_empty_directory_is_not() {
        let dir = temp_dir("ready");
        assert_eq!(readiness_in(&dir), Readiness::NotPrepared);
        let py = python_in(&dir);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, b"").unwrap();
        assert_eq!(readiness_in(&dir), Readiness::Ready(py.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plan_names_the_environment_and_the_requirements_file() {
        let dir = temp_dir("plan");
        let req = dir.join("requirements-cuda.txt");
        std::fs::write(&req, "-r requirements-common.txt\ntorch\n").unwrap();
        let plan = Plan::resolve_with(OsString::from("uv"), &dir.join("env"), &req);
        assert_eq!(
            plan.venv_args(),
            vec![
                OsString::from("venv"),
                OsString::from("--python"),
                OsString::from("3.12"),
                OsString::from("--allow-existing"),
                dir.join("env").into_os_string(),
            ]
        );
        let mut want = vec![
            OsString::from("pip"),
            OsString::from("install"),
            OsString::from("--python"),
            python_in(&dir.join("env")).into_os_string(),
        ];
        if let Some(backend) = torch_backend() {
            want.push(OsString::from("--torch-backend"));
            want.push(OsString::from(backend));
        }
        want.push(OsString::from("-r"));
        want.push(req.clone().into_os_string());
        assert_eq!(plan.install_args(), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The CUDA build only arrives when `uv` is told which backend to use: its
    /// default index serves the CPU wheel, and the requirements file's
    /// `--extra-index-url` (a pip spelling) is not enough on its own.
    #[test]
    fn the_cuda_backend_is_named_for_uv_and_macos_names_none() {
        assert_eq!(torch_backend(), if cfg!(target_os = "macos") { None } else { Some("cu128") });
    }

    /// The whole install, against a stand-in `uv` that creates the platform's
    /// venv layout for `venv` and succeeds silently for `pip install`. The
    /// argument positions the stand-in relies on are pinned by
    /// `the_plan_names_…` above.
    #[test]
    fn install_runs_venv_then_pip_and_leaves_a_ready_environment() {
        let dir = temp_dir("install");
        let req = dir.join("requirements-cuda.txt");
        std::fs::write(&req, "torch\n").unwrap();
        let env = dir.join("env");
        let py = python_in(&env);
        let parent = py.parent().unwrap().display().to_string();
        let py_path = py.display().to_string();
        // `%5` / `$5` is the environment directory (last argument of `venv`).
        let bat = format!(
            "@echo off\r\nif \"%1\"==\"venv\" (\r\n  mkdir \"%~5\" 2>nul\r\n  mkdir \"{parent}\" 2>nul\r\n  echo.> \"{py_path}\"\r\n  exit /b 0\r\n)\r\nif \"%1\"==\"pip\" exit /b 0\r\nexit /b 2\r\n"
        );
        let sh = format!(
            "#!/bin/sh\nif [ \"$1\" = venv ]; then mkdir -p \"$5\" \"{parent}\" && : > \"{py_path}\"; exit 0; fi\nif [ \"$1\" = pip ]; then exit 0; fi\nexit 2\n"
        );
        let uv = crate::write_stand_in(&dir, "uv", &bat, &sh);
        let plan = Plan::resolve_with(OsString::from(uv), &env, &req);
        let mut lines = Vec::new();
        let got = plan.install(&mut |l| lines.push(l.to_string())).unwrap();
        assert_eq!(got, py);
        assert_eq!(readiness_in(&env), Readiness::Ready(py));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_probe_reads_the_major_version_a_python_reports() {
        let dir = temp_dir("probe");
        let py3 = crate::write_stand_in(&dir, "py3", "@echo off\r\necho 3\r\n", "#!/bin/sh\necho 3\n");
        assert_eq!(python_major(&py3), Some(3));
        let py2 = crate::write_stand_in(&dir, "py2", "@echo off\r\necho 2\r\n", "#!/bin/sh\necho 2\n");
        assert_eq!(python_major(&py2), Some(2));
        let bad = crate::write_stand_in(&dir, "bad", "@echo off\r\nexit /b 3\r\n", "#!/bin/sh\nexit 3\n");
        assert_eq!(python_major(&bad), None, "a program that exits non-zero is not measured");
        assert_eq!(python_major(&dir.join("no-such-python").display().to_string()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_startup_check_names_the_resolution_it_found() {
        let dir = temp_dir("inspect");
        let managed = python_in(&dir.join("env"));
        std::fs::create_dir_all(managed.parent().unwrap()).unwrap();
        std::fs::write(&managed, b"").unwrap();
        // The managed interpreter is trusted without a probe: uv built it as 3.12.
        let check = inspect_with(&managed.to_string_lossy(), false, Some(managed.clone()));
        assert_eq!(check.state, PythonState::Managed(managed));
        let py3 = crate::write_stand_in(&dir, "py3", "@echo off\r\necho 3\r\n", "#!/bin/sh\necho 3\n");
        assert_eq!(inspect_with(&py3, true, None).state, PythonState::Interpreter);
        assert_eq!(inspect_with(&py3, false, None).state, PythonState::UnpreparedDefault);
        let py2 = crate::write_stand_in(&dir, "py2", "@echo off\r\necho 2\r\n", "#!/bin/sh\necho 2\n");
        assert_eq!(inspect_with(&py2, false, None).state, PythonState::Unusable);
        assert_eq!(inspect_with(&py2, true, None).major, Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn install_reports_a_missing_uv_as_that_and_not_as_a_panic() {
        let dir = temp_dir("no-uv");
        let req = dir.join("requirements-cuda.txt");
        std::fs::write(&req, "torch\n").unwrap();
        let plan = Plan::resolve_with(OsString::from(dir.join("no-such-uv")), &dir.join("env"), &req);
        let e = plan.install(&mut |_| {}).unwrap_err().to_string();
        assert!(e.contains("uv venv failed"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
