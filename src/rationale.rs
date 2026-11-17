//! Deterministic-rationale NOTES (L12#2B): the same fact carried twice —
//! once as the persisted English string (recipe.json, the XMP comment, the
//! `X-Heal-Rationale` header, the CLI printout and the verifier prompt are
//! all English by contract and must stay BYTE-STABLE), and once as typed
//! `(key, args)` pairs riding ALONGSIDE on in-process results only, so the
//! GUI can render them in the session language at draw time.
//!
//! NOT a field on `EditRecipe`: recipe.json is `deny_unknown_fields`
//! (a new field breaks downgrade compatibility) and the whole struct is
//! serialized into the verifier's LLM prompt (tokens + a wider untrusted
//! surface). The notes travel on `FitReport` / `HealReport` / the
//! `produce_recipe` return value instead — never to disk, never to a model.
//!
//! CONTRACT with consumers: the notes render, in order, the rationale
//! string's SUFFIX (`render_en(&notes)` == the tail of the rationale; the
//! prefix, possibly empty, is the AI model's own prose). A consumer strips
//! that suffix to recover the prose and renders the notes localized; when
//! the strip does not match (a byte cap truncated the string, an old build
//! produced it), the whole raw English string is shown instead — silent
//! English is the sanctioned fallback (user decision, 2026-08-11), never a
//! wrong or partial translation.

/// One deterministic rationale fragment: a catalogue key (its English
/// template, byte-for-byte what the persisted string carries) plus the
/// pre-formatted argument values.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub key: crate::text::Key,
    pub args: Vec<(&'static str, String)>,
}

/// Bound on notes riding one result: the RENDERED string is capped
/// elsewhere (MAX_RATIONALE), but the vec itself must not grow without
/// limit if a producer loops. Past the bound, [`push_note`] plants ONE
/// [`TRUNCATED_SENTINEL`] note whose rendering no rationale string carries,
/// so the consumer's suffix strip MISSES and the whole raw English shows —
/// never a partial translation. (Without the pill, 64 retained copies of a
/// REPEATED note could still match the string's tail and localize a
/// truncated subset while the overflow rode as fake "prose" — Codex AL F7.)
///
/// R36: 512, not 64. The bound was set when a fit carried a handful of
/// notes; a zoned fit on the reference pair renders about 115 sentences (a
/// 4x4 + 2x2 tile sweep prints eligibility, boundary, refinement and carrier
/// per tile), so every such fit tripped the pill and the GUI showed its whole
/// rationale in English while the zh panel was on. The string ceiling moves
/// with it (`EditRecipe::MAX_RATIONALE`, 64 KiB → 512 KiB; the two are one
/// contract, re-derived from the source by
/// `the_rationale_ceiling_holds_every_typed_note_a_run_can_produce`), so 512
/// is still a bound a runaway producer hits and one no honest fit reaches.
pub const MAX_NOTES: usize = 512;

/// Renders to text no rationale string contains (a NUL never enters the
/// templates), so a truncated vec can never strip-match its string.
pub const TRUNCATED_SENTINEL: &str = "\u{0}notes-truncated";

impl Note {
    /// The key is a resource id; a `&'static str` is the migration scaffold for
    /// producers that still name a template by its English text, resolved
    /// through `text::key_of_text` (the checker counts those sites).
    pub fn new(key: impl Into<crate::text::Key>, args: Vec<(&'static str, String)>) -> Self {
        Self { key: key.into(), args }
    }

    pub fn plain(key: impl Into<crate::text::Key>) -> Self {
        Self { key: key.into(), args: Vec::new() }
    }
}

/// Argument VALUES that are words rather than measurements.
///
/// `trf` copies an argument's value verbatim into the template, which is right
/// for a number, a path or the model's own prose — and wrong for the handful of
/// args whose value is an ENUMERATED English phrase this engine chose. Those
/// were the one untranslated fragment inside an otherwise translated sentence:
/// the zh Fit panel read 「高强度拟合显示 luma ranges 中…」.
///
/// A REGISTRY, not a special case: `ALL` is what the GUI's `trf` consults
/// before substituting any argument, and what the i18n audit extracts, so a new
/// enumerated value is translated by adding it here and giving it a zh row —
/// and forgotten by neither.
pub mod values {
    /// [`keys::FIT_NOTE_VETO_DISCLOSED`]'s `kind`, when the withheld evidence
    /// is tonal.
    pub const LUMA_RANGES: &str = "luma ranges";
    /// …and when it is chromatic.
    pub const HUE_BANDS: &str = "hue bands";

    // Native-carrier and residual-band outcomes are values, not prose.
    pub const BITMAP_CARRIER: &str = "bitmap";
    pub const FOUR_GRADIENTS: &str = "four gradients";
    pub const UNMEASURED: &str = "unmeasured";
    pub const UNMEASURED_HARD_MASK: &str = "unmeasured (hard mask)";
    pub const NATIVE_TRIAL_GATED: &str = "native trial gated";
    pub const BAND_ESTIMATOR: &str = "band estimator";
    pub const BOUNDARY_BUDGET: &str = "boundary budget";
    pub const DELTA_E_MARGIN: &str = "deltaE margin";
    pub const ZONE_DO_NO_HARM: &str = "zone do-no-harm";
    pub const FRAME_DO_NO_HARM: &str = "frame do-no-harm";
    pub const BOUNDARY_REGRESSION: &str = "boundary regression";
    pub const RAMP_STEEPER: &str = "ramp steeper than target";

    /// Every enumerated value, for the renderer and for the audit.
    pub const ALL: &[&str] = &[
        LUMA_RANGES, HUE_BANDS, BITMAP_CARRIER, FOUR_GRADIENTS, UNMEASURED,
        UNMEASURED_HARD_MASK, NATIVE_TRIAL_GATED, BAND_ESTIMATOR, BOUNDARY_BUDGET,
        DELTA_E_MARGIN, ZONE_DO_NO_HARM, FRAME_DO_NO_HARM, BOUNDARY_REGRESSION,
        RAMP_STEEPER,
    ];
}

/// Every note template as a TEXT RESOURCE: the generated ids whose English and
/// Chinese text live in `resources/strings/*.toml` (see
/// `scripts/check_text.py`). The persisted rationale is rendered from the `en`
/// bundle and stays byte-stable; a typed note carries the id, never the English
/// prose.
pub mod keys {
    pub use crate::text::generated::rationale_keys::*;
}

/// The note keys the GLOBAL reverse-fit solve OWNS: everything
/// `fit::compose_report` and its helpers can emit, which is the whole
/// `// --- reverse fit (fit.rs)` section of [`keys`] except
/// [`keys::ZONE_ROBUST_REJECTED`] (defined there beside its global twin but
/// pushed by the zoned pass).
///
/// It exists for R33 §H, as a DENYLIST: `fit::rescore_report` regenerates
/// this family from the recipe in front of it, and everything ELSE in a
/// prior report — zones, native ranges, spatial tiles, free masks, the
/// boundary gate, the guided refiner, the local field — rides through
/// untouched. The inverse list, an allowlist of what may be carried, is a
/// list a new producer forgets to join, and five families had.
///
/// `rescore_report_carries_every_key_this_module_files_under_the_solve` pins
/// it against this file's own text, so a new key in that section fails the
/// build rather than silently teleporting into a rescored report.
pub(crate) const GLOBAL_SOLVE_KEYS: &[crate::text::Key] = &[
    keys::FIT_SUMMARY_WITH_CURVE, keys::FIT_SUMMARY_NO_CURVE, keys::FIT_SUMMARY_ATMOSPHERE,
    keys::FIT_NOTE_PAIRING_PIXEL, keys::FIT_NOTE_PAIRING_CELL, keys::FIT_NOTE_MODE_MARGIN,
    keys::FIT_NOTE_WB_CELLS_REFUSED, keys::FIT_NOTE_WB_CELLS_VOUCHED,
    keys::FIT_NOTE_WB_CELLS_VOUCHED_AT_SHARE, keys::FIT_NOTE_FAR,
    keys::FIT_NOTE_SAT_PEGGED, keys::FIT_NOTE_ATMOSPHERE_SAT_PEGGED,
    keys::FIT_NOTE_ATMOSPHERE_CONFIDENCE, keys::FIT_NOTE_ATMOSPHERE_POPULATION_EVIDENCE,
    keys::FIT_NOTE_WB_CLAMPED, keys::FIT_NOTE_WB_WITHHELD_FOREIGN_HUE,
    keys::FIT_NOTE_WB_WITHHELD_ROTATION, keys::FIT_NOTE_WB_SEARCH_BOUND,
    keys::FIT_NOTE_CAST_ADMITTED_BY_STRENGTH, keys::FIT_NOTE_CAST_ADMITTED,
    keys::FIT_NOTE_CAST_ADMITTED_FOREIGN, keys::FIT_NOTE_CAST_ADMITTED_FOREIGN_NA,
    keys::FIT_NOTE_CAST_ADMITTED_FAN, keys::FIT_NOTE_CAST_ADMITTED_FAN_NA,
    keys::FIT_NOTE_CAST_HUE_FANNED, keys::FIT_NOTE_CAST_PROJECTED,
    keys::FIT_NOTE_CAST_PROJECTED_FAN, keys::FIT_NOTE_CAST_PROJECTED_FAN_NA,
    keys::FIT_NOTE_DELIVERED_FAN, keys::FIT_NOTE_DELIVERED_FAN_UNCAUSED,
    keys::FIT_NOTE_CAST_HANDED_TO_WB,
    keys::FIT_NOTE_GLOBAL_CAST, keys::FIT_NOTE_VETO_DISCLOSED, keys::FIT_NOTE_STRENGTH,
    keys::FIT_DEGENERATE, keys::FIT_NOTE_REGRESSED, keys::FIT_NOTE_SAT_REDUCED,
    keys::FIT_NOTE_TONE_WITHHELD_HARM, keys::FIT_NOTE_CURVE_WITHHELD_HARM,
    keys::FIT_NOTE_REHUE_BLOCKED, keys::FIT_NOTE_CAST_REJECTED, keys::FIT_NOTE_JOINT,
    keys::FIT_NOTE_JOINT_NONE, keys::FIT_NOTE_JOINT_MISS, keys::FIT_NOTE_JOINT_REFUSED,
    keys::FIT_NOTE_JOINT_REGRESSED, keys::FIT_NOTE_EVIDENCE_CONTRADICTED,
    keys::FIT_NOTE_EVIDENCE_UNMEASURABLE, keys::FIT_NOTE_UNREPRESENTED,
    keys::FIT_NOTE_ATMOSPHERE_UNREPRESENTED, keys::FIT_NOTE_DEEP_ADOPTED,
    keys::FIT_NOTE_NOT_SAME_FRAME, keys::FIT_SUMMARY_WITH_CURVE_PAIRED,
    keys::FIT_SUMMARY_NO_CURVE_PAIRED, keys::FIT_NOTE_ROBUST_REJECTED,
    keys::FIT_NOTE_VOUCHED_CONVERGENCE, keys::FIT_NOTE_CAST_CELLS_VOUCHED,
    keys::FIT_NOTE_EVIDENCE_WITHHELD, keys::FIT_NOTE_DETAIL,
    keys::FIT_NOTE_DETAIL_WITHHELD, keys::FIT_NOTE_HSL_BANDS, keys::FIT_NOTE_HSL_BANDS_VOUCHED,
    keys::FIT_NOTE_HSL_WITHDRAWN_ERROR, keys::FIT_NOTE_HSL_WITHDRAWN_BLIND,
    keys::FIT_NOTE_COLOR_GRADE, keys::FIT_NOTE_COLOR_GRADE_HELD,
    keys::FIT_ATMOSPHERE_REFERENCE_POPULATION, keys::FIT_ATMOSPHERE_REFERENCE_UNPAIRED,
    keys::FIT_ATMOSPHERE_REFERENCE_EXCLUDED, keys::FIT_ATMOSPHERE_REFERENCE_SHARED,
    keys::FIT_ATMOSPHERE_REFERENCE_THIN, keys::FIT_ATMOSPHERE_REFERENCE_UNMEASURED,
    keys::FIT_CORRESPONDENCE, keys::FIT_CORRESPONDENCE_UNAVAILABLE,
];

/// Whether `key` belongs to the global solve's own account — see
/// [`GLOBAL_SOLVE_KEYS`]. Linear over 72 pointers on a path that runs once
/// per rescored report; a set would cost more to build than to skip.
pub(crate) fn is_global_solve_key(key: crate::text::Key) -> bool {
    GLOBAL_SOLVE_KEYS.contains(&key)
}

/// Render ONE note's English text: substitute each `{name}` with its arg,
/// in a single pass over the TEMPLATE only (the GUI trf rule — a value that
/// happens to contain brace syntax is never reinterpreted as markup). An
/// unmatched placeholder stays visible; a bare `{` with no closer is text.
pub fn render_one(n: &Note) -> String {
    let template = crate::text::tr(crate::text::Lang::En, n.key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match n.args.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Render a note sequence to the English string the persisted rationale
/// carries — plain concatenation, each template owns its leading separator.
pub fn render_en(notes: &[Note]) -> String {
    notes.iter().map(render_one).collect()
}

/// Append a note to BOTH carriers at once: the English rendering onto the
/// rationale string (the persisted truth) and the typed note onto the vec
/// (the GUI's localizable copy) — one call site per fact, so the two can
/// never drift. Past [`MAX_NOTES`] the string still grows (it stays the
/// complete record) and the vec stops; the consumer-side strip rule then
/// falls back to raw English rather than rendering a partial translation.
pub fn push_note(rationale: &mut String, notes: &mut Vec<Note>, note: Note) {
    rationale.push_str(&render_one(&note));
    if notes.len() < MAX_NOTES {
        notes.push(note);
    } else if notes.len() == MAX_NOTES {
        // Poison pill (see MAX_NOTES): a truncated vec must never
        // strip-match the (complete) string again.
        notes.push(Note::plain(crate::text::TRUNCATED));
    }
}

/// Reduce an operational error to the single sanitized line safe for a
/// persisted rationale. Full diagnostics remain available to stderr/logging.
pub fn error_line(error: &anyhow::Error) -> String {
    let display = error.to_string();
    let first = display
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("operation failed");
    // A Python traceback opens with "Traceback (most recent call last):" and
    // ends with the exception line; the exception line is the disclosure.
    let first = if first.to_ascii_lowercase().starts_with("traceback") {
        display
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("operation failed")
    } else {
        first
    };
    let mut line = sanitize_error_paths(first);
    if let Some(code) = error.chain().find_map(|cause| exit_code(&cause.to_string())) {
        let canonical = format!("exit {code}");
        if !line.contains(&canonical) {
            let suffix = format!("; {canonical}");
            let room = 160usize.saturating_sub(suffix.chars().count());
            line = format!("{}{}", truncate_chars(&line, room), suffix);
        }
    }
    truncate_chars(&line, 160)
}

fn exit_code(text: &str) -> Option<i32> {
    // ASCII lowercasing keeps every byte offset: a match in `lower` indexes
    // `text` at the same place.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for marker in ["exited", "exit"] {
        for (offset, _) in lower.match_indices(marker) {
            // The WORD, not the substring: "exiting after 3 retries" and
            // "SystemExit" carry no exit code, and the substring test read
            // one out of both ("exits … past 4096" gave 4096).
            let end = offset + marker.len();
            let opens_word = offset == 0 || !bytes[offset - 1].is_ascii_alphanumeric();
            let closes_word = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if !opens_word || !closes_word {
                continue;
            }
            let tail: String = text[end..].chars().take(32).collect();
            for token in tail.split(|c: char| !c.is_ascii_digit() && c != '-') {
                if token.is_empty() || token == "-" {
                    continue;
                }
                if let Ok(value) = token.parse::<i32>() {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn sanitize_error_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        // A drive letter OPENS a token, like the unix rule's slash: the scheme letter
        // inside `https://` is not one, and matching it there ate the URL's
        // scheme ("httpcompletions").
        let drive = chars[i].is_ascii_alphabetic()
            && chars.get(i + 1) == Some(&':')
            && matches!(chars.get(i + 2), Some('/' | '\\'))
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());
        let unix = chars[i] == '/'
            && chars.get(i + 1).is_some_and(|c| !c.is_whitespace())
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());
        if drive || unix {
            let start = i;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && !matches!(chars[i], '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'')
            {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            out.push_str(&path_basename(&token));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn path_basename(token: &str) -> String {
    let mut value = token.trim_end_matches(|c: char| ",.;!?".contains(c)).to_string();
    if let Some((before, suffix)) = value.rsplit_once(':')
        && !suffix.is_empty()
        && suffix.chars().all(|c| c.is_ascii_digit())
    {
        value = before.to_string();
    }
    let parts: Vec<&str> = value.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
    let lower: Vec<String> = parts.iter().map(|part| part.to_ascii_lowercase()).collect();
    if let Some(index) = lower.iter().position(|part| part == "users" || part == "home")
        && parts.len() <= index + 2
    {
        return "[path]".into();
    }
    parts.last().copied().unwrap_or("[path]").into()
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R33 §H. [`GLOBAL_SOLVE_KEYS`] is the denylist `fit::rescore_report`
    /// carries a prior report through, so a key MISSING from it is a global
    /// claim that teleports, unchanged and unearned, onto a recipe someone
    /// adjusted after the solve — the exact defect R23 MED-3 cured.
    ///
    /// The solve's own family is the generated `rationale.fit_*` set (the
    /// reverse-fit section of the old keys module), minus the zoned producer
    /// filed beside its global twin but PUSHED by the zoned pass.
    #[test]
    fn rescore_report_carries_every_key_this_module_files_under_the_solve() {
        // The reverse-fit section's consts are the `rationale.fit_*` family,
        // minus the zoned producers filed under it by name (they are pushed by
        // the zoned pass and must ride through like every local note).
        const ZONED_FIT_KEYS: &[&str] = &["FIT_RESIDUAL_PRE_CALIBRATION"];
        let declared: Vec<crate::text::Key> = crate::text::generated::RATIONALE_KEYS
            .iter()
            .copied()
            .filter(|key| crate::text::name(*key).starts_with("rationale.fit_"))
            .filter(|key| {
                !ZONED_FIT_KEYS
                    .iter()
                    .any(|zoned| crate::text::name(*key) == format!("rationale.{}", zoned.to_ascii_lowercase()))
            })
            .collect();
        assert_eq!(
            declared.len(),
            GLOBAL_SOLVE_KEYS.len(),
            "{} key(s) in the solve's family are not in GLOBAL_SOLVE_KEYS",
            declared.len() as i64 - GLOBAL_SOLVE_KEYS.len() as i64
        );
        // …and each of them is the SAME key, which is what the note carries.
        for key in declared {
            assert!(
                GLOBAL_SOLVE_KEYS.contains(&key),
                "{} is counted but not in GLOBAL_SOLVE_KEYS",
                crate::text::name(key)
            );
        }
    }

    /// The refactor moved every deterministic rationale from `format!` to a
    /// template + args. recipe.json, the XMP comment, the HTTP header, the
    /// CLI printout and the verifier prompt all carry the STRING, so its
    /// bytes must be exactly what the old `format!` produced — pinned here
    /// against the original expressions for representative producers.
    #[test]
    fn deterministic_rationale_renders_byte_identical_english_from_notes() {
        let round = 2usize;
        let e = "boom {braces} stay";
        assert_eq!(
            render_one(&Note::new(
                keys::REVISION_FAILED,
                vec![("round", round.to_string()), ("e", e.to_string())],
            )),
            format!(" [revision round {round} failed ({e}) — keeping the previous verified proposal]"),
        );
        let (err_before, err_after) = (0.2013_f32, 0.0567_f32);
        assert_eq!(
            render_one(&Note::new(
                keys::FIT_SUMMARY_NO_CURVE,
                vec![
                    ("err_before", format!("{err_before:.3}")),
                    ("err_after", format!("{err_after:.3}")),
                ],
            )),
            format!(
                "Reverse-fit from a target rendition (statistical match; the target is not \
                 pixel-aligned, so local masks and per-band hue rotation are not recovered): luma-CDF \
                 → tone sliders {}, chroma → saturation, per-band colour mixer, per-channel cast \
                 curves. Residual look error {err_before:.3} → {err_after:.3}.",
                "(no residual curve)",
            ),
        );
        let (s_share, t_share) = (0.075_f32, 0.226_f32);
        assert_eq!(
            render_one(&Note::new(
                keys::ZONE_SHARE_MISMATCH,
                vec![
                    ("label", "sky".to_string()),
                    ("s", format!("{:.0}", s_share * 100.0)),
                    ("t", format!("{:.0}", t_share * 100.0)),
                ],
            )),
            format!(
                " Note: the {} zone covers {:.0}% of the source frame \
                 but {:.0}% of the target's — the compositions differ, so the \
                 overall distribution residual stays where the global fit \
                 left it.",
                "sky",
                s_share * 100.0,
                t_share * 100.0,
            ),
        );
        assert_eq!(
            render_one(&Note::plain(keys::MASKS_NOT_PRESERVED)),
            "\n⚠ the response did not preserve mask identities (a mask was renamed or \
             duplicated) — your masks were kept unchanged and the model's mask edits were \
             discarded",
        );
    }

    /// The substitution is single-pass over the template: values carrying
    /// brace syntax are inserted verbatim, an unknown placeholder stays
    /// visible, and a bare `{` is plain text (the GUI trf contract).
    #[test]
    fn note_rendering_is_single_pass_and_total() {
        let n = Note::new(
            keys::REVISION_FAILED,
            vec![("round", "1".into()), ("e", "err with {round} inside".into())],
        );
        let s = render_one(&n);
        assert!(s.contains("(err with {round} inside)"), "{s}");
        assert!(!s.contains("(err with 1 inside)"), "value braces must not re-expand: {s}");

        let missing = Note::new(keys::REVISION_FAILED, vec![("round", "1".into())]);
        assert!(render_one(&missing).contains("({e})"), "unmatched placeholder stays visible");

        let both = render_en(&[Note::plain(keys::FIT_NOTE_SAT_PEGGED), Note::plain(keys::FIT_NOTE_REHUE_BLOCKED)]);
        assert!(both.starts_with(" Saturation demand"));
        assert!(both.contains("cap (±60). Colour-cast curves"), "plain concatenation: {both}");
    }

    #[test]
    fn sidecar_failure_disclosure_has_no_traceback_or_home_path() {
        let user = "alice";
        let win_dir = format!("{}:\\Users\\{user}\\Pictures", 'C');
        let win_script = format!("{win_dir}\\run.py:17");
        let unix_home = format!("/{}/{}", "home", user);
        let unix_script = format!("{unix_home}/run.py");
        let win_users = format!("{}:\\Users", 'C');
        let home_root = format!("/{}/", "home");
        let error = anyhow::anyhow!("model sidecar exited Some(7)")
            .context(format!("sidecar failed at {win_script}\nTraceback (most recent call last):\n  {unix_script}"));
        let safe = error_line(&error);
        assert!(!safe.contains("Traceback"), "{safe}");
        assert!(!safe.contains("alice"), "{safe}");
        assert!(!safe.contains("Users") && !safe.contains("home"), "{safe}");
        assert!(safe.contains("run.py") && safe.contains("exit 7"), "{safe}");
        assert!(safe.chars().count() <= 160, "{} chars", safe.chars().count());
        let unix = error_line(&anyhow::anyhow!(format!("failed at {unix_script}")));
        let traceback = error_line(&anyhow::anyhow!(format!(
            "Traceback (most recent call last):\n  File \"{unix_script}\", line 3, in <module>\nValueError: boom at {win_dir}\\x.py"
        )));
        assert!(traceback.starts_with("ValueError: boom"), "{traceback}");
        assert!(!traceback.contains("Traceback") && !traceback.contains("alice"), "{traceback}");
        assert!(unix.contains("run.py") && !unix.contains(unix_home.as_str()) && !unix.contains("alice"), "{unix}");
        assert!(error_line(&anyhow::anyhow!("sidecar exited with code 9")).contains("exit 9"));
        let long = anyhow::anyhow!("worker exited Some(12)").context("x".repeat(300));
        let long = error_line(&long);
        assert!(long.chars().count() <= 160 && long.ends_with("; exit 12"), "{long}");

        for key in [
            keys::FIT_CORRESPONDENCE_UNAVAILABLE,
            keys::ZONED_UNAVAILABLE,
            keys::REVISION_FAILED,
            keys::REVISION_VERIFY_FAILED,
            keys::STYLE_UNAVAILABLE,
            keys::STYLE_REF_IMAGE_FAILED,
            keys::JUDGE_ROUND_FAILED,
            keys::JUDGE_REJUDGE_FAILED,
            keys::JUDGE_UNAVAILABLE,
            keys::HEURISTIC_UNAVAILABLE,
            keys::HEAL_DETECT_FAILED,
        ] {
            let rendered = render_one(&Note::new(
                key,
                vec![
                    ("round", "1".into()),
                    ("score", "70".into()),
                    ("critique", "test".into()),
                    ("mean", "100".into()),
                    ("clip_b", "0".into()),
                    ("clip_w", "0".into()),
                    ("ev", "0".into()),
                    ("hl", "0".into()),
                    ("sh", "0".into()),
                    ("e", safe.clone()),
                ],
            ));
            assert!(!rendered.contains("Traceback"), "{key}: {rendered}");
            assert!(!rendered.contains("alice"), "{key}: {rendered}");
            assert!(!rendered.contains(win_users.as_str()) && !rendered.contains(home_root.as_str()), "{key}: {rendered}");
        }
    }

    /// F3 (2026-09-24 audit). The drive-letter rule matched the letter pair
    /// INSIDE `https://` and collapsed a URL to its last segment glued onto
    /// `http` ("httpcompletions"). A drive letter opens a token; nothing
    /// spells one mid-word.
    #[test]
    fn a_url_scheme_is_not_a_drive_letter() {
        let line = error_line(&anyhow::anyhow!(
            "POST https://api.example.test/v1/chat/completions: 429 Too Many Requests"
        ));
        assert!(line.starts_with("POST https:"), "{line}");
        assert!(!line.contains("httpcompletions"), "{line}");
        // A real drive letter still collapses wherever the sentence puts it.
        let win_text = format!(
            "failed at {}:\\Users\\alice\\x.py and ({}:{}/y.py)",
            'C',
            'D',
            "/tmp"
        );
        let win = error_line(&anyhow::anyhow!(win_text));
        assert!(!win.contains("alice") && !win.contains("Users") && !win.contains("tmp"), "{win}");
        assert!(win.contains("x.py") && win.contains("(y.py)"), "{win}");
    }

    /// F7 (2026-09-24 audit). `exit_code` matched "exit" as a SUBSTRING, so
    /// "exiting after 3 retries" put "; exit 3" on the disclosure line and
    /// "exits every frame past 4096" read an exit code of 4096. The marker
    /// is a word.
    #[test]
    fn exit_codes_are_read_from_the_word_exit_and_not_from_inside_other_words() {
        assert_eq!(exit_code("worker exited Some(12)"), Some(12));
        assert_eq!(exit_code("sidecar exited with code 9"), Some(9));
        assert_eq!(exit_code("sys.exit(1) was called"), Some(1));
        assert_eq!(exit_code("Exit code: 3"), Some(3));
        assert_eq!(exit_code("exit_code=-1"), Some(-1));
        assert_eq!(exit_code("exiting after 3 retries"), None);
        assert_eq!(exit_code("the model exits every frame past 4096"), None);
        assert_eq!(exit_code("SystemExit raised by 2 workers"), None);
        let line = error_line(&anyhow::anyhow!("exiting after 3 retries"));
        assert!(!line.contains("exit 3"), "{line}");
    }

    /// push_note keeps string and vec in lockstep, and the vec stops at the
    /// bound while the string keeps the complete record.
    #[test]
    fn push_note_binds_the_string_and_the_vec_together() {
        let mut r = String::new();
        let mut v = Vec::new();
        for _ in 0..(MAX_NOTES + 3) {
            push_note(&mut r, &mut v, Note::plain(keys::FIT_NOTE_SAT_PEGGED));
        }
        assert_eq!(v.len(), MAX_NOTES + 1, "the bounded vec plus ONE poison pill");
        assert_eq!(v.last().unwrap().key, TRUNCATED_SENTINEL);
        assert_eq!(
            r.len(),
            keys::FIT_NOTE_SAT_PEGGED.len() * (MAX_NOTES + 3),
            "the string keeps the complete record"
        );
        // Codex AL F7: with REPEATED notes the retained 64 renderings match
        // the string's tail byte-for-byte — without the pill the consumer
        // would localize a truncated subset and present the overflow as
        // prose. The pill renders to text the string cannot contain, so the
        // strip misses and the raw-English fallback engages.
        assert!(
            !r.ends_with(render_en(&v).as_str()),
            "a truncated vec must never strip-match its string"
        );
        // Below the bound the two carriers are byte-lockstep — the suffix
        // rule that consumers strip on.
        let mut r2 = String::from("model prose.");
        let mut v2 = Vec::new();
        push_note(&mut r2, &mut v2, Note::plain(keys::FIT_NOTE_SAT_PEGGED));
        push_note(&mut r2, &mut v2, Note::plain(keys::FIT_NOTE_REHUE_BLOCKED));
        assert_eq!(r2.strip_suffix(render_en(&v2).as_str()), Some("model prose."));
    }
}
