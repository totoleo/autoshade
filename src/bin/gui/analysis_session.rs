//! The AI analysis session's state: the decision a landing produced, the
//! rationale the panel shows (raw text + its typed tail), and the scripts the
//! session already disclosed as undrawable. Per-photo session state; cleared
//! wherever a develop is reloaded from disk.

use std::collections::HashSet;

use autoshade::advisor::Decision;
use autoshade::rationale::Note;

/// What the last analysis/reverse-fit produced and the panel is showing.
#[derive(Default)]
pub(crate) struct AnalysisSession {
    /// The decision word + reasons, typed so a language switch re-renders them.
    pub(crate) verdict: Option<(Decision, Vec<String>)>,
    pub(crate) rationale: String,
    /// The rationale's deterministic tail as typed notes; it renders the
    /// rationale's SUFFIX, which the panel strips and draws localized.
    pub(crate) rationale_notes: Vec<Note>,
    /// Scripts already disclosed as undrawable this session (the tofu warning
    /// fires once per script, not once per folder or frame).
    pub(crate) disclosed_scripts: HashSet<&'static str>,
}
