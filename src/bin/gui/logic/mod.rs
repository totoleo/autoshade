//! Headless application logic: no widget layer lives here.
//!
//! Every function takes data and returns data — the recipe it may change, plus
//! the [`Effect`]s the caller has to run. The view (`panels/`, `canvas.rs`,
//! `app.rs`) draws a frame and emits [`Action`]s; the facade (`runtime.rs`)
//! applies those actions here and executes the returned effects. User-facing
//! wording leaves this layer as [`UiText`] — an i18n key plus its named
//! arguments, not a rendered sentence.
//!
//! The rules are pinned by `clippy.toml`: nothing under `logic/` mentions the
//! widget library, renders text, spawns workers or opens dialogs. The module
//! carries `#[deny(clippy::disallowed_types, clippy::disallowed_methods)]`;
//! the crate root allows those lints everywhere else.

pub(crate) mod ai;
pub(crate) mod command;
pub(crate) mod cards;
pub(crate) mod analysis_landing;
pub(crate) mod analysis_workflow;
pub(crate) mod crop;
pub(crate) mod curve;
pub(crate) mod fields;
pub(crate) mod develop;
pub(crate) mod export_landing;
pub(crate) mod fit;
pub(crate) mod folder;
pub(crate) mod gallery;
pub(crate) mod lens_blur;
pub(crate) mod mask_edit;
pub(crate) mod models;
pub(crate) mod noise;
pub(crate) mod normal;
pub(crate) mod paste;
pub(crate) mod mask_place;
pub(crate) mod mask_refine;
pub(crate) mod pick;
pub(crate) mod prefs;
pub(crate) mod portrait;
pub(crate) mod recipes;
pub(crate) mod settings;
pub(crate) mod slider;
pub(crate) mod strip;
pub(crate) mod style;
pub(crate) mod xmp_disclosure;
pub(crate) mod range;
pub(crate) mod region;
pub(crate) mod retouch;
pub(crate) mod same;
pub(crate) mod segmented;
pub(crate) mod list;
pub(crate) mod look;
pub(crate) mod lru;
pub(crate) mod version;
pub(crate) mod view;

use std::path::PathBuf;
use std::sync::Arc;

/// A short display list: the first three entries joined, with an ellipsis when
/// there were more. Used by landings that name files in a status line.
pub(crate) fn brief_list(v: &[String]) -> String {
    let mut d = v.iter().take(3).cloned().collect::<Vec<_>>().join(" · ");
    if v.len() > 3 {
        d = format!("{d} …");
    }
    d
}

/// The tone of a toast a landing asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Error,
    Success,
}

/// One landing's user-facing outcome: the status line, and a toast when the
/// outcome needs to interrupt.
pub(crate) struct Words {
    pub(crate) status: UiText,
    pub(crate) toast: Option<(Tone, UiText)>,
}

/// One named argument of a [`UiText`]: literal text, or a catalogue key that
/// the renderer translates (a verdict word, say) — the logic layer carries the
/// KEY, never the rendered word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Arg {
    Key(autoshade::text::Key),
    Text(String),
}

/// One line the caller should show: the resource key plus its named arguments.
/// Rendering lives outside this layer (`i18n::text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiText {
    pub(crate) key: autoshade::text::Key,
    pub(crate) args: Vec<(&'static str, Arg)>,
}

impl UiText {
    pub(crate) fn of(key: impl Into<autoshade::text::Key>) -> Self {
        Self { key: key.into(), args: Vec::new() }
    }

    /// A key with named arguments (`{name}` placeholders, as `trf` takes them).
    pub(crate) fn with(key: impl Into<autoshade::text::Key>, args: &[(&'static str, &str)]) -> Self {
        Self {
            key: key.into(),
            args: args.iter().map(|(name, value)| (*name, Arg::Text((*value).to_owned()))).collect(),
        }
    }

    /// A key with named arguments, some of which are resource keys to
    /// translate at render time.
    pub(crate) fn with_args(key: impl Into<autoshade::text::Key>, args: &[(&'static str, Arg)]) -> Self {
        Self { key: key.into(), args: args.to_vec() }
    }
}

/// Something the caller must do FOR the logic — the effects that need a
/// thread, a frame or the app's own state, kept out of the functions above so
/// they stay synchronous and testable.
#[derive(Debug)]
pub(crate) enum Effect {
    /// The line a view verb owns (the panel maps it to its own line enum).
    LookLine { kind: look::LookKind, line: look::LookLine },
    /// A one-click landed: remember the values 「back to the one-click result」
    /// restores for this photo.
    RememberOneClick { photo: PathBuf, fine: look::FineTune },
    /// Hand a started verb over to the look protocol (the reference-folder
    /// build's landing starts the look it built for).
    StartLook(Box<look::LookIntent>),
    /// Read this folder into Normal's reference library, for this photo.
    ReferenceBuild { photo: PathBuf, dir: PathBuf },
    /// Run the depth sidecar over this photo's original-frame base.
    EstimateDepth { photo: PathBuf, base: Arc<image::DynamicImage> },
    Status(UiText),
    /// An error toast. The only tone the logic raises so far — a success tone
    /// is a variant away when a domain needs one.
    ToastError(UiText),
    ArmFocusPick(bool),
    /// Put one armed eyedropper (WB / range / point colour) down.
    DisarmPicker(pick::Picker),
    /// The AI region box (the logic layer decided and worded it).
    SetRegion(Option<[f32; 4]>),
    /// One canvas tool at a time: every arming site disarms the rest.
    DisarmTools,
    MarkDirty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::text::generated::*;

    #[test]
    fn brief_list_names_up_to_three_and_marks_the_overflow() {
        assert_eq!(brief_list(&[]), "");
        assert_eq!(brief_list(&["a".into()]), "a");
        assert_eq!(brief_list(&["a".into(), "b".into()]), "a · b");
        assert_eq!(brief_list(&["a".into(), "b".into(), "c".into()]), "a · b · c");
        assert_eq!(brief_list(&["a".into(), "b".into(), "c".into(), "d".into()]), "a · b · c …");
    }

    #[test]
    fn ui_text_carries_its_key_and_named_arguments() {
        let plain = UiText::of(K_LANDING_AI_DEVELOP_APPLIED);
        assert_eq!(
            (autoshade::text::english(plain.key), plain.args.len()),
            ("AI develop applied", 0)
        );
        let with = UiText::with(K_LANDING_EXPORTED, &[("path", "out/a.jpg")]);
        assert_eq!(with.args.len(), 1);
        assert_eq!(with.args[0].0, "path");
        assert_eq!(with.args[0].1, Arg::Text("out/a.jpg".into()));
    }

    #[test]
    fn a_key_argument_stays_a_key_at_this_layer() {
        let words = UiText::with_args(
            K_LANDING_AI_EDIT_APPLIED_THE_AI_S_SELF_CHECK_SAID_NOT_SAV,
            &[("v", Arg::Key(K_LOGIC_ACCEPT))],
        );
        assert_eq!(words.args[0].1, Arg::Key(K_LOGIC_ACCEPT), "the decision word is translated at the render site");
    }

    #[test]
    fn words_pair_a_status_with_an_optional_toast() {
        let words = Words { status: UiText::of(K_LANDING_AI_DEVELOP_APPLIED_SAVED_TO_RECIPE_JSON), toast: None };
        assert!(words.toast.is_none());
        let status = UiText::of(K_LANDING_AI_DEVELOP_APPLIED_SAVED_TO_RECIPE_JSON);
        let words = Words { status: status.clone(), toast: Some((Tone::Error, status)) };
        assert!(matches!(words.toast, Some((Tone::Error, _))));
    }
}
