//! Local ComfyUI backend through the official `comfy` CLI.
//!
//! This is the S5.11 replacement for the OpenAI Images/Edits calls of the
//! generative path. autoshade owns the workflow JSON (the CLI's `run
//! --workflow` does not accept `--set`), stages the input image, runs the
//! workflow on the local server and asks comfy-cli to download its outputs.
//!
//! Path management is ours, not ComfyUI's defaults: the server this module
//! launches gets `--base-directory` (models), and `--input-directory /
//! --output-directory / --temp-directory` under the app store, so generated
//! pixels never land in ComfyUI's own folders. The uploaded input and the
//! fetched output file are removed after each run.
//!
//! Settings live in the per-user `autoshade.local.json` under the `comfy` key
//! (they used to live in a sibling `comfy.local.json`) and can be overridden by
//! env vars; the GUI's Settings panel is the surface for them. Fields: cli, workspace (dir containing main.py), base_dir (models),
//! input_dir, output_dir, temp_dir, checkpoint, max_edge.
//!
//! Gate: `AUTOSHADE_GENERATIVE=comfy` until the real-photo acceptance run is
//! green; the default stays on the old path until S5.11 closes out.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Whether the generative path runs through ComfyUI. Now the DEFAULT: the paid
/// image API was deleted (S5.11). `AUTOSHADE_GENERATIVE=legacy` only keeps the
/// old in-process *analysis* oracle selectable; generative pixels always use
/// the local server.
pub fn enabled() -> bool {
    std::env::var("AUTOSHADE_GENERATIVE")
        .map(|v| !v.eq_ignore_ascii_case("legacy"))
        .unwrap_or(true)
}

/// Persistent ComfyUI settings. Empty strings mean "derive the default"; the
/// env overrides are read on every load so a wrapper/CI can point the app at a
/// different install without editing the JSON.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub cli: String,
    /// Directory containing ComfyUI's main.py (what `comfy launch` needs).
    #[serde(default)]
    pub workspace: String,
    /// ComfyUI `--base-directory` (models live under `<base>/models`, e.g. the
    /// base directory the Comfy Desktop install points at).
    #[serde(default)]
    pub base_dir: String,
    #[serde(default)]
    pub input_dir: String,
    #[serde(default)]
    pub output_dir: String,
    #[serde(default)]
    pub temp_dir: String,
    #[serde(skip)]
    pub checkpoint: String,
    #[serde(default)]
    pub max_edge: Option<u32>,
    /// ONE model manifest. Shape:
    /// `{"qwen":{"unet":..,"clip":..,"vae":..},"zimage":{..},"lama":..,"sdxl_inpaint":..}`
    #[serde(default)]
    pub models: serde_json::Value,
    /// Per-task configuration: ONE workflow each. Placeholders:
    /// {{IMAGE}}, {{MASK}}, {{PROMPT}}, {{NEGATIVE}}, {{SEED}}.
    #[serde(default)]
    pub reimagine_workflow: String,
    #[serde(default)]
    pub fill_workflow: String,
    // Resolved from `models`; not separate config slots.
    #[serde(skip)]
    pub lama_model: String,
    #[serde(skip)]
    pub sdxl_inpaint_checkpoint: String,
    #[serde(skip)]
    pub qwen_unet: String,
    #[serde(skip)]
    pub qwen_clip: String,
    #[serde(skip)]
    pub qwen_vae: String,
    #[serde(skip)]
    pub zimage_unet: String,
    #[serde(skip)]
    pub zimage_clip: String,
    #[serde(skip)]
    pub zimage_vae: String,
}

impl Settings {
    pub fn path() -> PathBuf {
        crate::config::local_settings_path()
    }

    pub fn load() -> Self {
        let bundled = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|root| root.get("comfy").cloned())
            .and_then(|value| serde_json::from_value::<Settings>(value).ok());
        let legacy = std::fs::read_to_string(crate::store::store_root().join("comfy.local.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok());
        let mut settings = bundled.or(legacy).unwrap_or_default();
        if let Some(v) = env("AUTOSHADE_COMFY_CLI") { settings.cli = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_WORKSPACE") { settings.workspace = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_BASE_DIR") { settings.base_dir = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_MAX_EDGE").and_then(|v| v.parse().ok()) {
            settings.max_edge = Some(v);
        }
        if let Some(v) = env("AUTOSHADE_COMFY_REIMAGINE_WORKFLOW") { settings.reimagine_workflow = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_FILL_WORKFLOW") { settings.fill_workflow = v; }
        settings.fill_defaults();
        settings
    }

    /// Persist exactly the values the caller wants (no default filling): the
    /// settings surface owns the file, `load` derives the rest at every use.
    pub fn save(&self) -> Result<()> {
        let value = serde_json::to_value(self)?;
        crate::config::update_local_settings(|cur| cur.comfy = Some(value))
            .context("save ComfyUI settings into autoshade.local.json")?;
        let legacy = crate::store::store_root().join("comfy.local.json");
        if legacy.exists() {
            let backup = legacy.with_extension("local.json.migrated");
            let _ = std::fs::rename(&legacy, backup);
        }
        Ok(())
    }

    fn fill_defaults(&mut self) {
        let get = |path: &str| {
            self.models
                .pointer(path)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        if self.checkpoint.trim().is_empty() {
            self.checkpoint = get("/sd15");
        }
        self.lama_model = get("/lama");
        self.sdxl_inpaint_checkpoint = get("/sdxl_inpaint");
        self.qwen_unet = get("/qwen/unet");
        self.qwen_clip = get("/qwen/clip");
        self.qwen_vae = get("/qwen/vae");
        self.zimage_unet = get("/zimage/unet");
        self.zimage_clip = get("/zimage/clip");
        self.zimage_vae = get("/zimage/vae");
        // ComfyUI's own base-directory convention: models/, input/, output/
        // and temp/ live under the base directory. Do not invent a parallel
        // app-owned tree; only base_dir is configured.
        let base_path = Path::new(&self.base_dir);
        if self.input_dir.is_empty() {
            self.input_dir = base_path.join("input").display().to_string();
        }
        if self.output_dir.is_empty() {
            self.output_dir = base_path.join("output").display().to_string();
        }
        if self.temp_dir.is_empty() {
            self.temp_dir = base_path.join("temp").display().to_string();
        }
        if self.max_edge.is_none() {
            self.max_edge = Some(1024);
        }
    }

    /// Checkpoint for the workflow (env/settings override the SD1.5 default).
    pub fn checkpoint_name(&self) -> String {
        self.checkpoint.clone()
    }

    pub fn max_edge_value(&self) -> u32 {
        self.max_edge.unwrap_or(1024)
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// One img2img request. The caller owns the prompt and the pixel size; this
/// module owns only the wire workflow.
pub struct Img2Img {
    pub checkpoint: String,
    pub negative: String,
    pub steps: u32,
    pub cfg: f32,
    pub denoise: f32,
    pub seed: u64,
}

pub fn checkpoint() -> String {
    Settings::load().checkpoint_name()
}

pub fn max_edge() -> u32 {
    Settings::load().max_edge_value()
}

/// Quality tier -> sampler steps (SD1.5-class defaults; `AUTOSHADE_COMFY_STEPS`
/// overrides for another checkpoint's speed).
pub fn steps_for(quality: &str) -> u32 {
    if let Ok(s) = std::env::var("AUTOSHADE_COMFY_STEPS")
        && let Ok(n) = s.parse()
    {
        return n;
    }
    match quality {
        "low" => 12,
        "high" => 28,
        _ => 20,
    }
}

/// Fidelity tier -> img2img denoise: high fidelity keeps the source structure.
pub fn denoise_for(fidelity: &str) -> f32 {
    match fidelity {
        "high" => 0.35,
        "low" => 0.75,
        _ => 0.55,
    }
}

/// A Comfy-friendly output size: same aspect, long edge <= `max_edge`, both
/// edges multiples of 8 (VAEEncode's requirement), never upscaled.
pub fn plan_size((w, h): (u32, u32), max_edge: u32) -> (u32, u32) {
    plan_size_aligned((w, h), max_edge, 8)
}

/// Same as [`plan_size`] with a model-specific alignment. Qwen-Image 2.1's
/// sampler/latent path rounds to 32, so a requested 1024x680 silently comes
/// back 1024x672 and the caller's exact-size contract would reject it.
pub fn plan_size_aligned((w, h): (u32, u32), max_edge: u32, align: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (512, 512);
    }
    let align = align.max(8);
    let long = w.max(h) as f64;
    let scale = (max_edge as f64 / long).min(1.0);
    let round = |v: f64| -> u32 {
        let n = (v / align as f64).floor() * align as f64;
        n.max(64.0).max(align as f64) as u32
    };
    (round(w as f64 * scale), round(h as f64 * scale))
}

/// The alignment the configured model's latent path actually uses.
pub fn plan_size_for_model(frame: (u32, u32), max_edge: u32) -> (u32, u32) {
    // Model-agnostic: workflows own their latent alignment; the app only
    // constrains the frame to a safe multiple of 8 and the edge cap.
    plan_size_aligned(frame, max_edge, 32)
}

struct Cli {
    exe: String,
    workspace: Option<PathBuf>,
    settings: Settings,
}

impl Cli {
    fn resolve() -> Self {
        let settings = Settings::load();
        let exe = if settings.cli.is_empty() { "comfy".into() } else { settings.cli.clone() };
        let mut workspace = (!settings.workspace.is_empty()).then(|| PathBuf::from(&settings.workspace));
        // comfy-cli knows the workspace and the running server; ask it instead
        // of reading Comfy Desktop's private registry or guessing a port.
        if workspace.is_none() {
            let probe = Self { exe: exe.clone(), workspace: None, settings: settings.clone() };
            if let Ok(data) = probe.json_owned(&["env".into()]) {
                workspace = data.pointer("/workspace/path")
                    .and_then(Value::as_str)
                    .or_else(|| data.pointer("/config/recent_workspace").and_then(Value::as_str))
                    .map(PathBuf::from);
            }
        }
        Self { exe, workspace, settings }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.exe);
        if let Some(workspace) = &self.workspace {
            cmd.arg(format!("--workspace={}", workspace.display()));
        }
        // The CLI can otherwise emit analytics from a product path.
        cmd.env("COMFY_NO_TELEMETRY", "1").env("DO_NOT_TRACK", "1");
        // comfy-cli's background launcher re-invokes `comfy`; when this app was
        // pointed at the tool by absolute path (~/.local/bin not on PATH), that
        // child would otherwise fail with "comfy is not recognized". Keep the
        // CLI's own directory at the front of the child PATH.
        if let Some(dir) = Path::new(&self.exe).parent().filter(|d| d.is_dir()) {
            let mut paths = vec![dir.to_path_buf()];
            if let Some(current) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&current));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        cmd
    }

    /// One `comfy --json <args>` call: parse the envelope/1 document printed on
    /// stdout (warnings may precede it) and return `data`, refusing `ok:false`.
    fn json_owned(&self, args: &[String]) -> Result<Value> {
        let mut all: Vec<String> = vec!["--json".into()];
        all.extend(args.iter().cloned());
        let mut command = self.command();
        let output = command
            .args(&all)
            .output()
            .with_context(|| format!("spawn {} (install it with `uv tool install comfy-cli`)", self.exe))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let line = stdout
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('{'))
            .with_context(|| {
                format!(
                    "comfy {} printed no JSON envelope; stderr: {}",
                    args.join(" "),
                    stderr.trim()
                )
            })?;
        let envelope: Value = serde_json::from_str(line)
            .with_context(|| format!("parse comfy envelope for {}", args.join(" ")))?;
        if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
            bail!(
                "comfy {} failed: {}",
                args.join(" "),
                envelope.get("error").cloned().unwrap_or(Value::Null)
            );
        }
        Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
    }

    fn json(&self, args: &[&str]) -> Result<Value> {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        self.json_owned(&owned)
    }

    /// Whether comfy-cli sees a running server.
    fn server_running(&self) -> bool {
        self.json(&["env"])
            .ok()
            .and_then(|data| data.pointer("/server/running").and_then(Value::as_bool))
            == Some(true)
    }

    /// A running server is reused; otherwise one is launched through comfy-cli
    /// (the management half of the CLI) with OUR directories, so models come
    /// from the configured base and generated pixels never land in ComfyUI's
    /// own output folder; subsequent calls go through comfy-cli's configured target.
    fn ensure_server(&self) -> Result<()> {
        if self.server_running() {
            return Ok(());
        }
        if self.workspace.is_none() {
            bail!(
                "no ComfyUI server and no workspace to launch one — set AUTOSHADE_COMFY_WORKSPACE \
                 (dir containing main.py) or start ComfyUI yourself"
            );
        }
        let settings = &self.settings;
        for dir in [&settings.input_dir, &settings.output_dir, &settings.temp_dir] {
            if !dir.is_empty() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("create ComfyUI directory {dir}"))?;
            }
        }
        let mut args: Vec<String> = vec!["launch".into(), "--background".into(), "--".into()];
        if !settings.base_dir.is_empty() {
            args.push("--base-directory".into());
            args.push(settings.base_dir.clone());
        }
        self.json_owned(&args)
            .context("comfy launch --background")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while std::time::Instant::now() < deadline {
            if self.server_running() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        bail!("ComfyUI did not become ready within 180 s after comfy launch --background")
    }
}

pub fn whole_frame(input_png: &[u8], prompt: &str, _opts: &Img2Img, _quality: &str) -> Result<Vec<u8>> {
    let settings = Settings::load();
    if settings.reimagine_workflow.trim().is_empty() {
        bail!("Reimagine workflow is not configured — set Settings → ComfyUI → Reimagine workflow");
    }
    custom_workflow(input_png, None, prompt, settings.reimagine_workflow.trim())
}

/// Masked task: ONE configured workflow. No model fallback.
pub fn edit_png(input_png: &[u8], mask_png: Option<&[u8]>, prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let settings = Settings::load();
    let path = if mask_png.is_some() {
        settings.fill_workflow.trim()
    } else {
        settings.reimagine_workflow.trim()
    };
    if path.is_empty() {
        bail!("workflow is not configured — set Settings → ComfyUI → Reimagine/Fill workflow");
    }
    custom_workflow(input_png, mask_png, prompt, path)
}

/// The 8-step Qwen-Image 2.1 turbo sigma schedule (official template).
fn upload_file(cli: &Cli, path: &Path) -> Result<String> {
    let data = cli
        .json(&["upload", &path.to_string_lossy()])
        .context("comfy upload")?;
    data.pointer("/uploads/0/cloud_name")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/uploads/0/name").and_then(Value::as_str))
        .map(str::to_owned)
        .with_context(|| format!("comfy upload returned no file name: {data}"))
}



/// Task-specific, model-agnostic workflow runner. Each task configures ONE
/// workflow; no model names are known here.
pub fn custom_workflow(input_png: &[u8], mask_png: Option<&[u8]>, prompt: &str, path: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("custom-in-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png).context("write custom input")?;
    let image = upload_file(&cli, &input_path)?;
    let mask = match mask_png {
        Some(bytes) => {
            let mp = dir.join(format!("custom-mask-{}-{stamp}.png", std::process::id()));
            let rgba = image::load_from_memory(bytes).context("decode custom mask")?.to_rgba8();
            let (mw, mh) = rgba.dimensions();
            let mut binary = image::GrayImage::new(mw, mh);
            for (x, y, px) in rgba.enumerate_pixels() {
                binary.put_pixel(x, y, image::Luma([if px[3] == 0 { 255 } else { 0 }]));
            }
            let mut png = Vec::new();
            image::DynamicImage::ImageLuma8(binary)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .context("encode custom mask")?;
            std::fs::write(&mp, &png).context("write custom mask")?;
            let name = upload_file(&cli, &mp)?;
            let _ = std::fs::remove_file(&mp);
            Some(name)
        }
        None => None,
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("read workflow {path}"))?;
    let mut wf: Value = serde_json::from_str(&text).with_context(|| format!("parse workflow {path}"))?;
    replace_placeholders(&mut wf, &[
        ("{{IMAGE}}", image.clone()),
        ("{{MASK}}", mask.clone().unwrap_or_default()),
        ("{{PROMPT}}", prompt.to_string()),
        ("{{NEGATIVE}}", String::new()),
        ("{{SEED}}", stamp.to_string()),
    ]);
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    if let Some(mask) = mask {
        let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(mask));
    }
    bytes
}

fn replace_placeholders(value: &mut Value, vars: &[(&str, String)]) {
    match value {
        Value::String(s) => { for (k, v) in vars { *s = s.replace(k, v); } }
        Value::Array(items) => items.iter_mut().for_each(|v| replace_placeholders(v, vars)),
        Value::Object(map) => map.values_mut().for_each(|v| replace_placeholders(v, vars)),
        _ => {}
    }
}

fn run_workflow_json(cli: &Cli, settings: &Settings, wf: &Value) -> Result<Vec<u8>> {
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let workflow_path = dir.join(format!("wf-{}-{stamp}.json", std::process::id()));
    std::fs::write(&workflow_path, serde_json::to_vec(wf)?)
        .with_context(|| format!("write {}", workflow_path.display()))?;
    let download_dir = dir.join(format!("download-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&download_dir)
        .with_context(|| format!("create {}", download_dir.display()))?;

    // The documented pipeline: `comfy run --wait | comfy download`.
    let mut run = cli.command();
    run.args(["run", "--workflow", &workflow_path.to_string_lossy(), "--wait"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = run.spawn().context("spawn comfy run")?;
    let stdout = child.stdout.take().context("comfy run stdout")?;
    let mut download = cli.command();
    download
        .args(["download", "--out-dir", &download_dir.to_string_lossy()])
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let downloaded = download.output().context("comfy download")?;
    let run_out = child.wait_with_output().context("wait for comfy run")?;
    let _ = std::fs::remove_file(&workflow_path);
    if !run_out.status.success() {
        let _ = std::fs::remove_dir_all(&download_dir);
        bail!("comfy run failed: {}", String::from_utf8_lossy(&run_out.stderr).trim());
    }
    if !downloaded.status.success() {
        let _ = std::fs::remove_dir_all(&download_dir);
        bail!("comfy download failed: {}", String::from_utf8_lossy(&downloaded.stderr).trim());
    }
    let image = std::fs::read_dir(&download_dir)
        .with_context(|| format!("read {}", download_dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp"))
        })
        .with_context(|| format!("comfy download wrote no image into {}", download_dir.display()))?;
    let bytes = std::fs::read(&image).with_context(|| format!("read {}", image.display()));
    let _ = std::fs::remove_dir_all(&download_dir);
    remove_compare_temp(Path::new(&settings.temp_dir));
    bytes
}

fn remove_compare_temp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_compare_temp(&path);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("comfy.compare"))
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Path to the executable this module will call, for diagnostics/tests.
pub fn cli_path() -> String {
    Cli::resolve().exe
}


/// The workspace root this module would launch, for diagnostics.
pub fn workspace() -> Option<PathBuf> {
    Cli::resolve().workspace
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_size_keeps_aspect_and_respects_the_edge_cap() {
        assert_eq!(plan_size((7008, 4672), 1024), (1024, 680));
        assert_eq!(plan_size_aligned((7008, 4672), 1024, 32), (1024, 672));
        assert_eq!(plan_size_aligned((1600, 1067), 1024, 32), (1024, 672));
        assert_eq!(plan_size((4000, 4000), 1024), (1024, 1024));
        // Never upscales a small source.
        assert_eq!(plan_size((640, 480), 1024), (640, 480));
        // Both edges stay multiples of 8 (VAEEncode's requirement).
        for (w, h) in [(7008, 4672), (3, 1), (1, 3), (5000, 100)] {
            let (ow, oh) = plan_size((w, h), 1024);
            assert_eq!(ow % 8, 0, "{w}x{h} -> {ow}x{oh}");
            assert_eq!(oh % 8, 0, "{w}x{h} -> {ow}x{oh}");
            assert!(ow >= 64 && oh >= 64, "{w}x{h} -> {ow}x{oh}");
        }
    }

    #[test]
    fn quality_and_fidelity_map_to_sampler_and_denoise() {
        assert_eq!(steps_for("low"), 12);
        assert_eq!(steps_for("high"), 28);
        assert_eq!(denoise_for("high"), 0.35);
        assert_eq!(denoise_for("low"), 0.75);
    }
}
