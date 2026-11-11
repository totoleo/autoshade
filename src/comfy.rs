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
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    cli.ensure_server()?;

    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("input-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;

    let uploaded = cli
        .json(&["upload", &input_path.to_string_lossy()])
        .context("comfy upload")?;
    let image = uploaded
        .pointer("/uploads/0/cloud_name")
        .and_then(Value::as_str)
        .or_else(|| uploaded.pointer("/uploads/0/name").and_then(Value::as_str))
        .with_context(|| format!("comfy upload returned no file name: {uploaded}"))?
        .to_string();

    let prefix = format!("autoshade_comfy_{}_{stamp}", std::process::id());
    let wf = workflow(&image, prompt, opts, &prefix);
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    bytes
}

/// A masked fill (object removal / generative fill): the mask PNG's TRANSPARENT
/// pixels are the region to regenerate. ComfyUI's mask convention is the
/// opposite (white = inpaint), so the alpha mask is inverted before
/// `VAEEncodeForInpaint`.
pub fn inpaint(input_png: &[u8], mask_png: &[u8], prompt: &str, quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let input_path = dir.join(format!("fill-in-{pid}-{stamp}.png"));
    let mask_path = dir.join(format!("fill-mask-{pid}-{stamp}.png"));
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
            .json(&["upload", &path.to_string_lossy()])
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
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&mask_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&mask));
    bytes
}

/// LaMa object removal through the ComfyUI inpaint nodes. It is the backend
/// that actually removes large scene objects without inventing a new building:
/// a tight object mask (e.g. OpenCV GrabCut) and a local model fill the hole
/// from the surrounding pixels; the weights path comes from settings.
pub fn lama_inpaint(input_png: &[u8], mask_png: &[u8], _prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let input_path = dir.join(format!("lama-in-{pid}-{stamp}.png"));
    let mask_path = dir.join(format!("lama-mask-{pid}-{stamp}.png"));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let rgba = image::load_from_memory(mask_png)
        .context("decode the LaMa mask")?
        .to_rgba8();
    let (mw, mh) = rgba.dimensions();
    let mut binary = image::GrayImage::new(mw, mh);
    for (x, y, px) in rgba.enumerate_pixels() {
        binary.put_pixel(x, y, image::Luma([if px[3] == 0 { 255 } else { 0 }]));
    }
    let mut mask_png = Vec::new();
    image::DynamicImage::ImageLuma8(binary)
        .write_to(&mut std::io::Cursor::new(&mut mask_png), image::ImageFormat::Png)
        .context("encode the LaMa binary mask")?;
    std::fs::write(&mask_path, &mask_png)
        .with_context(|| format!("write {}", mask_path.display()))?;
    let image = upload_file(&cli, &input_path)?;
    let mask = upload_file(&cli, &mask_path)?;
    if settings.lama_model.trim().is_empty() {
        bail!("LaMa model is not configured — set Settings → ComfyUI → LaMa model");
    }
    let model = settings.lama_model.clone();
    let wf = json!({
        "1": { "class_type": "LoadImage", "inputs": { "image": image } },
        "2": { "class_type": "LoadImageMask", "inputs": { "image": mask, "channel": "red" } },
        "3": { "class_type": "INPAINT_LoadInpaintModel", "inputs": { "model_name": model } },
        "4": { "class_type": "INPAINT_InpaintWithModel", "inputs": {
            "inpaint_model": ["3", 0], "image": ["1", 0], "mask": ["2", 0],
            "seed": (stamp as u64) } },
        "5": { "class_type": "SaveImage",
               "inputs": { "filename_prefix": format!("autoshade_lama_{pid}_{stamp}"),
                           "images": ["4", 0] } }
    });
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&mask_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&mask));
    bytes
}

fn sdxl_inpaint_checkpoint(settings: &Settings) -> Result<String> {
    if settings.sdxl_inpaint_checkpoint.trim().is_empty() {
        bail!("SDXL inpaint checkpoint is not configured — set Settings → ComfyUI → SDXL inpaint checkpoint");
    }
    Ok(settings.sdxl_inpaint_checkpoint.clone())
}

/// SDXL inpainting checkpoint via the proper `InpaintModelConditioning` graph.
/// This is the quality path for object removal: SDXL's 9-channel inpaint UNet
/// understands the mask as a hole, unlike the SD1.5 inpaint checkpoints.
pub fn sdxl_inpaint(input_png: &[u8], mask_png: &[u8], prompt: &str, quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    let input_path = dir.join(format!("sdxl-fill-in-{pid}-{stamp}.png"));
    let mask_path = dir.join(format!("sdxl-fill-mask-{pid}-{stamp}.png"));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let rgba = image::load_from_memory(mask_png)
        .context("decode the SDXL fill mask")?
        .to_rgba8();
    let (mw, mh) = rgba.dimensions();
    let mut binary = image::GrayImage::new(mw, mh);
    for (x, y, px) in rgba.enumerate_pixels() {
        binary.put_pixel(x, y, image::Luma([if px[3] == 0 { 255 } else { 0 }]));
    }
    let mut mask_png = Vec::new();
    image::DynamicImage::ImageLuma8(binary)
        .write_to(&mut std::io::Cursor::new(&mut mask_png), image::ImageFormat::Png)
        .context("encode the SDXL binary fill mask")?;
    std::fs::write(&mask_path, &mask_png)
        .with_context(|| format!("write {}", mask_path.display()))?;
    let image = upload_file(&cli, &input_path)?;
    let mask = upload_file(&cli, &mask_path)?;
    let steps = match quality {
        "high" => 32,
        "medium" => 28,
        _ => 24,
    };
    let ckpt = sdxl_inpaint_checkpoint(settings)?;
    let wf = json!({
        "1": { "class_type": "CheckpointLoaderSimple",
               "inputs": { "ckpt_name": ckpt } },
        "2": { "class_type": "LoadImage", "inputs": { "image": image } },
        "3": { "class_type": "LoadImageMask", "inputs": { "image": mask, "channel": "red" } },
        "4": { "class_type": "CLIPTextEncode", "inputs": { "text": prompt, "clip": ["1", 1] } },
        "5": { "class_type": "CLIPTextEncode",
               "inputs": { "text": "blurry, low quality, watermark, text, duplicate",
                           "clip": ["1", 1] } },
        "6": { "class_type": "InpaintModelConditioning", "inputs": {
            "positive": ["4", 0], "negative": ["5", 0], "vae": ["1", 2],
            "pixels": ["2", 0], "mask": ["3", 0], "noise_mask": true } },
        "7": { "class_type": "KSampler", "inputs": {
            "seed": (stamp as u64), "steps": steps, "cfg": 7.0,
            "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0,
            "model": ["1", 0], "positive": ["6", 0], "negative": ["6", 1],
            "latent_image": ["6", 2] } },
        "8": { "class_type": "VAEDecode",
               "inputs": { "samples": ["7", 0], "vae": ["1", 2] } },
        "9": { "class_type": "SaveImage",
               "inputs": { "filename_prefix": format!("autoshade_sdxl_fill_{pid}_{stamp}"),
                           "images": ["8", 0] } }
    });
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&mask_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&mask));
    bytes
}

/// The inpaint backend for masked fills: `sdxl` (SDXL inpaint checkpoint,
/// default), `sd15`, `qwen`/`qwen-crop` (reference-conditioned Qwen edit on a
/// crop), or any other value for the Z-Image crop path.
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
    cli.ensure_server()?;
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
    let image_name = upload_file(&cli, &input_path)?;
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
    let bytes = run_workflow_json(&cli, settings, &wf);
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
    let feather = (cw.min(ch) / 12).clamp(16, 96);
    blend_crop(&mut out, &generated, &mask, (x0, y0), feather);
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(out)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encode the inpainted frame")?;
    Ok(png)
}

/// Some image, whole-frame: the model setting decides which graph (SD1.5
/// img2img / Qwen edit / Z-Image img2img).
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
const QWEN_SIGMAS: &str =
    "1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0";

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

/// Whole-frame Qwen-Image 2.1 turbo edit, built as an API graph.
///
/// The reference image MUST ride the flat dotted input name
/// `images.image_1` (the nested `images: {image_1: ...}` shape validates but is
/// ignored), and the reference LATENT (`TextEncodeQwenImage21` output 2) starts
/// the sampler. Using `EmptyLatentImage` instead makes this a text-to-image
/// start: the prompt runs, but the source composition is not preserved.
pub fn qwen_edit(input_png: &[u8], prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    if cli.settings.qwen_unet.trim().is_empty()
        || cli.settings.qwen_clip.trim().is_empty()
        || cli.settings.qwen_vae.trim().is_empty()
    {
        bail!("Qwen model files are not configured — set Settings → ComfyUI → Qwen UNET/CLIP/VAE");
    }
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("qwen-in-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let image = upload_file(&cli, &input_path)?;
    let decoded = image::load_from_memory(input_png).context("decode the Qwen edit input")?;
    let (iw, ih) = decoded.dimensions();
    let prefix = format!("autoshade_qwen_{}_{stamp}", std::process::id());
    let wf = json!({
        "1": { "class_type": "UNETLoader",
               "inputs": { "unet_name": settings.qwen_unet, "weight_dtype": "default" } },
        "2": { "class_type": "CLIPLoader",
               "inputs": { "clip_name": settings.qwen_clip, "type": "qwen_image",
                           "device": "default" } },
        "3": { "class_type": "VAELoader", "inputs": { "vae_name": settings.qwen_vae } },
        "13": { "class_type": "LoadImage", "inputs": { "image": image } },
        "4": { "class_type": "TextEncodeQwenImage21", "inputs": {
            "clip": ["2", 0], "prompt": prompt, "negative_prompt": "",
            "resolution": settings.max_edge_value(), "vae": ["3", 0],
            "images.image_1": ["13", 0] } },
        "6": { "class_type": "CFGGuider", "inputs": {
            "model": ["1", 0], "positive": ["4", 0], "negative": ["4", 1],
            "cfg": 1.0 } },
        "7": { "class_type": "RandomNoise", "inputs": { "noise_seed": (stamp as u64) } },
        "8": { "class_type": "KSamplerSelect", "inputs": { "sampler_name": "euler" } },
        "9": { "class_type": "ManualSigmas", "inputs": { "sigmas": QWEN_SIGMAS } },
        "10": { "class_type": "SamplerCustomAdvanced", "inputs": {
            "noise": ["7", 0], "guider": ["6", 0], "sampler": ["8", 0],
            "sigmas": ["9", 0], "latent_image": ["4", 2] } },
        "11": { "class_type": "VAEDecode", "inputs": { "samples": ["10", 0], "vae": ["3", 0] } },
        "12": { "class_type": "SaveImage",
                "inputs": { "images": ["11", 0], "filename_prefix": prefix } }
    });
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    let bytes = bytes?;
    let generated = image::load_from_memory(&bytes)
        .context("decode the Qwen edit result")?
        .to_rgb8();
    let generated = unsharp_rgb(&generated, 0.4);
    let resized = if generated.dimensions() == (iw, ih) {
        generated
    } else {
        image::imageops::resize(
            &generated,
            iw,
            ih,
            image::imageops::FilterType::Lanczos3,
        )
    };
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(resized)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .context("encode the resized Qwen edit result")?;
    Ok(out)
}

/// Crop-based Qwen fill: crop the mask + context, run the reference-conditioned
/// Qwen edit on the crop, paste back through the mask. Qwen's whole-frame edit
/// is not an inpaint model, so locality is what makes a small area tractable.
pub fn qwen_crop_inpaint(input_png: &[u8], mask_png: &[u8], prompt: &str, quality: &str) -> Result<Vec<u8>> {
    let base = image::load_from_memory(input_png)
        .context("decode the Qwen fill base")?
        .to_rgb8();
    let mask = image::load_from_memory(mask_png)
        .context("decode the Qwen fill mask")?
        .to_rgba8();
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
    let pad = ((max_x - min_x).max(max_y - min_y) as f32 * 0.25).max(64.0) as u32;
    let x0 = min_x.saturating_sub(pad);
    let y0 = min_y.saturating_sub(pad);
    let x1 = (max_x + pad + 1).min(bw);
    let y1 = (max_y + pad + 1).min(bh);
    let cw = x1 - x0;
    let ch = y1 - y0;
    let crop = image::imageops::crop_imm(&base, x0, y0, cw, ch).to_image();
    let mut crop_png = Vec::new();
    image::DynamicImage::ImageRgb8(crop.clone())
        .write_to(&mut std::io::Cursor::new(&mut crop_png), image::ImageFormat::Png)
        .context("encode the Qwen fill crop")?;
    let generated = qwen_edit(&crop_png, prompt, quality)?;
    let generated = image::load_from_memory(&generated)
        .context("decode the Qwen fill crop result")?
        .to_rgb8();
    let generated = image::imageops::resize(&generated, cw, ch, image::imageops::FilterType::Lanczos3);
    let mut out = base;
    let feather = (cw.min(ch) / 12).clamp(16, 96);
    blend_crop(&mut out, &generated, &mask, (x0, y0), feather);
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(out)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encode the Qwen fill result")?;
    Ok(png)
}

/// Whole-frame Z-Image turbo img2img (8-step, cfg 1, res_multistep/simple).
pub fn zimage_img2img(input_png: &[u8], prompt: &str, _quality: &str) -> Result<Vec<u8>> {
    let cli = Cli::resolve();
    if cli.settings.zimage_unet.trim().is_empty()
        || cli.settings.zimage_clip.trim().is_empty()
        || cli.settings.zimage_vae.trim().is_empty()
    {
        bail!("Z-Image model files are not configured — set Settings → ComfyUI → Z-Image UNET/CLIP/VAE");
    }
    let settings = &cli.settings;
    cli.ensure_server()?;
    let dir = crate::store::store_root().join("comfy-tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let input_path = dir.join(format!("zimage-in-{}-{stamp}.png", std::process::id()));
    std::fs::write(&input_path, input_png)
        .with_context(|| format!("write {}", input_path.display()))?;
    let image = upload_file(&cli, &input_path)?;
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
    let bytes = run_workflow_json(&cli, settings, &wf);
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(Path::new(&settings.input_dir).join(&image));
    bytes
}

/// Path to the executable this module will call, for diagnostics/tests.
pub fn cli_path() -> String {
    Cli::resolve().exe
}


/// The workspace root this module would launch, for diagnostics.
pub fn workspace() -> Option<PathBuf> {
    Cli::resolve().workspace
}


/// Blend a generated CROP back into the base through the painted mask,
/// feathering the boundary. The mask is the app's RGBA convention: alpha 0 =
/// regenerate. A hard paste is what made the first crop fills show a rectangle.
fn blend_crop(
    base: &mut image::RgbImage,
    generated: &image::RgbImage,
    mask: &image::RgbaImage,
    origin: (u32, u32),
    feather: u32,
) {
    let (cw, ch) = generated.dimensions();
    let mut weights = Vec::with_capacity((cw * ch) as usize);
    for y in 0..ch {
        for x in 0..cw {
            let alpha = mask.get_pixel(origin.0 + x, origin.1 + y)[3];
            weights.push(1.0 - alpha as f32 / 255.0);
        }
    }
    let weights = blur_weights(weights, cw, ch, feather);
    let generated = unsharp_rgb(generated, 0.25);

    // Colour-match the crop's CONTEXT to the base before blending: the model
    // re-renders the whole crop (not just the hole), and its white balance /
    // exposure can differ by a visible step at the seam. Fit a per-channel
    // gain+offset on the pixels OUTSIDE the mask, where both images describe
    // the same scenery, then apply it to the generated pixels.
    let mut b_sum = [0.0f64; 3];
    let mut b_sq = [0.0f64; 3];
    let mut g_sum = [0.0f64; 3];
    let mut g_sq = [0.0f64; 3];
    let mut n = 0u64;
    for y in 0..ch {
        for x in 0..cw {
            if mask.get_pixel(origin.0 + x, origin.1 + y)[3] < 200 {
                continue;
            }
            let b = base.get_pixel(origin.0 + x, origin.1 + y);
            let g = generated.get_pixel(x, y);
            for c in 0..3 {
                b_sum[c] += b[c] as f64;
                b_sq[c] += (b[c] as f64) * (b[c] as f64);
                g_sum[c] += g[c] as f64;
                g_sq[c] += (g[c] as f64) * (g[c] as f64);
            }
            n += 1;
        }
    }
    let mut gain = [1.0f32; 3];
    let mut offset = [0.0f32; 3];
    if n >= 32 {
        for c in 0..3 {
            let bm = b_sum[c] / n as f64;
            let gm = g_sum[c] / n as f64;
            let bv = (b_sq[c] / n as f64 - bm * bm).max(0.0);
            let gv = (g_sq[c] / n as f64 - gm * gm).max(0.0);
            let b_std = bv.sqrt();
            let g_std = gv.sqrt();
            // A flat context (a test swatch, a clear sky) has no reliable
            // statistics; leave it alone rather than dividing by ~zero.
            if b_std < 1.0 || g_std < 1.0 {
                continue;
            }
            gain[c] = (b_std / g_std).clamp(0.6, 1.67) as f32;
            offset[c] = (bm - gain[c] as f64 * gm) as f32;
        }
    }

    for y in 0..ch {
        for x in 0..cw {
            let w = weights[(y * cw + x) as usize];
            let b = base.get_pixel(origin.0 + x, origin.1 + y);
            let g = generated.get_pixel(x, y);
            let mix = |i: usize| {
                let corrected = (g[i] as f32 * gain[i] + offset[i]).clamp(0.0, 255.0);
                (b[i] as f32 * (1.0 - w) + corrected * w).round().clamp(0.0, 255.0) as u8
            };
            base.put_pixel(origin.0 + x, origin.1 + y, image::Rgb([mix(0), mix(1), mix(2)]));
        }
    }
}


/// Mild unsharp mask on the generated crop: the diffusion crop is often a
/// little soft next to the photographic surroundings; 0.55 recovers micro
/// texture without ringing. A 3×3 box blur supplies the low-pass.
fn unsharp_rgb(src: &image::RgbImage, amount: f32) -> image::RgbImage {
    let (w, h) = src.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut lo = [0f32; 3];
            let mut n = 0f32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                    let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                    let p = src.get_pixel(xx, yy);
                    for c in 0..3 {
                        lo[c] += p[c] as f32;
                    }
                    n += 1.0;
                }
            }
            let p = src.get_pixel(x, y);
            let mut q = [0u8; 3];
            for c in 0..3 {
                let v = p[c] as f32 + amount * (p[c] as f32 - lo[c] / n);
                q[c] = v.round().clamp(0.0, 255.0) as u8;
            }
            out.put_pixel(x, y, image::Rgb(q));
        }
    }
    out
}

fn blur_weights(mut w: Vec<f32>, width: u32, height: u32, radius: u32) -> Vec<f32> {
    if radius == 0 {
        return w;
    }
    let r = radius as i32;
    let mut tmp = vec![0.0f32; w.len()];
    for y in 0..height {
        for x in 0..width {
            let x0 = (x as i32 - r).max(0);
            let x1 = (x as i32 + r).min(width as i32 - 1);
            let mut sum = 0.0;
            for xx in x0..=x1 {
                sum += w[(y * width + xx as u32) as usize];
            }
            tmp[(y * width + x) as usize] = sum / (x1 - x0 + 1) as f32;
        }
    }
    w = vec![0.0f32; tmp.len()];
    for y in 0..height {
        for x in 0..width {
            let y0 = (y as i32 - r).max(0);
            let y1 = (y as i32 + r).min(height as i32 - 1);
            let mut sum = 0.0;
            for yy in y0..=y1 {
                sum += tmp[(yy as u32 * width + x) as usize];
            }
            w[(y * width + x) as usize] = sum / (y1 - y0 + 1) as f32;
        }
    }
    w
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
            checkpoint: "fixture-checkpoint".into(),
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
    fn crop_blend_keeps_the_base_outside_the_mask_and_uses_the_generated_pixels_inside() {
        let mut base = image::RgbImage::from_pixel(16, 16, image::Rgb([0, 0, 0]));
        let generated = image::RgbImage::from_pixel(16, 16, image::Rgb([255, 255, 255]));
        let mut mask = image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 255]));
        for y in 4..12 {
            for x in 4..12 {
                mask.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        blend_crop(&mut base, &generated, &mask, (0, 0), 0);
        assert_eq!(base.get_pixel(8, 8), &image::Rgb([255, 255, 255]));
        assert_eq!(base.get_pixel(0, 0), &image::Rgb([0, 0, 0]));
        // A feathered blend leaves a grey ramp at the boundary, never a hard step.
        let mut soft = image::RgbImage::from_pixel(16, 16, image::Rgb([0, 0, 0]));
        blend_crop(&mut soft, &generated, &mask, (0, 0), 3);
        let edge = soft.get_pixel(4, 8)[0];
        assert!(edge > 0 && edge < 255, "feather must blend the edge, got {edge}");
    }
}
