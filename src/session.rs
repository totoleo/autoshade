//! Reading a pi-harness session transcript (`<store>/harness/sessions/*.json`).
//!
//! The transcript is the durable record of every AI decision: per-message
//! token usage/cost, the critic verdicts, the tool loop, and the commit. The
//! CLI (`autoshade session show|replay`) and the tests share these readers so
//! the numbers cannot drift between the printer and the replay.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// Token/cost totals over one transcript.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

/// Sum the assistant messages' `usage` blocks.
pub fn usage_of(value: &Value) -> Usage {
    let mut u = Usage::default();
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return u;
    };
    for m in messages {
        if m.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = m.get("usage") else { continue };
        u.input += usage.get("input").and_then(Value::as_u64).unwrap_or(0);
        u.output += usage.get("output").and_then(Value::as_u64).unwrap_or(0);
        u.cache_read += usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
        u.total_tokens += usage.get("totalTokens").and_then(Value::as_u64).unwrap_or(0);
        u.cost_usd += usage
            .pointer("/cost/total")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
    }
    u
}

/// Assistant turns in the transcript.
pub fn assistant_turns(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
                .count()
        })
        .unwrap_or(0)
}

/// Tool calls the model issued (`content[]` blocks of type `toolCall`).
pub fn tool_calls(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
                .filter_map(|m| m.get("content").and_then(Value::as_array))
                .flatten()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("toolCall"))
                .count()
        })
        .unwrap_or(0)
}

/// Tool results the harness fed back.
pub fn tool_results(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("toolResult"))
                .count()
        })
        .unwrap_or(0)
}

/// `["revise 80", "revise 68", …]` in critic order.
pub fn verdict_lines(value: &Value) -> Vec<String> {
    value
        .get("verdicts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| {
                    format!(
                        "{} {}",
                        v.get("decision").and_then(Value::as_str).unwrap_or("?"),
                        v.get("score").and_then(Value::as_i64).unwrap_or(-1)
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `(develop, xmp, note)` from `commit`, accepting both the object the harness
/// records today and the JSON string older transcripts kept. Empty strings mean
/// "not committed".
pub fn commit_paths(value: &Value) -> (String, String, String) {
    let owned = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    match value.get("commit") {
        Some(c) if c.is_object() => (owned(c, "develop"), owned(c, "xmp"), owned(c, "note")),
        Some(c) if c.is_string() => {
            let parsed: Value =
                serde_json::from_str(c.as_str().unwrap_or("")).unwrap_or(Value::Null);
            (owned(&parsed, "develop"), owned(&parsed, "xmp"), owned(&parsed, "note"))
        }
        _ => (String::new(), String::new(), String::new()),
    }
}

/// Resolve a session id (`analyze-…`), a bare filename, or a JSON path against
/// the harness sessions directory. Ids win over files? No: an explicit existing
/// path is used as-is, then `<dir>/<id>.json`, then `<dir>/<arg>`.
pub fn resolve_session(arg: &str, sessions_dir: &Path) -> Result<PathBuf> {
    let direct = PathBuf::from(arg);
    if direct.is_file() {
        return Ok(direct);
    }
    for candidate in [
        sessions_dir.join(format!("{arg}.json")),
        sessions_dir.join(arg),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "no harness session {arg:?} — pass an id (analyze-…) or the JSON path; looked in {}",
        sessions_dir.display()
    )
}

/// Parse a session file.
pub fn load(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use serde_json::json;

    fn transcript() -> Value {
        json!({
            "session": "analyze-1",
            "photo": "DSC00001.ARW",
            "photo_path": "E:/photos/DSC00001.ARW",
            "model": "deepseek/deepseek-flash",
            "verdicts": [
                { "decision": "revise", "score": 80 },
                { "decision": "accept", "score": 84 }
            ],
            "commit": { "develop": "D:/store/dev", "xmp": "D:/store/dev/x.xmp", "note": "n" },
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "go" },
                { "role": "assistant", "usage": { "input": 100, "output": 20, "cacheRead": 50, "totalTokens": 170, "cost": { "total": 0.01 } },
                  "content": [ { "type": "text" }, { "type": "toolCall" } ] },
                { "role": "toolResult", "content": "ok" },
                { "role": "assistant", "usage": { "input": 200, "output": 40, "cacheRead": 0, "totalTokens": 240, "cost": { "total": 0.02 } },
                  "content": [ { "type": "toolCall" } ] }
            ]
        })
    }

    #[test]
    fn usage_sums_only_assistant_turns() {
        let u = usage_of(&transcript());
        assert_eq!(u.input, 300);
        assert_eq!(u.output, 60);
        assert_eq!(u.cache_read, 50);
        assert_eq!(u.total_tokens, 410);
        assert!((u.cost_usd - 0.03).abs() < 1e-9);
        assert_eq!(usage_of(&json!({})), Usage::default());
    }

    #[test]
    fn tool_loop_counts_calls_and_results() {
        let t = transcript();
        assert_eq!(assistant_turns(&t), 2);
        assert_eq!(tool_calls(&t), 2);
        assert_eq!(tool_results(&t), 1);
        let empty = json!({});
        assert_eq!(assistant_turns(&empty), 0);
        assert_eq!(tool_calls(&empty), 0);
        assert_eq!(tool_results(&empty), 0);
    }

    #[test]
    fn verdicts_keep_critic_order_and_score() {
        assert_eq!(verdict_lines(&transcript()), vec!["revise 80", "accept 84"]);
        assert!(verdict_lines(&json!({})).is_empty());
        // A transcript of a failed run has no verdicts and must not invent one.
        assert!(verdict_lines(&json!({ "error": "model_error" })).is_empty());
    }

    #[test]
    fn commit_accepts_object_and_legacy_string_forms() {
        let t = transcript();
        assert_eq!(
            commit_paths(&t),
            ("D:/store/dev".into(), "D:/store/dev/x.xmp".into(), "n".into())
        );
        let legacy = json!({ "commit": "{\"develop\":\"d\",\"xmp\":\"x\",\"note\":\"\"}" });
        assert_eq!(commit_paths(&legacy), ("d".into(), "x".into(), String::new()));
        assert_eq!(
            commit_paths(&json!({})),
            (String::new(), String::new(), String::new())
        );
    }

    #[test]
    fn resolve_session_accepts_an_id_a_filename_or_a_path() {
        // Under the crate target (writable everywhere; std::env::temp_dir
        // is denied by this sandbox, and a test must not depend on TMP).
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!(
                "session-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("analyze-9.json");
        std::fs::write(&file, "{}").expect("write session");
        assert_eq!(resolve_session("analyze-9", &dir).unwrap(), file);
        assert_eq!(
            resolve_session(file.to_str().unwrap(), &dir).unwrap(),
            file
        );
        let err = resolve_session("analyze-missing", &dir).unwrap_err().to_string();
        assert!(err.contains("no harness session"), "{err}");
    }
}
