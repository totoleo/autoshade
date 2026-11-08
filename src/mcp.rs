//! The MCP server: a stdio JSON-RPC surface over the deterministic engine.
//!
//! The harness owns the conversation and every model call; this process owns
//! decode, validate, render, mask resolution and commit. The contract is
//! `wishes/harness-refactor/MCP.md`; S1 implements the tools listed in its §8.
//!
//! **stdout is the MCP channel.** Every diagnostic goes to stderr (or a file);
//! a stray `println!` here would corrupt the protocol.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use crate::advisor::{
    catalogue, decision_key, expand_ai_mask_intents, parse_judgement, repair_hsl_axis_lengths,
    take_opinions,
};
use crate::recipe::EditRecipe;

/// One JSON-RPC server bound to a photo root and an artifact directory.
pub struct Server {
    roots: Vec<PathBuf>,
    artifacts: PathBuf,
    /// Recipe hashes this session has validated. `recipe.commit` refuses a hash
    /// that is not here, so an unvalidated proposal cannot reach the store.
    validated: HashSet<String>,
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }
}

impl Server {
    pub fn new(roots: Vec<PathBuf>, data_dir: PathBuf) -> Result<Self> {
        if roots.is_empty() {
            anyhow::bail!("mcp needs at least one --root");
        }
        let mut canon = Vec::new();
        for r in roots {
            canon.push(
                r.canonicalize()
                    .with_context(|| format!("--root {} does not exist", r.display()))?,
            );
        }
        let artifacts = data_dir.join("artifacts").join("mcp");
        std::fs::create_dir_all(&artifacts)
            .with_context(|| format!("create {}", artifacts.display()))?;
        Ok(Self { roots: canon, artifacts, validated: HashSet::new() })
    }

    /// Read one request per line from stdin until EOF.
    pub fn run(mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut out = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            let line = line.context("read stdin")?;
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    reply_error(&mut out, Value::Null, -32700, &format!("parse error: {e}"))?;
                    continue;
                }
            };
            let Some(id) = msg.get("id").cloned() else {
                // A notification: initialize/initialized/cancelled. Nothing to answer.
                continue;
            };
            let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let result = match method {
                "initialize" => Ok(init_result(&params)),
                "ping" => Ok(json!({})),
                "tools/list" => Ok(json!({ "tools": tool_definitions() })),
                "tools/call" => self.call_tool(&params),
                other => Err(RpcError::new(-32601, format!("method not found: {other}"))),
            };
            match result {
                Ok(v) => reply_result(&mut out, id, v)?,
                Err(e) => reply_error(&mut out, id, e.code, &e.message)?,
            }
        }
        Ok(())
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid("tools/call needs `name`"))?;
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        Ok(match name {
            "frame.read" => self.frame_read(&args),
            "recipe.schema" => recipe_schema(),
            "recipe.validate" => self.recipe_validate(&args),
            "recipe.render" => self.recipe_render(&args),
            "recipe.commit" => self.recipe_commit(&args),
            "masks.cut" => self.masks_cut(&args),
            "judge.rubric" => judge_rubric(&args),
            "judge.parse" => judge_parse(&args),
            other => tool_failure("unknown_tool", format!("no such tool: {other}")),
        })
    }

    fn frame_read(&self, args: &Value) -> Value {
        let Some(photo) = args.get("photo").and_then(Value::as_str) else {
            return tool_failure("invalid_args", "frame.read needs `photo`");
        };
        let path = match self.resolve(photo) {
            Ok(p) => p,
            Err(e) => return tool_failure("bad_path", e),
        };
        let decoded = match crate::decode::decode_any(&path) {
            Ok(d) => d,
            Err(e) => return tool_failure("decode_failed", format!("{e:#}")),
        };
        let img = decoded.preview_resized(1536);
        let mut jpeg = Vec::new();
        if let Err(e) =
            img.write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        {
            return tool_failure("decode_failed", format!("encode preview JPEG: {e}"));
        }
        let key = self.key_of(&path);
        let meta = &decoded.meta;
        let text = json!({
            "key": key,
            "photo": path.display().to_string(),
            "width": meta.width,
            "height": meta.height,
            "meta": meta,
            "histogram": decoded.histogram,
            "as_shot_wb": meta.as_shot_wb_coeffs,
        });
        json!({
            "content": [
                { "type": "image", "data": b64(&jpeg), "mimeType": "image/jpeg" },
                { "type": "text", "text": text.to_string() },
            ],
            "isError": false,
        })
    }

    fn recipe_validate(&mut self, args: &Value) -> Value {
        let Some(recipe) = args.get("recipe") else {
            return tool_failure("invalid_args", "recipe.validate needs `recipe`");
        };
        match parse_recipe(recipe.clone()) {
            Ok((recipe, warnings)) => {
                let hash = hash_recipe(&recipe);
                self.validated.insert(hash.clone());
                tool_ok(json!({
                    "ok": true, "hash": hash, "errors": [], "warnings": warnings,
                    // The CANONICAL recipe (clamps, HSL repair, AI-mask
                    // expansion already applied). A parent that owns
                    // persistence (the GUI's deferred commit) needs this, not
                    // the raw candidate: the raw shape carries nulls the
                    // EditRecipe parser rejects.
                    "recipe": recipe,
                }))
            }
            Err(errors) => tool_ok(json!({
                "ok": false, "hash": null, "errors": errors, "warnings": []
            })),
        }
    }

    fn recipe_render(&self, args: &Value) -> Value {
        let Some(photo) = args.get("photo").and_then(Value::as_str) else {
            return tool_failure("invalid_args", "recipe.render needs `photo`");
        };
        let Some(recipe) = args.get("recipe") else {
            return tool_failure("invalid_args", "recipe.render needs `recipe`");
        };
        let path = match self.resolve(photo) {
            Ok(p) => p,
            Err(e) => return tool_failure("bad_path", e),
        };
        let (recipe, _) = match parse_recipe(recipe.clone()) {
            Ok(v) => v,
            Err(errors) => {
                return tool_failure(
                    "invalid_recipe",
                    json!({ "errors": errors }).to_string(),
                );
            }
        };
        let edge = args
            .get("long_edge")
            .and_then(Value::as_u64)
            .unwrap_or(1600)
            .clamp(256, 8192) as u32;
        let hash = hash_recipe(&recipe);
        let key = format!("{}-{edge}-{}", &hash[..16], env!("CARGO_PKG_VERSION"));
        let out = self.artifacts.join(format!("{key}.jpg"));
        if !out.exists() {
            let opts = crate::render::ExportOpts {
                long_edge: Some(edge),
                ..Default::default()
            };
            if let Err(e) = crate::render::render_to_file(
                &path,
                &recipe,
                &out,
                None,
                Some(&opts),
                crate::diag::stderr(),
            ) {
                return tool_failure("render_failed", format!("{e:#}"));
            }
        }
        let bytes = match std::fs::read(&out) {
            Ok(b) => b,
            Err(e) => return tool_failure("render_failed", format!("read artifact: {e}")),
        };
        let (w, h) = image::image_dimensions(&out).unwrap_or((0, 0));
        let text = json!({
            "artifact": key,
            "path": out.display().to_string(),
            "sha256": crate::sha256::sha256_hex(&bytes),
            "width": w,
            "height": h,
        });
        json!({
            "content": [
                { "type": "image", "data": b64(&bytes), "mimeType": "image/jpeg" },
                { "type": "text", "text": text.to_string() },
            ],
            "isError": false,
        })
    }

    fn recipe_commit(&self, args: &Value) -> Value {
        let Some(photo) = args.get("photo").and_then(Value::as_str) else {
            return tool_failure("invalid_args", "recipe.commit needs `photo`");
        };
        let Some(recipe) = args.get("recipe") else {
            return tool_failure("invalid_args", "recipe.commit needs `recipe`");
        };
        let Some(hash) = args.get("hash").and_then(Value::as_str) else {
            return tool_failure("invalid_args", "recipe.commit needs `hash`");
        };
        let path = match self.resolve(photo) {
            Ok(p) => p,
            Err(e) => return tool_failure("bad_path", e),
        };
        let (recipe, _) = match parse_recipe(recipe.clone()) {
            Ok(v) => v,
            Err(errors) => {
                return tool_failure("invalid_recipe", json!({ "errors": errors }).to_string());
            }
        };
        let actual = hash_recipe(&recipe);
        if actual != hash || !self.validated.contains(&actual) {
            return tool_failure(
                "commit_unvalidated",
                json!({
                    "expected": hash,
                    "actual": actual,
                    "why": "run recipe.validate first and commit the hash it returned"
                })
                .to_string(),
            );
        }
        let outcome =
            match crate::pipeline::commit_analyzed_develop(&path, &recipe, crate::diag::stderr())
            {
                Ok(o) => o,
                Err(e) => return tool_failure("commit_failed", format!("{e:#}")),
            };
        let develop = crate::store::develop_dir(&path);
        match outcome {
            crate::pipeline::ProjectionOutcome::Written { path: xmp, note, losses } => tool_ok(
                json!({
                    "develop": develop.display().to_string(),
                    "xmp": xmp.display().to_string(),
                    "note": note,
                    "losses": losses.len(),
                }),
            ),
            crate::pipeline::ProjectionOutcome::Cleared => tool_ok(json!({
                "develop": develop.display().to_string(),
                "xmp": Value::Null,
                "note": "no sidecar reproduces this develop",
            })),
            crate::pipeline::ProjectionOutcome::Failed(e) => {
                tool_failure("xmp_failed", format!("{e:#}"))
            }
        }
    }

    /// Cut the model's `ai` mask intents locally. The intents arrive in the
    /// schema's own spelling (`what` / `ref_x` / `ref_y`); the expander turns
    /// them into the engine's `ai_mask` geometry, the segmenter fills the
    /// rasters, and the tool returns where each one landed.
    fn masks_cut(&self, args: &Value) -> Value {
        let Some(photo) = args.get("photo").and_then(Value::as_str) else {
            return tool_failure("invalid_args", "masks.cut needs `photo`");
        };
        let Some(masks) = args.get("masks").and_then(Value::as_array) else {
            return tool_failure("invalid_args", "masks.cut needs `masks`");
        };
        if masks.is_empty() {
            return tool_failure("invalid_args", "masks.cut needs at least one mask");
        }
        let path = match self.resolve(photo) {
            Ok(p) => p,
            Err(e) => return tool_failure("bad_path", e),
        };
        let entries: Vec<Value> = masks
            .iter()
            .map(|m| {
                let what = m.get("what").and_then(Value::as_str).unwrap_or("");
                let ref_x = m.get("ref_x").and_then(Value::as_f64).unwrap_or(0.5);
                let ref_y = m.get("ref_y").and_then(Value::as_f64).unwrap_or(0.5);
                json!({ "mask": { "kind": "ai", "what": what, "ref_x": ref_x, "ref_y": ref_y } })
            })
            .collect();
        let mut value = json!({ "masks": entries });
        expand_ai_mask_intents(&mut value);
        let mut recipe: EditRecipe = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => return tool_failure("invalid_args", format!("mask intent: {e}")),
        };
        let cfg = crate::config::Config::load();
        let resolution = crate::segment::resolve_ai_masks(&cfg, &path, &path, &mut recipe);
        let value = serde_json::to_value(&recipe).unwrap_or(Value::Null);
        let mut cuts = Vec::new();
        for (index, mask) in value
            .get("masks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            cuts.push(json!({
                "index": index,
                "raster": mask.pointer("/mask/raster").and_then(Value::as_str),
            }));
        }
        tool_ok(json!({
            "masks": cuts,
            "resolved": resolution.resolved,
            "cached": resolution.cached,
            "renewed": resolution.renewed,
            "unresolved": resolution.unresolved,
        }))
    }

    fn resolve(&self, photo: &str) -> Result<PathBuf, String> {        let p = Path::new(photo);
        let mut candidates = Vec::new();
        if p.is_absolute() {
            candidates.push(p.to_path_buf());
        } else {
            for root in &self.roots {
                candidates.push(root.join(p));
            }
        }
        for candidate in candidates {
            if let Ok(canon) = candidate.canonicalize()
                && canon.is_file()
                && self.roots.iter().any(|r| canon.starts_with(r))
            {
                return Ok(canon);
            }
        }
        Err(format!(
            "{photo} is not a file under a configured --root ({})",
            self.roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    fn key_of(&self, path: &Path) -> String {
        let root = self
            .roots
            .iter()
            .find(|r| path.starts_with(r))
            .unwrap_or(&self.roots[0]);
        let rel = path.strip_prefix(root).unwrap_or(path);
        rel.with_extension("")
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn init_result(params: &Value) -> Value {
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "autoshade", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "frame.read",
            "description": "Read a photo's preview (JPEG), metadata, histogram and as-shot WB.",
            "inputSchema": {
                "type": "object",
                "properties": { "photo": { "type": "string", "description": "path under a configured --root" } },
                "required": ["photo"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "recipe.schema",
            "description": "The EditRecipe JSON Schema the harness must use for its submit tool.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "recipe.validate",
            "description": "Validate and normalize an EditRecipe. Returns ok, a canonical hash, and structured errors.",
            "inputSchema": {
                "type": "object",
                "properties": { "recipe": { "type": "object" } },
                "required": ["recipe"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "recipe.render",
            "description": "Render an EditRecipe to a JPEG artifact and return the image plus its hash.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "photo": { "type": "string" },
                    "recipe": { "type": "object" },
                    "long_edge": { "type": "integer", "minimum": 256, "maximum": 8192 }
                },
                "required": ["photo", "recipe"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "recipe.commit",
            "description": "Write a validated EditRecipe to the develop store and its XMP sidecar.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "photo": { "type": "string" },
                    "recipe": { "type": "object" },
                    "hash": { "type": "string", "description": "the hash from the last recipe.validate" },
                    "session": { "type": "string" },
                    "verdict": { "type": "string", "enum": ["accept", "revise", "reject"] }
                },
                "required": ["photo", "recipe", "hash", "session"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "masks.cut",
            "description": "Resolve the model's AI mask intents (sky/subject/object/face/eyes/background) to local rasters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "photo": { "type": "string" },
                    "masks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "what": { "enum": ["sky", "subject", "object", "face", "eyes", "background"] },
                                "ref_x": { "type": "number" },
                                "ref_y": { "type": "number" }
                            },
                            "required": ["what"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["photo", "masks"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "judge.rubric",
            "description": "The critic's brief (text) and reply schema. The harness runs the critic itself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "strength": { "type": "number", "minimum": 0, "maximum": 1 },
                    "direction": { "type": "string" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "judge.parse",
            "description": "Parse a critic's text reply into {decision, score, critique, hint}.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }
        }),
    ]
}

fn recipe_schema() -> Value {
    tool_ok(json!({
        "name": "edit_recipe",
        "version": "1",
        "schema": catalogue::edit_recipe_schema(),
    }))
}

fn judge_rubric(args: &Value) -> Value {
    let strength = args
        .get("strength")
        .and_then(Value::as_f64)
        .map(|v| v.clamp(0.0, 1.0) as f32);
    let direction = args.get("direction").and_then(Value::as_str);
    let intent = crate::advisor::GradeIntent {
        strength: crate::recipe::GradeStrength::from_optional(strength),
        direction,
        ..Default::default()
    };
    tool_ok(crate::advisor::rubric_for(Some(intent)))
}

fn judge_parse(args: &Value) -> Value {
    let Some(text) = args.get("text").and_then(Value::as_str) else {
        return tool_failure("invalid_args", "judge.parse needs `text`");
    };
    match parse_judgement(text, &[]) {
        Ok(j) => tool_ok(json!({
            "ok": true,
            "decision": decision_key(&j.decision),
            "score": j.score,
            "critique": j.critique,
            "hint": j.hint,
        })),
        Err(e) => tool_ok(json!({ "ok": false, "reason": e.to_string() })),
    }
}

/// Normalize then parse, exactly the way the text path does: nullable opinions,
/// HSL axis repair, AI-mask intents, then `EditRecipe`'s serde.
fn parse_recipe(mut value: Value) -> Result<(EditRecipe, Vec<String>), Vec<Value>> {
    let mut warnings = Vec::new();
    let _ = take_opinions(&mut value);
    for axis in repair_hsl_axis_lengths(&mut value) {
        warnings.push(format!("hsl repaired: {axis}"));
    }
    for dropped in expand_ai_mask_intents(&mut value) {
        warnings.push(format!("ai mask dropped: {dropped}"));
    }
    match serde_json::from_value::<EditRecipe>(value) {
        Ok(recipe) => Ok((recipe, warnings)),
        Err(e) => {
            let detail = e.to_string();
            let field = detail
                .split("unknown field `")
                .nth(1)
                .and_then(|s| s.split('`').next())
                .or_else(|| {
                    detail.split("missing field `").nth(1).and_then(|s| s.split('`').next())
                })
                .map(str::to_owned);
            Err(vec![json!({
                "path": "$",
                "field": field,
                "expected": "a field the schema lists",
                "detail": detail,
            })])
        }
    }
}

fn hash_recipe(recipe: &EditRecipe) -> String {
    let value = serde_json::to_value(recipe).unwrap_or(Value::Null);
    crate::sha256::sha256_hex(canonical(&value).as_bytes())
}

/// Key-sorted JSON, so a hash does not depend on insertion order.
fn canonical(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for key in keys {
                    out.insert(key.clone(), sorted(&map[key]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    serde_json::to_string(&sorted(value)).unwrap_or_default()
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn tool_ok(value: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "isError": false,
    })
}

fn tool_failure(code: &str, message: impl Into<String>) -> Value {
    let body = json!({ "ok": false, "code": code, "message": message.into() });
    json!({
        "content": [{ "type": "text", "text": body.to_string() }],
        "isError": true,
    })
}

fn reply_result(out: &mut impl Write, id: Value, result: Value) -> Result<()> {
    write_line(out, &json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn reply_error(out: &mut impl Write, id: Value, code: i64, message: &str) -> Result<()> {
    write_line(out, &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
}

fn write_line(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_form_ignores_key_order() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":2,"x":3}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":3,"y":2},"b":1}"#).unwrap();
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(hash_recipe(&EditRecipe::default()), hash_recipe(&EditRecipe::default()));
    }

    #[test]
    fn parse_recipe_reports_the_offending_field() {
        let mut value = serde_json::to_value(EditRecipe::default()).unwrap();
        value["masks"] = json!([{ "mask": { "kind": "linear", "full_bottom": 0.6 } }]);
        let errors = parse_recipe(value).unwrap_err();
        assert!(
            errors[0]["detail"].as_str().unwrap().contains("full_bottom"),
            "the validator must name the field the model invented: {}",
            errors[0]
        );
    }

    #[test]
    fn tool_table_has_unique_names_and_closed_schemas() {
        let tools = tool_definitions();
        let mut seen = HashSet::new();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(seen.insert(name.to_string()), "duplicate tool {name}");
            assert_eq!(
                tool["inputSchema"]["additionalProperties"],
                json!(false),
                "{name} must reject fields it does not list"
            );
        }
    }
}

#[cfg(test)]
mod mcp_tool_tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autoshade-mcp-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    fn tool_body(reply: &Value) -> Value {
        let text = reply["content"][0]["text"].as_str().expect("tool text body");
        serde_json::from_str(text).expect("tool body JSON")
    }

    fn is_error(reply: &Value) -> bool {
        reply["isError"].as_bool().unwrap_or(false)
    }

    #[test]
    fn validate_returns_the_canonical_recipe_and_a_stable_hash() {
        let root = temp_root("validate");
        let mut server = Server::new(vec![root.clone()], root.join("data")).expect("server");
        let mut value = serde_json::to_value(EditRecipe::default()).unwrap();
        // A short HSL axis is repaired by the canonicaliser; the tool must hand
        // the repaired recipe back (the deferred GUI seam commits this shape,
        // not the model's raw candidate).
        value["hsl"]["hue"] = json!([0.0, 1.0]);
        let reply = server
            .call_tool(&json!({
                "name": "recipe.validate",
                "arguments": { "recipe": value }
            }))
            .expect("validate call");
        assert!(!is_error(&reply));
        let body = tool_body(&reply);
        assert_eq!(body["ok"], true);
        assert!(body["hash"].as_str().is_some_and(|h| !h.is_empty()));
        assert_eq!(
            body["recipe"]["hsl"]["hue"].as_array().map(Vec::len),
            Some(8),
            "validate must return the repaired recipe: {}",
            body["recipe"]["hsl"]["hue"]
        );
        assert!(
            body["warnings"]
                .as_array()
                .is_some_and(|w| w.iter().any(|x| x.as_str().unwrap_or("").contains("hsl repaired"))),
            "the repair is disclosed: {}",
            body["warnings"]
        );
        // Same recipe -> same hash; the canonical form is order-independent.
        let again = tool_body(
            &server
                .call_tool(&json!({
                    "name": "recipe.validate",
                    "arguments": { "recipe": body["recipe"] }
                }))
                .expect("second validate"),
        );
        assert_eq!(again["hash"], body["hash"]);
    }

    #[test]
    fn validate_refuses_an_invented_field_with_structured_errors() {
        let root = temp_root("validate-bad");
        let mut server = Server::new(vec![root.clone()], root.join("data")).expect("server");
        let mut value = serde_json::to_value(EditRecipe::default()).unwrap();
        value["masks"] = json!([{ "mask": { "kind": "linear", "full_bottom": 0.6 } }]);
        let body = tool_body(
            &server
                .call_tool(&json!({
                    "name": "recipe.validate",
                    "arguments": { "recipe": value }
                }))
                .expect("validate call"),
        );
        assert_eq!(body["ok"], false);
        assert!(body["hash"].is_null());
        assert!(
            body["errors"][0].to_string().contains("full_bottom"),
            "the error names the invented field: {}",
            body["errors"]
        );
    }

    #[test]
    fn commit_refuses_a_hash_the_session_never_validated_then_accepts_the_real_one() {
        let root = temp_root("commit");
        let data = root.join("data");
        let mut server = Server::new(vec![root.clone()], data).expect("server");
        // A tiny baked source is enough: commit's gate is the hash, not the
        // sensor format.
        let photo = root.join("tiny.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([90, 90, 90]))
            .save(&photo)
            .expect("write png");
        let recipe = serde_json::to_value(EditRecipe::default()).unwrap();

        let wrong = server
            .call_tool(&json!({
                "name": "recipe.commit",
                "arguments": { "photo": "tiny.png", "recipe": recipe, "hash": "deadbeef" }
            }))
            .expect("commit call");
        assert!(is_error(&wrong), "an unvalidated hash must be refused");
        let body = tool_body(&wrong);
        assert_eq!(body["code"], "commit_unvalidated");
        assert!(!crate::store::develop_dir(&photo).join("recipe.json").exists());

        let validated = tool_body(
            &server
                .call_tool(&json!({
                    "name": "recipe.validate",
                    "arguments": { "recipe": recipe }
                }))
                .expect("validate"),
        );
        let hash = validated["hash"].as_str().expect("hash").to_string();
        let committed = server
            .call_tool(&json!({
                "name": "recipe.commit",
                "arguments": { "photo": "tiny.png", "recipe": recipe, "hash": hash }
            }))
            .expect("commit call");
        assert!(!is_error(&committed), "the validated hash must commit: {committed}");
        assert!(
            crate::store::develop_dir(&photo).join("recipe.json").exists(),
            "commit wrote the develop"
        );
    }
}
