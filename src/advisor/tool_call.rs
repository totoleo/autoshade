//! The tool-use loop, after the agent-harness contract (the shape DSH uses):
//!
//! 1. the request carries `tools` — one function, `submit_recipe`, whose
//!    `parameters` is the reply schema the prompts already describe;
//! 2. the model answers with a STRUCTURED `tool_calls[]` entry, never with prose
//!    that happens to look like JSON;
//! 3. the harness appends the call to the conversation and replies with a
//!    `role: "tool"` message that cites the call's id and carries a machine
//!    readable verdict (`{"ok":false,"errors":[…]}`);
//! 4. the model sees that observation on the next step and may correct itself;
//! 5. a bounded number of steps ends the turn, and every path says why.
//!
//! The point that the earlier repair patch missed: the conversation is the
//! state. Appending prose to the prompt starts a NEW call that cannot tell
//! whether it is a retry; a tool result is an observation the model is trained
//! to read.

use serde_json::{json, Value};

use super::{post_ai_json, AdvisorError, SseFamily, VisionRequest};

/// One tool call, four observations. The proposer's reply is a single object, so
/// a well-behaved model needs one call; the rest are self-corrections.
const MAX_STEPS: usize = 4;

const TOOL_NAME: &str = "submit_recipe";

fn text_of(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Validate the submitted arguments. Returns the human/machine readable error
/// list the model will read as the tool's result.
fn validate(args: &Value) -> Result<(), Vec<Value>> {
    let mut errors: Vec<Value> = Vec::new();
    if !args.is_object() {
        return Err(vec![json!({
            "path": "$",
            "expected": "a JSON object",
            "got": args.to_string().chars().take(160).collect::<String>(),
        })]);
    }
    // Either the envelope (`recipe` inside it) or a bare recipe is accepted —
    // the same tolerance the text path has, so a model that skips the wrapper
    // is not punished for it.
    let mut recipe = args.get("recipe").cloned().unwrap_or_else(|| args.clone());
    // VALIDATE THE SAME THING THE TEXT PATH PARSES. The shipped path normalises
    // before `EditRecipe`'s strict serde runs (null opinions, miscounted HSL
    // axes, and `ai` mask INTENTS that only `expand_ai_mask_intents` turns into
    // the engine's `ai_mask` geometry). Validating the raw arguments instead
    // rejected perfectly good proposals — the model was told its `ai` mask was
    // an unknown variant when the text path accepts exactly that spelling.
    let _ = super::openai::take_opinions(&mut recipe);
    let _ = super::openai::repair_hsl_axis_lengths(&mut recipe);
    let _ = super::openai::expand_ai_mask_intents(&mut recipe);
    if let Err(e) = serde_json::from_value::<crate::recipe::EditRecipe>(recipe.clone()) {
        let msg = e.to_string();
        // serde names the offending field, which is the whole feedback loop.
        let field = msg
            .split("unknown field `")
            .nth(1)
            .and_then(|s| s.split('`').next())
            .or_else(|| msg.split("missing field `").nth(1).and_then(|s| s.split('`').next()))
            .map(|s| s.to_string());
        errors.push(json!({
            "path": if args.get("recipe").is_some() { "$.recipe" } else { "$" },
            "field": field,
            "expected": "a field the schema lists",
            "detail": msg,
        }));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// Run the loop. Returns the validated argument object (the envelope).
pub(crate) fn propose_via_tools(
    base_url: &str,
    key: &str,
    req: &VisionRequest<'_>,
    schema: &Value,
    budget_secs: u64,
    effort: Option<&str>,
) -> Result<Value, AdvisorError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");

    let mut content = vec![json!({
        "type": "text",
        "text": format!(
            "Answer ONLY by calling the function `{TOOL_NAME}` with the complete object. \
             Its schema is below; a field the schema does not list is rejected by the \
             validator and you will be told exactly which field failed.\n\n{}",
            req.instruction
        )
    })];
    for f in &req.frames {
        content.push(json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/jpeg;base64,{}", f.b64),
                "detail": f.detail,
            }
        }));
    }

    let tools = json!([{
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Submit the develop proposal: the reply schema below, validation runs before it is accepted.",
            "parameters": schema,
        }
    }]);

    let mut messages: Vec<Value> = vec![json!({ "role": "user", "content": content })];

    for step in 0..MAX_STEPS {
        let body = json!({
            "model": req.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
        });
        let value = post_ai_json(&url, key, body, budget_secs, SseFamily::Chat, effort)?;
        let message = value
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(Value::Null);
        let calls = message
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let Some(call) = calls.first() else {
            // No tool call: the model answered in prose. One observation, then
            // the next step can still succeed — this is a correction, not a
            // failure, because the turn is not over.
            let said = text_of(&message);
            if step + 1 == MAX_STEPS {
                return Err(AdvisorError::ModelFailure(format!(
                    "{TOOL_NAME} was never called after {MAX_STEPS} steps; last words: {}",
                    said.chars().take(300).collect::<String>()
                )));
            }
            messages.push(json!({ "role": "assistant", "content": said }));
            messages.push(json!({
                "role": "user",
                "content": format!(
                    "You did not call `{TOOL_NAME}`. Call it now with the complete object; prose is not accepted."
                )
            }));
            continue;
        };

        let call_id = call
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("call_0")
            .to_string();
        let name = call
            .pointer("/function/name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let raw = call
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // The conversation records the call exactly as the model sent it:
        // a tool result must cite the call it answers.
        messages.push(json!({
            "role": "assistant",
            "content": Value::Null,
            "tool_calls": [call],
        }));

        let result = if name != TOOL_NAME {
            Err(vec![json!({
                "path": "$",
                "expected": format!("a call to `{TOOL_NAME}`"),
                "got": name,
            })])
        } else {
            match serde_json::from_str::<Value>(raw) {
                Ok(args) => validate(&args).and(Ok(args)),
                Err(e) => Err(vec![json!({
                    "path": "$",
                    "expected": "the arguments as a JSON object",
                    "detail": e.to_string(),
                    "got": raw.chars().take(200).collect::<String>(),
                })]),
            }
        };

        match result {
            Ok(args) => return Ok(args),
            Err(errors) => {
                // The observation. `isError` is implicit in `ok:false` on this
                // wire (Chat Completions has no isError flag), and the content
                // is the same structured body a harness would log.
                eprintln!(
                    "  note: the validator refused {TOOL_NAME} (step {}/{}): {}",
                    step + 1,
                    MAX_STEPS,
                    errors
                        .first()
                        .and_then(|e| e.get("detail").or_else(|| e.get("field")))
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                );
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": json!({ "ok": false, "errors": errors }).to_string(),
                }));
            }
        }
    }

    Err(AdvisorError::ModelFailure(format!(
        "{TOOL_NAME} never produced a valid object in {MAX_STEPS} steps"
    )))
}
