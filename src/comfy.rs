//! Local ComfyUI backend through the official `comfy` CLI.
//!
//! This is the S5.11 replacement for the OpenAI Images/Edits calls of the
//! generative path. autoshade owns the workflow JSON (the CLI's `run
//! --workflow` does not accept `--set`), stages the input image, runs the
//! workflow on the local server and fetches the output URL the run returns.
//!
//! Path management is ours, not ComfyUI's defaults: the server this module
//! launches gets `--base-directory` (models), and `--input-directory /
//! --output-directory / --temp-directory` under the app store, so generated
//! pixels never land in ComfyUI's own folders. The uploaded input and the
//! fetched output file are removed after each run.
//!
//! Settings live in `<store>/comfy.local.json` (created on first save) and can
//! be overridden by env vars; the GUI's Settings panel is the next surface for
//! them. Fields: cli, workspace (dir containing main.py), base_dir (models),
//! input_dir, output_dir, temp_dir, checkpoint, url, max_edge.
//!
//! Gate: `AUTOSHADE_GENERATIVE=comfy` until the real-photo acceptance run is
//! green; the default stays on the old path until S5.11 closes out.

use anyhow::{Context, Result, bail};
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    #[serde(default)]
    pub checkpoint: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub max_edge: Option<u32>,
    /// `sd15` (checkpoint img2img/inpaint), `qwen` (Qwen-Image 2.1 turbo edit
    /// template) or `zimage` (Z-Image turbo). Empty = sd15.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub qwen_unet: String,
    #[serde(default)]
    pub qwen_clip: String,
    #[serde(default)]
    pub qwen_vae: String,
    #[serde(default)]
    pub zimage_unet: String,
    #[serde(default)]
    pub zimage_clip: String,
    #[serde(default)]
    pub zimage_vae: String,
}

impl Settings {
    pub fn path() -> PathBuf {
        crate::store::store_root().join("comfy.local.json")
    }

    pub fn load() -> Self {
        let mut settings: Settings = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if let Some(v) = env("AUTOSHADE_COMFY_CLI") { settings.cli = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_WORKSPACE") { settings.workspace = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_BASE_DIR") { settings.base_dir = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_INPUT_DIR") { settings.input_dir = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_OUTPUT_DIR") { settings.output_dir = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_TEMP_DIR") { settings.temp_dir = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_CHECKPOINT") { settings.checkpoint = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_URL").or_else(|| env("COMFY_LOCAL_URL")) {
            settings.url = v;
        }
        if let Some(v) = env("AUTOSHADE_COMFY_MAX_EDGE").and_then(|v| v.parse().ok()) {
            settings.max_edge = Some(v);
        }
        if let Some(v) = env("AUTOSHADE_COMFY_MODEL") { settings.model = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_QWEN_UNET") { settings.qwen_unet = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_QWEN_CLIP") { settings.qwen_clip = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_QWEN_VAE") { settings.qwen_vae = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_ZIMAGE_UNET") { settings.zimage_unet = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_ZIMAGE_CLIP") { settings.zimage_clip = v; }
        if let Some(v) = env("AUTOSHADE_COMFY_ZIMAGE_VAE") { settings.zimage_vae = v; }
        settings.fill_defaults();
        settings
    }

    /// Persist exactly the values the caller wants (no default filling): the
    /// settings surface owns the file, `load` derives the rest at every use.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("write {}", path.display()))
    }

    fn fill_defaults(&mut self) {
        let store = crate::store::store_root().join("comfy");
        if self.workspace.is_empty() {
            self.workspace = desktop_workspace()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }
        if self.base_dir.is_empty() {
            self.base_dir = desktop_base_dir().unwrap_or_default();
        }
        if self.input_dir.is_empty() {
            self.input_dir = store.join("input").display().to_string();
        }
        if self.output_dir.is_empty() {
            self.output_dir = store.join("output").display().to_string();
        }
        if self.temp_dir.is_empty() {
            self.temp_dir = store.join("temp").display().to_string();
        }
        if self.checkpoint.is_empty() {
            self.checkpoint = "v1-5-pruned-emaonly.safetensors".into();
        }
        if self.max_edge.is_none() {
            self.max_edge = Some(1024);
        }
        if self.model.is_empty() {
            self.model = "sd15".into();
        }
        if self.qwen_unet.is_empty() {
            self.qwen_unet = "qwen_image_2.1_turbo_int8_convrot.safetensors".into();
        }
        if self.qwen_clip.is_empty() {
            self.qwen_clip = "qwen3vl_8b_int8_convrot.safetensors".into();
        }
        if self.qwen_vae.is_empty() {
            self.qwen_vae = "qwen_image_2.1_vae_bf16.safetensors".into();
        }
        if self.zimage_unet.is_empty() {
            self.zimage_unet = "z_image_turbo_bf16.safetensors".into();
        }
        if self.zimage_clip.is_empty() {
            self.zimage_clip = "qwen_3_4b.safetensors".into();
        }
        if self.zimage_vae.is_empty() {
            self.zimage_vae = "ae.safetensors".into();
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

pub fn model() -> String {
    Settings::load().model.to_ascii_lowercase()
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
    let align = match model().as_str() {
        "qwen" => 32,
        "zimage" => 16,
        _ => 8,
    };
    plan_size_aligned(frame, max_edge, align)
}

struct Cli {
    exe: String,
    workspace: Option<PathBuf>,
    url: Option<String>,
    settings: Settings,
}

impl Cli {
    fn resolve() -> Self {
        let settings = Settings::load();
        Self {
            exe: if settings.cli.is_empty() { "comfy".into() } else { settings.cli.clone() },
            workspace: (!settings.workspace.is_empty())
                .then(|| PathBuf::from(&settings.workspace)),
            url: (!settings.url.is_empty()).then(|| settings.url.clone()),
            settings,
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.exe);
        if let Some(workspace) = &self.workspace {
            cmd.arg(format!("--workspace={}", workspace.display()));
        }
        if let Some(url) = &self.url {
            cmd.env("COMFY_LOCAL_URL", url);
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
    fn json_owned(&self, url: Option<&str>, args: &[String]) -> Result<Value> {
        let mut all: Vec<String> = vec!["--json".into()];
        all.extend(args.iter().cloned());
        let mut command = self.command();
        if let Some(url) = url {
            command.env("COMFY_LOCAL_URL", url);
        }
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

    fn json(&self, url: Option<&str>, args: &[&str]) -> Result<Value> {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        self.json_owned(url, &owned)
    }

    /// The URL of a running server, if comfy-cli sees one.
    fn running_url(&self) -> Option<String> {
        let data = self.json(self.url.as_deref(), &["env"]).ok()?;
        if data.pointer("/server/running").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        data.pointer("/server/url")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.url.clone())
            .or_else(|| Some("http://127.0.0.1:8188".into()))
    }

    /// A running server is reused; otherwise one is launched through comfy-cli
    /// (the management half of the CLI) with OUR directories, so models come
    /// from the configured base and generated pixels never land in ComfyUI's
    /// own output folder. Returns the URL subsequent calls target.
    fn ensure_server(&self) -> Result<String> {
        if let Some(url) = self.running_url() {
            return Ok(url);
        }
        if self.workspace.is_none() {
            bail!(
                "no ComfyUI server and no workspace to launch one — set AUTOSHADE_COMFY_WORKSPACE \
                 (dir containing main.py) or start ComfyUI and set COMFY_LOCAL_URL"
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
        for (flag, value) in [
            ("--input-directory", &settings.input_dir),
            ("--output-directory", &settings.output_dir),
            ("--temp-directory", &settings.temp_dir),
        ] {
            if !value.is_empty() {
                args.push(flag.into());
                args.push(value.clone());
            }
        }
        let launched = self
            .json_owned(self.url.as_deref(), &args)
            .context("comfy launch --background")?;
        let url = launched
            .pointer("/url")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "http://127.0.0.1:8188".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while std::time::Instant::now() < deadline {
            if self
                .json(Some(&url), &["env"])
                .map(|data| data.pointer("/server/running").and_then(Value::as_bool) == Some(true))
                .unwrap_or(false)
            {
                return Ok(url);
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        bail!("ComfyUI did not become ready within 180 s after comfy launch --background")
    }
}

/// The install the Comfy Desktop manages, read from its own registry:
/// `installPath` is the root containing `ComfyUI/`; `comfy launch` wants the
/// directory that holds `main.py` (the inner `ComfyUI/`).
fn desktop_workspace() -> Option<PathBuf> {
    let root = desktop_install_root()?;
    let inner = root.join("ComfyUI");
    if inner.join("main.py").is_file() {
        Some(inner)
    } else if root.join("main.py").is_file() {
        Some(root)
    } else {
        None
    }
}

/// The desktop's base directory (where models live), read from ComfyUI's own
/// per-user config rather than hard-coded.
fn desktop_base_dir() -> Option<String> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = PathBuf::from(appdata).join("ComfyUI").join("config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("basePath")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn desktop_install_root() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = PathBuf::from(appdata)
        .join("Comfy Desktop")
        .join("installations.json");
    let text = std::fs::read_to_string(path).ok()?;
    let installs: Value = serde_json::from_str(&text).ok()?;
    installs.as_array()?.iter().find_map(|install| {
        let installed = install.get("status").and_then(Value::as_str) == Some("installed");
        let local = install.get("sourceId").and_then(Value::as_str) == Some("standalone");
        if !(installed && local) {
            return None;
        }
        install.get("installPath").and_then(Value::as_str).map(PathBuf::from)
    })
}

fn workflow(img: &str, prompt: &str, opts: &Img2Img, prefix: &str) -> Value {
    json!({
        "1": { "class_type": "CheckpointLoaderSimple",
               "inputs": { "ckpt_name": opts.checkpoint } },
        "2": { "class_type": "LoadImage", "inputs": { "image": img } },
        "3": { "class_type": "VAEEncode",
               "inputs": { "pixels": ["2", 0], "vae": ["1", 2] } },
        "4": { "class_type": "CLIPTextEncode",
               "inputs": { "text": prompt, "clip": ["1", 1] } },
        "5": { "class_type": "CLIPTextEncode",
               "inputs": { "text": opts.negative, "clip": ["1", 1] } },
        "6": { "class_type": "KSampler",
               "inputs": { "seed": opts.seed, "steps": opts.steps, "cfg": opts.cfg,
                           "sampler_name": "euler", "scheduler": "normal",
                           "denoise": opts.denoise,
                           "model": ["1", 0], "positive": ["4", 0],
                           "negative": ["5", 0], "latent_image": ["3", 0] } },
        "7": { "class_type": "VAEDecode",
               "inputs": { "samples": ["6", 0], "vae": ["1", 2] } },
        "8": { "class_type": "SaveImage",
               "inputs": { "filename_prefix": prefix, "images": ["7", 0] } }
    })
}

/// One img2img round through the local ComfyUI server. Returns the PNG bytes
/// the workflow wrote; the caller validates/canonicalises them. Both staged
/// files (the uploaded input and the server's output file) are removed here.
pub fn img2img(input_png: &[u8], prompt: &str, opts: &Img2Img) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    let url = cli.ensure_server()?;

    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("input-{}-{stamp}.png", std::process::id()));
    let workflow_path = dir.join(format!("workflow-{}-{stamp}.json", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;

    let uploaded = cli
        .json(Some(&url), &["upload", &input_path.to_string_lossy()])
        .context("comfy upload")?;
    let image = uploaded
        .pointer("/uploads/0/cloud_name")
        .and_then(Value::as_str)
        .or_else(|| uploaded.pointer("/uploads/0/name").and_then(Value::as_str))
        .with_context(|| format!("comfy upload returned no file name: {uploaded}"))?
        .to_string();

    let prefix = format!("autoshade_comfy_{}_{stamp}", std::process::id());
    let wf = workflow(&image, prompt, opts, &prefix);
    std::fs::write(&workflow_path, serde_json::to_vec(&wf)?)
        .with_context(|| format!("write {}", workflow_path.display()))?;
    let run = cli
        .json(Some(&url), &["run", "--workflow", &workflow_path.to_string_lossy(), "--wait"])
        .context("comfy run --workflow --wait");
    let _ = std::fs::remove_file(&workflow_path);
    let _ = std::fs::remove_file(&input_path);
    let run = run?;

    if run.get("status").and_then(Value::as_str) != Some("completed") {
        bail!("ComfyUI workflow did not complete: {run}");
    }
    let output_url = run
        .get("outputs")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .with_context(|| format!("ComfyUI workflow completed without an output URL: {run}"))?
        .to_string();
    let bytes = fetch(&output_url)?;

    // "Don't litter": remove the staged input from the server's input dir and
    // the output file from OUR output dir. Both are best-effort — the pixels are
    // already returned by value, a cleanup failure is not an error.
    let input_on_server = Path::new(&settings.input_dir).join(&image);
    let _ = std::fs::remove_file(input_on_server);
    if let Some(path) = local_output_path(&output_url, &settings.output_dir) {
        let _ = std::fs::remove_file(path);
    }
    Ok(bytes)
}

/// A masked fill (object removal / generative fill): the mask PNG's TRANSPARENT
/// pixels are the region to regenerate. ComfyUI's mask convention is the
/// opposite (white = inpaint), so the alpha mask is inverted before
/// `VAEEncodeForInpaint`.
pub fn inpaint(input_png: &[u8], mask_png: &[u8], prompt: &str, quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    let url = cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let input_path = dir.join(format!("fill-in-{pid}-{stamp}.png"));
    let mask_path = dir.join(format!("fill-mask-{pid}-{stamp}.png"));
    let workflow_path = dir.join(format!("fill-wf-{pid}-{stamp}.json"));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    // The app's mask convention is "transparent = regenerate"; ComfyUI's
    // LoadImageMask wants a white-on-black binary mask where white = inpaint.
    // Convert once here so the wire workflow uses the plain `red` channel and
    // needs no InvertMask node.
    let rgba = image::load_from_memory(mask_png)
        .context("decode the fill mask")?
        .to_rgba8();
    let (mw, mh) = rgba.dimensions();
    let mut binary = image::GrayImage::new(mw, mh);
    for (x, y, px) in rgba.enumerate_pixels() {
        let white = if px[3] == 0 { 255 } else { 0 };
        binary.put_pixel(x, y, image::Luma([white]));
    }
    let binary_png = {
        let mut out = Vec::new();
        image::DynamicImage::ImageLuma8(binary)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .context("encode the binary fill mask")?;
        out
    };
    std::fs::write(&mask_path, &binary_png)
        .with_context(|| format!("write {}", mask_path.display()))?;

    let upload = |path: &Path| -> Result<String> {
        let data = cli
            .json(Some(&url), &["upload", &path.to_string_lossy()])
            .context("comfy upload")?;
        data.pointer("/uploads/0/cloud_name")
            .and_then(Value::as_str)
            .or_else(|| data.pointer("/uploads/0/name").and_then(Value::as_str))
            .map(str::to_owned)
            .with_context(|| format!("comfy upload returned no file name: {data}"))
    };
    let image = upload(&input_path)?;
    let mask = upload(&mask_path)?;

    let seed = stamp as u64;
    let prefix = format!("autoshade_fill_{pid}_{stamp}");
    let wf = json!({
        "1": { "class_type": "CheckpointLoaderSimple",
               "inputs": { "ckpt_name": inpaint_checkpoint(settings) } },
        "2": { "class_type": "LoadImage", "inputs": { "image": image } },
        "3": { "class_type": "LoadImageMask",
               "inputs": { "image": mask, "channel": "red" } },
        "4": { "class_type": "VAEEncodeForInpaint",
               "inputs": { "pixels": ["2", 0], "vae": ["1", 2],
                           "mask": ["3", 0], "grow_mask_by": 6 } },
        "6": { "class_type": "CLIPTextEncode",
               "inputs": { "text": prompt, "clip": ["1", 1] } },
        "7": { "class_type": "CLIPTextEncode",
               "inputs": { "text": "blurry, low quality, watermark, text",
                           "clip": ["1", 1] } },
        "8": { "class_type": "KSampler",
               "inputs": { "seed": seed, "steps": steps_for(quality), "cfg": 7.0,
                           "sampler_name": "euler", "scheduler": "normal",
                           "denoise": 1.0, "model": ["1", 0],
                           "positive": ["6", 0], "negative": ["7", 0],
                           "latent_image": ["4", 0] } },
        "9": { "class_type": "VAEDecode",
               "inputs": { "samples": ["8", 0], "vae": ["1", 2] } },
        "10": { "class_type": "SaveImage",
                "inputs": { "filename_prefix": prefix, "images": ["9", 0] } }
    });
    std::fs::write(&workflow_path, serde_json::to_vec(&wf)?)
        .with_context(|| format!("write {}", workflow_path.display()))?;
    let run = cli
        .json(Some(&url), &["run", "--workflow", &workflow_path.to_string_lossy(), "--wait"])
        .context("comfy run --workflow --wait");
    let _ = std::fs::remove_file(&workflow_path);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&mask_path);
    let run = run?;
    if run.get("status").and_then(Value::as_str) != Some("completed") {
        bail!("ComfyUI inpaint did not complete: {run}");
    }
    let output_url = run
        .get("outputs")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .with_context(|| format!("ComfyUI inpaint completed without an output URL: {run}"))?
        .to_string();
    let bytes = fetch(&output_url)?;
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&mask));
    if let Some(path) = local_output_path(&output_url, &settings.output_dir) {
        let _ = std::fs::remove_file(path);
    }
    Ok(bytes)
}

/// The inpaint backend for masked fills. Crop-based Z-Image img2img is the
/// default because the available SD1.5/inpaint checkpoints hallucinate and the
/// Qwen edit template preserves the reference object instead of removing it.
pub fn inpaint_model() -> String {
    std::env::var("AUTOSHADE_COMFY_INPAINT_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "sd15".into())
        .to_ascii_lowercase()
}

/// The checkpoint a masked fill uses: an explicit
/// `AUTOSHADE_COMFY_INPAINT_CHECKPOINT`, else the first installed checkpoint
/// whose name says "inpaint", else the normal checkpoint.
fn inpaint_checkpoint(settings: &Settings) -> String {
    if let Some(v) = env("AUTOSHADE_COMFY_INPAINT_CHECKPOINT") {
        return v;
    }
    let dir = Path::new(&settings.base_dir)
        .join("models")
        .join("checkpoints");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| n.to_ascii_lowercase().contains("inpaint"))
            .collect();
        names.sort();
        if let Some(name) = names.into_iter().next() {
            return name;
        }
    }
    settings.checkpoint_name()
}

/// Crop-based inpaint: the mask's bounding box plus context is redrawn by
/// Z-Image turbo, then pasted back only through the mask. This keeps the model
/// on a small frame (fast, and the prompt has locality), unlike a whole-frame
/// edit which preserves the object the user asked to remove.
pub fn crop_inpaint(input_png: &[u8], mask_png: &[u8], prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let base = image::load_from_memory(input_png)
        .context("decode the inpaint base")?
        .to_rgb8();
    let mask = image::load_from_memory(mask_png)
        .context("decode the inpaint mask")?
        .to_rgba8();
    if base.dimensions() != mask.dimensions() {
        bail!(
            "inpaint base {}x{} and mask {}x{} differ",
            base.width(), base.height(), mask.width(), mask.height()
        );
    }
    let (bw, bh) = base.dimensions();
    let mut min_x = bw;
    let mut min_y = bh;
    let mut max_x = 0;
    let mut max_y = 0;
    for (x, y, px) in mask.enumerate_pixels() {
        if px[3] < 128 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x > max_x || min_y > max_y {
        bail!("the fill mask has no transparent (to-regenerate) pixels");
    }
    let pad = ((max_x - min_x).max(max_y - min_y) as f32 * 0.30).max(64.0) as u32;
    let x0 = min_x.saturating_sub(pad);
    let y0 = min_y.saturating_sub(pad);
    let x1 = (max_x + pad + 1).min(bw);
    let y1 = (max_y + pad + 1).min(bh);
    let cw = x1 - x0;
    let ch = y1 - y0;
    let crop = image::imageops::crop_imm(&base, x0, y0, cw, ch).to_image();
    let scale = 1024.0 / cw.max(ch) as f32;
    let rw = (((cw as f32 * scale) as u32).max(64) / 16 * 16).max(64);
    let rh = (((ch as f32 * scale) as u32).max(64) / 16 * 16).max(64);
    let crop_resized = image::imageops::resize(&crop, rw, rh, image::imageops::FilterType::Lanczos3);

    let cli = Cli::resolve();
    let settings = &cli.settings;
    let url = cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("inpaint-crop-{}-{stamp}.png", std::process::id()));
    let crop_png = {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(crop_resized)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .context("encode the inpaint crop")?;
        out
    };
    std::fs::write(&input_path, &crop_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let image_name = upload_file(&cli, &url, &input_path)?;
    // A removal prompt (blank user text) is an instruction, but Z-Image img2img
    // wants a description of the wanted content.
    let lower = prompt.to_ascii_lowercase();
    let wire_prompt = if lower.contains("remove") || lower.contains("erase") {
        "calm natural background continuing the surroundings seamlessly, photorealistic, no objects".to_string()
    } else {
        prompt.to_string()
    };
    let seed = stamp as u64;
    let prefix = format!("autoshade_inpaint_{}_{stamp}", std::process::id());
    let wf = json!({
        "1": { "class_type": "UNETLoader",
               "inputs": { "unet_name": settings.zimage_unet, "weight_dtype": "default" } },
        "2": { "class_type": "CLIPLoader",
               "inputs": { "clip_name": settings.zimage_clip, "type": "lumina2",
                           "device": "default" } },
        "3": { "class_type": "VAELoader", "inputs": { "vae_name": settings.zimage_vae } },
        "4": { "class_type": "LoadImage", "inputs": { "image": image_name } },
        "5": { "class_type": "VAEEncode", "inputs": { "pixels": ["4", 0], "vae": ["3", 0] } },
        "6": { "class_type": "CLIPTextEncode",
               "inputs": { "clip": ["2", 0], "text": wire_prompt } },
        "7": { "class_type": "ConditioningZeroOut", "inputs": { "conditioning": ["6", 0] } },
        "8": { "class_type": "ModelSamplingAuraFlow",
               "inputs": { "model": ["1", 0], "shift": 3.0 } },
        "9": { "class_type": "KSampler",
               "inputs": { "model": ["8", 0], "positive": ["6", 0],
                           "negative": ["7", 0], "latent_image": ["5", 0],
                           "seed": seed, "steps": 8, "cfg": 1.0,
                           "sampler_name": "res_multistep", "scheduler": "simple",
                           "denoise": 0.9 } },
        "10": { "class_type": "VAEDecode",
                "inputs": { "samples": ["9", 0], "vae": ["3", 0] } },
        "11": { "class_type": "SaveImage",
                "inputs": { "images": ["10", 0], "filename_prefix": prefix } }
    });
    let bytes = run_workflow_json(&cli, &url, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image_name));
    let generated = image::load_from_memory(&bytes?)
        .context("decode the inpaint crop result")?
        .to_rgb8();
    let generated = image::imageops::resize(
        &generated,
        cw,
        ch,
        image::imageops::FilterType::Lanczos3,
    );
    let mut out = base;
    for (x, y, px) in generated.enumerate_pixels() {
        if mask.get_pixel(x0 + x, y0 + y)[3] < 128 {
            out.put_pixel(x0 + x, y0 + y, *px);
        }
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(out)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encode the inpainted frame")?;
    Ok(png)
}

/// Some image, whole-frame: the model setting decides which graph (SD1.5
/// img2img / Qwen edit / Z-Image img2img).
pub fn whole_frame(input_png: &[u8], prompt: &str, opts: &Img2Img, quality: &str) -> Result<Vec<u8>> {
    match model().as_str() {
        "qwen" => qwen_edit(input_png, prompt, quality),
        "zimage" => zimage_img2img(input_png, prompt, quality),
        _ => img2img(input_png, prompt, opts),
    }
}

/// The `images/edits` replacement for the shared fill/adjust path. The model
/// setting picks the backend: `sd15` inpaints (mask) or img2img (no mask),
/// `qwen` runs the Qwen-Image 2.1 turbo edit template on the whole frame (the
/// caller composites the masked region), `zimage` runs Z-Image img2img.
pub fn edit_png(input_png: &[u8], mask_png: Option<&[u8]>, prompt: &str, quality: &str) -> Result<Vec<u8>> {
    // A masked fill uses the inpaint backend; the whole-frame model setting is
    // for reimagine/adjust (no mask) and for the fill's fallback.
    if let Some(mask) = mask_png {
        return match inpaint_model().as_str() {
            "sd15" => inpaint(input_png, mask, prompt, quality),
            _ => crop_inpaint(input_png, mask, prompt, quality),
        };
    }
    match model().as_str() {
        "qwen" => qwen_edit(input_png, prompt, quality),
        "zimage" => zimage_img2img(input_png, prompt, quality),
        _ => {
            let opts = Img2Img {
                checkpoint: checkpoint(),
                negative: "blurry, low quality, watermark, text, extra limbs".into(),
                steps: steps_for(quality),
                cfg: 6.0,
                denoise: 0.55,
                seed: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(42),
            };
            img2img(input_png, prompt, &opts)
        }
    }
}

/// The official Qwen-Image 2.1 turbo edit graph, shipped as a UI-format
/// template because its subgraph handles the Qwen-specific sampler/guider
/// wiring; comfy-cli converts it client-side. Only the input image, prompt,
/// output size and model names are patched.
const QWEN_EDIT_TEMPLATE: &str =
    include_str!("../assets/comfy/qwen_image_2_1_turbo_image_edit.json");

fn upload_file(cli: &Cli, url: &str, path: &Path) -> Result<String> {
    let data = cli
        .json(Some(url), &["upload", &path.to_string_lossy()])
        .context("comfy upload")?;
    data.pointer("/uploads/0/cloud_name")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/uploads/0/name").and_then(Value::as_str))
        .map(str::to_owned)
        .with_context(|| format!("comfy upload returned no file name: {data}"))
}

fn run_workflow_json(cli: &Cli, url: &str, settings: &Settings, wf: &Value) -> Result<Vec<u8>> {
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let workflow_path = dir.join(format!("wf-{}-{stamp}.json", std::process::id()));
    std::fs::write(&workflow_path, serde_json::to_vec(wf)?)
        .with_context(|| format!("write {}", workflow_path.display()))?;
    let run = cli
        .json(Some(url), &["run", "--workflow", &workflow_path.to_string_lossy(), "--wait"])
        .context("comfy run --workflow --wait");
    let _ = std::fs::remove_file(&workflow_path);
    let run = run?;
    if run.get("status").and_then(Value::as_str) != Some("completed") {
        bail!("ComfyUI workflow did not complete: {run}");
    }
    let output_url = run
        .get("outputs")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .with_context(|| format!("ComfyUI workflow completed without an output URL: {run}"))?
        .to_string();
    let bytes = fetch(&output_url)?;
    if let Some(path) = local_output_path(&output_url, &settings.output_dir) {
        let _ = std::fs::remove_file(path);
    }
    // Nodes such as ImageCompare write their scratch previews under our temp
    // dir; they are ours, so take them with us.
    remove_compare_temp(Path::new(&settings.temp_dir));
    Ok(bytes)
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

/// Whole-frame Qwen-Image 2.1 turbo edit (reference-guided). The caller decides
/// which pixels of the answer to keep (a painted mask composites only that
/// region; no mask keeps the whole frame).
pub fn qwen_edit(input_png: &[u8], prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    let url = cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("qwen-in-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let image = upload_file(&cli, &url, &input_path)?;
    let (iw, ih) = image::load_from_memory(input_png)
        .context("decode the Qwen edit input")?
        .dimensions();
    let (w, h) = plan_size_aligned((iw, ih), settings.max_edge_value(), 32);
    let mut wf: Value = serde_json::from_str(QWEN_EDIT_TEMPLATE)
        .context("parse the Qwen edit template")?;
    let prefix = format!("autoshade_qwen_{}_{stamp}", std::process::id());
    let nodes = wf
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .context("the Qwen edit template carries no nodes")?;
    for node in nodes {
        let Some(kind) = node.get("type").and_then(Value::as_str) else {
            continue;
        };
        match kind {
            "LoadImage" => {
                if let Some(widgets) = node.get_mut("widgets_values").and_then(Value::as_array_mut)
                    && !widgets.is_empty()
                {
                    widgets[0] = json!(image);
                }
            }
            "SaveImageAdvanced" => {
                if let Some(widgets) = node.get_mut("widgets_values").and_then(Value::as_array_mut)
                    && !widgets.is_empty()
                {
                    widgets[0] = json!(prefix);
                }
            }
            // A subgraph instance's id is a UUID; its promoted widgets are the
            // fixed order the template ships (prompt, size, three model names,
            // the prompt-enhancer clip last).
            kind if kind.contains('-') => {
                if let Some(widgets) = node.get_mut("widgets_values").and_then(Value::as_array_mut)
                    && widgets.len() > 17
                {
                        widgets[1] = json!(prompt);
                        widgets[8] = json!(w);
                        widgets[9] = json!(h);
                        widgets[12] = json!(settings.qwen_unet);
                        widgets[13] = json!(settings.qwen_clip);
                        widgets[14] = json!(settings.qwen_vae);
                        // The template's i2i enhancer clip is not installed on
                        // this machine; the enhancer is off by default, but the
                        // graph still has to validate. The t2i enhancer is the
                        // installed stand-in.
                        if widgets[17]
                            .as_str()
                            .is_some_and(|s| s.contains("_pe_i2i"))
                        {
                            widgets[17] = json!(
                                "qwen3.5_9b_qwen_image_2.1_pe_t2i.int8_convrot.safetensors"
                            );
                        }
                    }
                }
            _ => {}
        }
    }
    let bytes = run_workflow_json(&cli, &url, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    bytes
}

/// Whole-frame Z-Image turbo img2img (8-step, cfg 1, res_multistep/simple).
pub fn zimage_img2img(input_png: &[u8], prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    let url = cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("zimage-in-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let image = upload_file(&cli, &url, &input_path)?;
    let prefix = format!("autoshade_zimage_{}_{stamp}", std::process::id());
    let wf = json!({
        "1": { "class_type": "UNETLoader",
               "inputs": { "unet_name": settings.zimage_unet, "weight_dtype": "default" } },
        "2": { "class_type": "CLIPLoader",
               "inputs": { "clip_name": settings.zimage_clip, "type": "lumina2",
                           "device": "default" } },
        "3": { "class_type": "VAELoader", "inputs": { "vae_name": settings.zimage_vae } },
        "4": { "class_type": "LoadImage", "inputs": { "image": image } },
        "5": { "class_type": "VAEEncode",
               "inputs": { "pixels": ["4", 0], "vae": ["3", 0] } },
        "6": { "class_type": "CLIPTextEncode",
               "inputs": { "clip": ["2", 0], "text": prompt } },
        "7": { "class_type": "ConditioningZeroOut",
               "inputs": { "conditioning": ["6", 0] } },
        "8": { "class_type": "ModelSamplingAuraFlow",
               "inputs": { "model": ["1", 0], "shift": 3.0 } },
        "9": { "class_type": "KSampler",
               "inputs": { "model": ["8", 0], "positive": ["6", 0],
                           "negative": ["7", 0], "latent_image": ["5", 0],
                           "seed": (stamp as u64), "steps": 8, "cfg": 1.0,
                           "sampler_name": "res_multistep", "scheduler": "simple",
                           "denoise": 0.55 } },
        "10": { "class_type": "VAEDecode",
                "inputs": { "samples": ["9", 0], "vae": ["3", 0] } },
        "11": { "class_type": "SaveImage",
                "inputs": { "images": ["10", 0], "filename_prefix": prefix } }
    });
    let bytes = run_workflow_json(&cli, &url, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    bytes
}

/// Resolve a `/view?filename=..&subfolder=..&type=output` URL back to the local
/// file under our managed output dir (so it can be removed after fetching).
fn local_output_path(url: &str, output_dir: &str) -> Option<PathBuf> {
    let query = url.split_once('?')?.1;
    let mut filename = None;
    let mut subfolder = String::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        let value = percent_decode(value);
        match key {
            "filename" => filename = Some(value),
            "subfolder" => subfolder = value,
            _ => {}
        }
    }
    let filename = filename?;
    Some(Path::new(output_dir).join(subfolder).join(filename))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn fetch(url: &str) -> Result<Vec<u8>> {
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes).with_context(|| format!("read {url}"))?;
    Ok(bytes)
}

/// Path to the executable this module will call, for diagnostics/tests.
pub fn cli_path() -> String {
    Cli::resolve().exe
}

/// The current server URL comfy-cli/this module will target, if configured.
pub fn server_url() -> Option<String> {
    Cli::resolve().url
}

/// True when a ComfyUI server is reachable through the configured CLI.
pub fn server_available() -> bool {
    Cli::resolve().running_url().is_some()
}

/// The workspace root this module would launch, for diagnostics.
pub fn workspace() -> Option<PathBuf> {
    Cli::resolve().workspace
}

/// Read-only diagnostics for the UI/logs: `(cli, workspace, url, running)`.
pub fn diagnostics() -> (String, Option<PathBuf>, Option<String>, bool) {
    let cli = Cli::resolve();
    let running = cli.running_url();
    let is_running = running.is_some();
    let url = running.or(cli.url);
    (cli.exe, cli.workspace, url, is_running)
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

    #[test]
    fn the_workflow_is_core_nodes_only_and_wired_in_order() {
        let opts = Img2Img {
            checkpoint: "v1-5-pruned-emaonly.safetensors".into(),
            negative: "blurry".into(),
            steps: 20,
            cfg: 6.0,
            denoise: 0.55,
            seed: 42,
        };
        let wf = workflow("input.png", "warm dusk", &opts, "prefix");
        for node in ["CheckpointLoaderSimple", "LoadImage", "VAEEncode", "CLIPTextEncode",
                     "KSampler", "VAEDecode", "SaveImage"] {
            assert!(
                wf.as_object().unwrap().values().any(|n| n["class_type"] == node),
                "missing {node}"
            );
        }
        assert_eq!(wf["6"]["inputs"]["latent_image"], json!(["3", 0]));
        assert_eq!(wf["6"]["inputs"]["positive"], json!(["4", 0]));
        assert_eq!(wf["2"]["inputs"]["image"], json!("input.png"));
        assert_eq!(wf["8"]["inputs"]["filename_prefix"], json!("prefix"));
    }

    #[test]
    fn a_view_url_resolves_to_the_managed_output_file() {
        let url = "http://127.0.0.1:8188/view?filename=autoshade_comfy_1_2_00001_.png&subfolder=&type=output";
        let path = local_output_path(url, r"D:\store\comfy\output").unwrap();
        assert_eq!(path, Path::new(r"D:\store\comfy\output\autoshade_comfy_1_2_00001_.png"));
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
    }
}
