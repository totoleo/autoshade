//! The pi harness bridge: the analysis chain when the model loop lives in
//! `harness/pi/analyze.mjs` instead of `pipeline::produce_recipe`.
//!
//! S3 of `wishes/harness-refactor/PLAN.md`: `analyze` / `auto` / `batch` /
//! `serve` call this by default; `AUTOSHADE_PIPELINE=legacy` restores the old
//! in-process chain. The harness commits the develop itself (its
//! `recipe.commit` tool), so the callers here only read it back, print it, and
//! render when their own contract asks for a deliverable.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use crate::advisor::{Decision, Verdict};
use crate::recipe::EditRecipe;

/// One completed harness run, read back from the summary the harness writes.
pub struct Run {
    pub summary: serde_json::Value,
    pub recipe: EditRecipe,
    pub verdict: Verdict,
    pub develop: PathBuf,
    pub xmp: Option<PathBuf>,
    pub image: Option<PathBuf>,
    pub session: String,
}

impl Run {
    pub fn score(&self) -> Option<i64> {
        self.summary.get("score").and_then(serde_json::Value::as_i64)
    }
}

/// Which analyze chain this process uses. Default is the harness; the legacy
/// chain stays reachable through `AUTOSHADE_PIPELINE=legacy` (the rollback the
/// plan's S3 requires), and that is the ONLY opt-out.
pub fn enabled() -> bool {
    !std::env::var("AUTOSHADE_PIPELINE")
        .map(|v| v.eq_ignore_ascii_case("legacy"))
        .unwrap_or(false)
}

/// `$AUTOSHADE_PI_HARNESS`, else `harness/pi/analyze.mjs` above this binary or
/// the cwd (the dev checkout's layout).
pub fn find_script() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AUTOSHADE_PI_HARNESS") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        starts.push(dir.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    for start in starts {
        let mut cur = start.as_path();
        for _ in 0..8 {
            let candidate = cur.join("harness").join("pi").join("analyze.mjs");
            if candidate.is_file() {
                return Some(candidate);
            }
            match cur.parent() {
                Some(parent) if parent != cur => cur = parent,
                _ => break,
            }
        }
    }
    None
}

/// The CLI binary that speaks the `mcp` subcommand. `current_exe()` is NOT it
/// when the caller is `autoshade-gui` or a test binary — those have no `mcp`
/// subcommand, and the harness died with "Unrecognized option: 'root'".
fn cli_exe() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("AUTOSHADE_MCP_EXE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in ["autoshade.exe", "autoshade"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!(
        "cannot find the autoshade CLI beside {} — set AUTOSHADE_MCP_EXE to it",
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
    )
}

/// Run the harness on `photo` and read its committed develop back.
pub fn run(photo: &Path) -> Result<Run> {
    run_inner(photo, false)
}

/// Like [`run`], but the harness does NOT commit. It hands the final recipe
/// back through `AUTOSHADE_HARNESS_RESULT`; persistence belongs to the caller.
/// The desktop GUI uses this so its `on_analyzed` backup/undo contract is the
/// only writer (no duplicate `v<N>` snapshot from a second commit).
pub fn run_deferred(photo: &Path) -> Result<Run> {
    run_inner(photo, true)
}

fn run_inner(photo: &Path, defer: bool) -> Result<Run> {
    let photo = photo
        .canonicalize()
        .with_context(|| format!("photo {}", photo.display()))?;
    if !photo.is_file() {
        anyhow::bail!("not a file: {}", photo.display());
    }
    let root = photo
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", photo.display()))?;
    let script = find_script().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot find harness/pi/analyze.mjs — pass AUTOSHADE_PI_HARNESS, or set \
             AUTOSHADE_PIPELINE=legacy to use the in-process chain"
        )
    })?;
    let script = script
        .canonicalize()
        .with_context(|| format!("harness {}", script.display()))?;
    let exe = cli_exe()?;
    let store = crate::store::store_root();
    let data = store.join("harness");
    let sessions = data.join("sessions");
    std::fs::create_dir_all(&sessions)
        .with_context(|| format!("create {}", sessions.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let summary_path = sessions.join(format!(".summary-{}-{stamp}.json", std::process::id()));
    let result_path = sessions.join(format!(".result-{}-{stamp}.json", std::process::id()));
    let node = std::env::var("AUTOSHADE_NODE").unwrap_or_else(|_| "node".into());
    let mut cmd = ProcessCommand::new(&node);
    cmd.arg(&script);
    cmd.arg(photo.file_name().unwrap_or(photo.as_os_str()));
    cmd.env("AUTOSHADE_PHOTO_ROOT", root);
    cmd.env("AUTOSHADE_MCP_EXE", &exe);
    cmd.env("AUTOSHADE_MCP_DATA", &data);
    cmd.env("AUTOSHADE_DATA_DIR", &store);
    cmd.env("AUTOSHADE_SESSION_SUMMARY", &summary_path);
    if defer {
        cmd.env("AUTOSHADE_HARNESS_DEFER_COMMIT", "1");
        cmd.env("AUTOSHADE_HARNESS_RESULT", &result_path);
    }
    // The harness's Python default is the checkout's `.venv`; only override it
    // with the configured interpreter when that venv is absent (a configured
    // bare `python` on this machine can be Python 2, which cannot run the
    // segment sidecar — the DSC06817 run lost its AI mask that way).
    if std::env::var_os("AUTOSHADE_PYTHON").is_none() {
        let venv = script
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::parent)
            .map(|repo| repo.join(".venv").join("Scripts").join("python.exe"));
        match venv {
            Some(python) if python.is_file() => {
                cmd.env("AUTOSHADE_PYTHON", python);
            }
            _ => {
                let cfg = crate::config::Config::load();
                if !cfg.python_bin.is_empty() {
                    cmd.env("AUTOSHADE_PYTHON", &cfg.python_bin);
                }
            }
        }
    }
    if std::env::var_os("DEEPSEEK_API_KEY").is_none() {
        let cfg = crate::config::Config::load();
        // The harness IS the analysis role: a DeepSeek key saved there is the
        // one to send. The image role's OpenAI key is only a fallback (the
        // Settings panel's DeepSeek preset writes the analysis fields).
        if let Some(key) = cfg
            .analysis_api_key
            .as_deref()
            .or(cfg.openai_api_key.as_deref())
            && !key.is_empty()
        {
            cmd.env("DEEPSEEK_API_KEY", key);
        }
    }
    let status = cmd.status().with_context(|| format!("spawn {node}"))?;
    if !status.success() {
        anyhow::bail!("the pi harness failed ({status})");
    }
    let text = std::fs::read_to_string(&summary_path).with_context(|| {
        format!(
            "the harness finished but wrote no summary at {}",
            summary_path.display()
        )
    })?;
    let summary: serde_json::Value = serde_json::from_str(&text).context("parse harness summary")?;
    let _ = std::fs::remove_file(&summary_path);
    if defer {
        let result_text = std::fs::read_to_string(&result_path).with_context(|| {
            format!(
                "the deferred harness wrote no result at {}",
                result_path.display()
            )
        })?;
        let _ = std::fs::remove_file(&result_path);
        let result: serde_json::Value =
            serde_json::from_str(&result_text).context("parse deferred result")?;
        let recipe: EditRecipe = serde_json::from_value(
            result
                .get("recipe")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("the deferred result carries no recipe"))?,
        )
        .context("parse deferred recipe")?;
        let decision = match result
            .get("verdict")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "accept" => Decision::Accept,
            "reject" => Decision::Reject,
            _ => Decision::Revise,
        };
        let score = result.get("score").and_then(serde_json::Value::as_i64);
        let verdict = Verdict {
            decision,
            reasons: score
                .map(|s| vec![format!("critic score {s}/100")])
                .unwrap_or_default(),
            revised_hint: None,
        };
        let develop = crate::store::develop_dir(&photo);
        let image = summary
            .get("image")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let session = result
            .get("session")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        return Ok(Run {
            summary,
            recipe,
            verdict,
            develop,
            xmp: None,
            image,
            session,
        });
    }
    let develop = summary
        .get("develop")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("the harness summary names no develop: {text}"))?;
    let recipe_path = develop.join("recipe.json");
    let recipe_text = std::fs::read_to_string(&recipe_path)
        .with_context(|| format!("read committed recipe {}", recipe_path.display()))?;
    let recipe: EditRecipe = serde_json::from_str(&recipe_text).context("parse committed recipe")?;
    let decision = match summary
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "accept" => Decision::Accept,
        "reject" => Decision::Reject,
        _ => Decision::Revise,
    };
    let score = summary.get("score").and_then(serde_json::Value::as_i64);
    let verdict = Verdict {
        decision,
        reasons: score
            .map(|s| vec![format!("critic score {s}/100")])
            .unwrap_or_default(),
        revised_hint: None,
    };
    let xmp = summary
        .get("xmp")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let image = summary
        .get("image")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let session = summary
        .get("session")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(Run {
        summary,
        recipe,
        verdict,
        develop,
        xmp,
        image,
        session,
    })
}

/// `analyze` through the harness: the harness commits the develop itself, so
/// this prints the committed recipe/verdict and copies to `-o` when asked.
pub fn cli_analyze(raw: &Path, out: Option<PathBuf>) -> Result<()> {
    if let Some(o) = &out {
        crate::pipeline::preflight_out(o, raw)?;
    }
    let run = run(raw)?;
    println!("\n--- proposed recipe ---");
    println!("{}", serde_json::to_string_pretty(&run.recipe)?);
    println!("\n--- verdict: {:?} ---", run.verdict.decision);
    for reason in &run.verdict.reasons {
        println!("  - {reason}");
    }
    if let Some(o) = out {
        std::fs::create_dir_all(o.parent().unwrap_or(Path::new(".")))
            .with_context(|| format!("create {}", o.display()))?;
        std::fs::write(&o, serde_json::to_string_pretty(&run.recipe)?)
            .with_context(|| format!("write {}", o.display()))?;
        println!("\nrecipe -> {}", o.display());
        if crate::decode::is_raw(raw)
            && let Some(xmp) = &run.xmp
        {
            let side = o.with_extension("xmp");
            if side != o {
                std::fs::copy(xmp, &side)
                    .with_context(|| format!("copy XMP to {}", side.display()))?;
                println!("xmp    -> {}", side.display());
            }
        }
    } else {
        println!("\nrecipe -> {}", run.develop.join("recipe.json").display());
        if let Some(xmp) = &run.xmp {
            println!("xmp    -> {}", xmp.display());
        }
        if let Some(image) = &run.image {
            println!("image  -> {}", image.display());
        }
        if !run.session.is_empty() {
            println!("session-> {}", run.session);
        }
    }
    if run.verdict.decision != Decision::Accept {
        println!(
            "\nthe critic's verdict is {:?}; the harness already committed its best candidate \
             (that is its contract) — set AUTOSHADE_PIPELINE=legacy for the old \
             non-Accept-never-saves behaviour.",
            run.verdict.decision
        );
    }
    Ok(())
}
