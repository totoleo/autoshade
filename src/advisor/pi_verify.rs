//! The analyst/verifier role on the pi harness: a data-only acceptance check.
//!
//! It judges the recipe from data (recipe + metadata + histogram + the grade
//! intent), never the image — the same brief `build_verify_prompt` gave the
//! Claude CLI and the OpenAI chat verifier, now executed by the one-shot pi
//! model call. Text-only: no frames ride the call.
//!
//! S5.10: this replaced `advisor/claude.rs` and `advisor/proposer_verify.rs`; the
//! verification transport no longer needs a Claude/OAuth CLI or an
//! analysis-provider API key, only the pi harness's model key.

use crate::decode::{Histogram, Meta};
use crate::recipe::EditRecipe;

use super::{Advisor, AdvisorError, GradeIntent, Verdict, build_verify_prompt, parse_verdict};

/// One `Advisor` whose model call belongs to the pi harness.
pub struct PiVerifier;

impl PiVerifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PiVerifier {
    fn default() -> Self {
        Self
    }
}

impl Advisor for PiVerifier {
    fn name(&self) -> &'static str {
        "pi-harness"
    }

    fn verify(
        &self,
        recipe: &EditRecipe,
        meta: &Meta,
        hist: &Histogram,
        intent: &GradeIntent,
    ) -> Result<Verdict, AdvisorError> {
        let prompt = build_verify_prompt(recipe, meta, hist, intent)?;
        let call = crate::harness::ModelCall {
            instruction: prompt,
            frames: Vec::new(),
            system: None,
            schema: None,
        };
        let text = crate::harness::complete(&call, super::VERIFY_TIMEOUT_SECS)
            .map_err(|e| AdvisorError::ModelFailure(format!("pi verify: {e:#}")))?;
        parse_verdict(&text, &[])
    }
}
