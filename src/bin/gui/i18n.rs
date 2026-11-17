//! The GUI's binding to the text resources (`autoshade::text`).
//!
//! Every user-facing string is a resource: a stable key whose English and
//! Chinese text live in `resources/strings/{en,zh-Hans}.toml`, compiled into
//! `autoshade::text::generated` by `scripts/gen_text.py`. Call sites name the
//! generated key constant; this module adds the ONE platform-specific
//! presentation rule (macOS's modifier key) and re-exports the language type
//! and the key constants so panels keep their `tr`/`trf` vocabulary.
//!
//! The resource model has no English text as identity: editing copy edits the
//! TOML, not a translation key. `Lang` is persisted in `Prefs` (eframe
//! storage); a save from an older build decodes to `En` via serde's default.

use autoshade::text;

pub use autoshade::text::generated::*;
pub use autoshade::text::Lang;

/// Translate one resource key.
pub fn tr(lang: Lang, key: impl Into<Key>) -> &'static str {
    platform_modifier(text::tr(lang, key.into()))
}

/// Say the modifier key this platform's users actually press.
///
/// The BINDINGS are already right: every shortcut is
/// `egui::Modifiers::COMMAND`, which egui maps to Command on a Mac and Ctrl
/// everywhere else (`app.rs` names `Modifiers::CTRL` nowhere at all). Only the
/// hint strings still said "Ctrl+", so on a Mac the app named a key its own
/// bindings do not listen for.
///
/// Rewritten HERE and not in the ~50 resources: the modifier is presentation,
/// per platform, and a resource should not have to carry a second copy of every
/// shortcut hint. Off macOS this is the identity function by `cfg`, not by
/// branch, so those strings are the same bytes they have always been.
///
/// "Cmd+" rather than ⌘, deliberately: the embedded font subsets are generated
/// from the resources (`scripts/subset_gui_fonts.py`), U+2318 appears nowhere
/// in them, and the runtime face that would supply it is the system CJK font —
/// the one most at risk of being refused by the per-face byte budget. A glyph
/// that renders as a box would be worse than the word.
#[cfg(not(target_os = "macos"))]
fn platform_modifier(s: &'static str) -> &'static str {
    s
}

/// Interned, because [`tr`] returns `&'static str` and a rewrite has to
/// outlive the call. Bounded by the number of DISTINCT strings that mention
/// the modifier — each language's text and its rewrite, leaked at most once,
/// holding memory the resource tables would have held anyway.
#[cfg(target_os = "macos")]
fn platform_modifier(s: &'static str) -> &'static str {
    if !s.contains("Ctrl+") {
        return s;
    }
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<&'static str, &'static str>>> =
        std::sync::OnceLock::new();
    let mut cache =
        CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(s)
        .or_insert_with(|| Box::leak(s.replace("Ctrl+", "Cmd+").into_boxed_str()))
}

/// Translate + interpolate a resource. Arguments are `(name, value)` pairs;
/// each `{name}` placeholder is replaced by its value in one pass over the
/// TEMPLATE, never over inserted values — a directory literally named
/// `{count}` or an OS error quoting a braced path must not be reinterpreted as
/// markup. A placeholder a translation drops stays visible as-is (the existing
/// contract), never a panic.
///
/// An argument VALUE that is an enumerated engine word (`rationale::values`)
/// renders in the session language too, through the one shared registry.
pub fn trf(lang: Lang, key: impl Into<Key>, args: &[(&str, &str)]) -> String {
    let key = key.into();
    let template = tr(lang, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        match after.find('}') {
            Some(close) => {
                let name = &after[1..close];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => {
                        out.push_str(text::value_text(lang, value))
                    }
                    None => out.push_str(&after[..=close]),
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Render one piece of logic-authored text (`logic::UiText`) in `lang`.
///
/// The logic layer never renders: it hands back the resource key plus its
/// named arguments, and this is the one place that turns that data into the
/// sentence the user reads — through the same `tr`/`trf` table every view
/// string uses, so one string cannot have two spellings.
pub fn text(lang: Lang, text: &crate::logic::UiText) -> String {
    use crate::logic::Arg;
    if text.args.is_empty() {
        return tr(lang, text.key).to_owned();
    }
    // A `Key` argument is a resource key in its own right (a verdict word);
    // translate it here, at the one place that renders.
    let rendered: Vec<(&str, String)> = text
        .args
        .iter()
        .map(|(name, value)| {
            let value = match value {
                Arg::Key(key) => tr(lang, *key).to_owned(),
                Arg::Text(text) => text.clone(),
            };
            (*name, value)
        })
        .collect();
    let args: Vec<(&str, &str)> = rendered.iter().map(|(name, value)| (*name, value.as_str())).collect();
    trf(lang, text.key, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS says Cmd — the app's bindings always meant Cmd there, and egui
    /// maps `Modifiers::COMMAND` to it; every other platform says Ctrl. A
    /// string naming no modifier comes back untouched on every platform.
    #[test]
    fn the_modifier_label_follows_the_platform_the_bindings_already_use() {
        let hint = tr(Lang::En, K_APP_CTRL_O_OR_DRAG_A_FILE_INTO_THE_WINDOW);
        if cfg!(target_os = "macos") {
            assert!(hint.starts_with("Cmd+O"), "{hint}");
        } else {
            assert_eq!(hint, "Ctrl+O · or drag a file into the window");
        }
        assert_eq!(tr(Lang::En, K_LANDING_REVISE), "Revise");
    }

    /// A logic-authored line can carry a resource KEY as an argument (a
    /// verdict word). The renderer translates it in the live language — the
    /// logic layer never renders.
    #[test]
    fn a_key_argument_is_translated_by_the_one_renderer() {
        let line = crate::logic::UiText::with_args(
            K_LANDING_AI_EDIT_APPLIED_THE_AI_S_SELF_CHECK_SAID_NOT_SAV,
            &[("v", crate::logic::Arg::Key(K_LANDING_REVISE))],
        );
        assert_eq!(
            text(Lang::Zh, &line),
            trf(
                Lang::Zh,
                K_LANDING_AI_EDIT_APPLIED_THE_AI_S_SELF_CHECK_SAID_NOT_SAV,
                &[("v", tr(Lang::Zh, K_LANDING_REVISE))],
            ),
        );
        assert!(text(Lang::En, &line).contains("Revise"));
    }
}
