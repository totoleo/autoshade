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

/// The critic verdict carried by a harness summary or a deferred result.
/// Missing/unrecognised decisions fall back to Revise: the caller must never
/// treat an unreadable verdict as an Accept.
fn verdict_from(value: &serde_json::Value) -> Verdict {
    let decision = match value
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
    let score = value.get("score").and_then(serde_json::Value::as_i64);
    Verdict {
        decision,
        reasons: score
            .map(|s| vec![format!("critic score {s}/100")])
            .unwrap_or_default(),
        revised_hint: None,
    }
}

/// The canonical recipe from a deferred harness result. The harness writes the
/// output of `recipe.validate`, not the model's raw candidate (which carries
/// nulls the `EditRecipe` parser rejects).
fn recipe_from_deferred(result: &serde_json::Value) -> Result<EditRecipe> {
    serde_json::from_value(
        result
            .get("recipe")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the deferred result carries no recipe"))?,
    )
    .context("parse deferred recipe")
}

/// Is there a key the harness can use? Checked BEFORE spawning Node so a
/// missing key fails fast with the in-process chain's own wording. The rule
/// mirrors the legacy preflight: an explicitly configured API analysis role
/// with no analysis key must refuse, not silently borrow the image key.
pub fn key_present(cfg: &crate::config::Config) -> bool {
    if std::env::var("DEEPSEEK_API_KEY").is_ok_and(|v| !v.is_empty()) {
        return true;
    }
    if cfg
        .analysis_api_key
        .as_deref()
        .is_some_and(|k| !k.is_empty())
    {
        return true;
    }
    cfg.analysis_provider != "api"
        && cfg
            .openai_api_key
            .as_deref()
            .is_some_and(|k| !k.is_empty())
}

/// The Node executable: `AUTOSHADE_NODE` wins; otherwise a runtime bundled
/// beside the app (`<exe dir>\node\node.exe`, then `<exe dir>\node.exe`); then
/// PATH. The bundle is what the installer ships, so an installed app does not
/// depend on the user having Node.
fn node_exe_from(start: Option<&Path>) -> String {
    if let Some(dir) = start {
        for candidate in [dir.join("node").join("node.exe"), dir.join("node.exe")] {
            if candidate.is_file() {
                return candidate.display().to_string();
            }
        }
    }
    "node".into()
}

fn node_exe() -> String {
    if let Ok(node) = std::env::var("AUTOSHADE_NODE") {
        return node;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    node_exe_from(exe_dir.as_deref())
}

/// The ONE way this module starts Node.
///
/// Node keeps the stdio it inherits, because its stderr is the analyze trace
/// the CLI prints. From the windowed GUI there is no console to inherit:
/// Windows gives a console child a new console window of its own instead — the
/// cmd window that appeared when an AI button was pressed. So the console flag
/// is applied exactly when [`crate::has_console`] is false.
fn node_command(node: &str) -> ProcessCommand {
    let mut cmd = ProcessCommand::new(node);
    // Only the windowed GUI lacks a console; the flag is Windows-only.
    #[cfg(windows)]
    if !crate::has_console() {
        crate::hide_child_console(&mut cmd);
    }
    cmd
}

/// The analysis-role key the harness process must see. The harness IS the
/// analysis role: a DeepSeek key saved there is the one to send; the image
/// role's OpenAI key is only a fallback. An ambient env key wins (the wrapper
/// scripts set it), so nothing is overwritten.
fn apply_analysis_key(cmd: &mut ProcessCommand) {
    if std::env::var_os("DEEPSEEK_API_KEY").is_some() {
        return;
    }
    let cfg = crate::config::Config::load();
    if let Some(key) = cfg
        .analysis_api_key
        .as_deref()
        .or(cfg.openai_api_key.as_deref())
        && !key.is_empty()
    {
        cmd.env("DEEPSEEK_API_KEY", key);
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
    find_named_script("analyze.mjs")
}

/// `$AUTOSHADE_PI_HARNESS` names the analyze script; a sibling with this file
/// name is the override for any other harness entry (currently `complete.mjs`).
fn find_named_script(name: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AUTOSHADE_PI_HARNESS") {
        let p = PathBuf::from(p);
        let candidate = if name == "analyze.mjs" { p } else { p.with_file_name(name) };
        if candidate.is_file() {
            return Some(candidate);
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
            let candidate = cur.join("harness").join("pi").join(name);
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
    let node = node_exe();
    let mut cmd = node_command(&node);
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
    apply_analysis_key(&mut cmd);
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
        let recipe = recipe_from_deferred(&result)?;
        let verdict = verdict_from(&result);
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
    let verdict = verdict_from(&summary);
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

/// One one-shot model call served by pi instead of the in-process paid API.
/// `frames` are JPEGs already base64-encoded, in prompt order.
pub(crate) struct ModelCall {
    pub instruction: String,
    pub frames: Vec<crate::advisor::VisionFrame>,
    pub system: Option<String>,
    /// Optional JSON Schema the reply must take; `complete.mjs` appends it to the
    /// instruction and the caller keeps its own parser (single source).
    pub schema: Option<serde_json::Value>,
}

/// The request JSON `harness/pi/complete.mjs` consumes. A pure function so the
/// wire shape is unit-tested without spawning Node.
fn model_request_json(call: &ModelCall) -> serde_json::Value {
    serde_json::json!({
        "instruction": call.instruction,
        "system": call.system,
        "schema": call.schema,
        "frames": call.frames.iter().map(|f| serde_json::json!({
            "b64": f.b64,
            "mimeType": "image/jpeg",
            "detail": f.detail,
        })).collect::<Vec<_>>(),
    })
}

/// Run one model call through pi and return the model's text. `timeout_secs`
/// bounds the whole child process: the paid HTTP call had a per-call budget,
/// and the replacement must not park a worker thread forever either.
pub(crate) fn complete(call: &ModelCall, timeout_secs: u64) -> Result<String> {
    let script = find_named_script("complete.mjs").ok_or_else(|| {
        anyhow::anyhow!(
            "cannot find harness/pi/complete.mjs — pass AUTOSHADE_PI_HARNESS, or set \
             AUTOSHADE_PIPELINE=legacy to use the in-process chain"
        )
    })?;
    let store = crate::store::store_root();
    let sessions = store.join("harness").join("sessions");
    std::fs::create_dir_all(&sessions)
        .with_context(|| format!("create {}", sessions.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let request_path = sessions.join(format!(".model-req-{pid}-{stamp}.json"));
    let result_path = sessions.join(format!(".model-res-{pid}-{stamp}.json"));
    std::fs::write(
        &request_path,
        serde_json::to_vec(&model_request_json(call))?,
    )
    .with_context(|| format!("write {}", request_path.display()))?;

    let node = node_exe();
    let mut cmd = node_command(&node);
    cmd.arg(&script)
        .env("AUTOSHADE_MODEL_REQUEST", &request_path)
        .env("AUTOSHADE_MODEL_RESULT", &result_path)
        .env("AUTOSHADE_DATA_DIR", &store);
    apply_analysis_key(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("spawn {node}"))?;
    let status = wait_with_timeout(&mut child, timeout_secs);
    let text = std::fs::read_to_string(&result_path).ok();
    let _ = std::fs::remove_file(&request_path);
    let _ = std::fs::remove_file(&result_path);
    let status = status?;
    let Some(text) = text else {
        anyhow::bail!("the pi model call wrote no result ({status})");
    };
    let value: serde_json::Value = serde_json::from_str(&text).context("parse model result")?;
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string());
    }
    anyhow::bail!(
        "the pi model call failed: {}",
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error")
    )
}

/// Wait for `child`, killing it once `timeout_secs` elapse. `std::process` has
/// no wait-with-timeout, and the paid API's per-call budget must not become an
/// unbounded hang now that the call is a child process.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout_secs: u64,
) -> Result<std::process::ExitStatus> {
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() > std::time::Duration::from_secs(timeout_secs) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("the pi model call timed out after {timeout_secs}s");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_request_carries_the_instruction_frames_and_system() {
        let call = ModelCall {
            instruction: "describe the grade".into(),
            frames: vec![crate::advisor::VisionFrame { b64: "AAAA".into(), detail: "low" }],
            system: Some("be terse".into()),
            schema: Some(serde_json::json!({ "type": "object" })),
        };
        let value = model_request_json(&call);
        assert_eq!(value["instruction"], "describe the grade");
        assert_eq!(value["system"], "be terse");
        assert_eq!(value["frames"][0]["b64"], "AAAA");
        assert_eq!(value["frames"][0]["mimeType"], "image/jpeg");
        assert_eq!(value["schema"]["type"], "object");
    }

    #[test]
    fn a_model_call_without_system_or_frames_is_still_well_formed() {
        let call = ModelCall {
            instruction: "hi".into(),
            frames: Vec::new(),
            system: None,
            schema: None,
        };
        let value = model_request_json(&call);
        assert_eq!(value["instruction"], "hi");
        assert_eq!(value["frames"].as_array().map(Vec::len), Some(0));
        assert!(value["system"].is_null());
        assert!(value["schema"].is_null());
    }

    #[test]
    fn verdict_maps_accept_revise_reject_and_unknown() {
        let cases = [
            ("accept", Decision::Accept),
            ("ACCEPT", Decision::Accept),
            ("revise", Decision::Revise),
            ("reject", Decision::Reject),
            ("", Decision::Revise),
            ("garbage", Decision::Revise),
        ];
        for (raw, want) in cases {
            let v = verdict_from(&serde_json::json!({ "verdict": raw }));
            assert_eq!(v.decision, want, "verdict {raw:?}");
        }
        // A missing verdict is NOT an Accept: the caller must not save on it.
        assert_eq!(
            verdict_from(&serde_json::json!({})).decision,
            Decision::Revise
        );
    }

    #[test]
    fn verdict_carries_the_critic_score_as_a_reason() {
        let v = verdict_from(&serde_json::json!({ "verdict": "accept", "score": 87 }));
        assert_eq!(v.decision, Decision::Accept);
        assert_eq!(v.reasons, vec!["critic score 87/100".to_string()]);
        // No score -> no fabricated reason.
        assert!(verdict_from(&serde_json::json!({ "verdict": "accept" })).reasons.is_empty());
    }

    #[test]
    fn deferred_recipe_round_trips_the_default_recipe() {
        let recipe = EditRecipe::default();
        let value = serde_json::json!({
            "recipe": recipe,
            "verdict": "accept",
            "score": 90,
            "session": "analyze-test",
        });
        let parsed = recipe_from_deferred(&value).expect("the canonical recipe must parse");
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::to_value(&recipe).unwrap()
        );
    }

    #[test]
    fn deferred_recipe_refuses_missing_or_invalid_recipe() {
        assert!(recipe_from_deferred(&serde_json::json!({})).is_err());
        assert!(
            recipe_from_deferred(&serde_json::json!({ "recipe": { "exposure_ev": "not-a-number" } }))
                .is_err(),
            "a malformed recipe must be refused, not defaulted"
        );
    }

    #[test]
    fn a_bundled_node_beside_the_app_wins_over_path() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("node-{}-{}", std::process::id(), line!()));
        let nested = dir.join("node");
        std::fs::create_dir_all(&nested).unwrap();
        let exe = nested.join("node.exe");
        std::fs::write(&exe, b"fake").unwrap();
        assert_eq!(node_exe_from(Some(&dir)), exe.display().to_string());
        // A flat node.exe beside the app also counts, and no bundle falls back
        // to PATH ("node").
        let flat = dir.join("flat");
        std::fs::create_dir_all(&flat).unwrap();
        let flat_exe = flat.join("node.exe");
        std::fs::write(&flat_exe, b"fake").unwrap();
        assert_eq!(node_exe_from(Some(&flat)), flat_exe.display().to_string());
        assert_eq!(node_exe_from(Some(&dir.join("nothing"))), "node");
        assert_eq!(node_exe_from(None), "node");
        let _ = std::fs::remove_dir_all(&dir);
    }}
