// Round-12 split: body moved verbatim from main.rs's inline
// `mod tests` (indentation kept — raw-string fixtures must not
// change by one byte). `super::*` still resolves to the root.
    use super::*;
    use crate::harness::*;

    // The tests themselves, in parts under tests/: each file is spliced in here
    // by include!, so its items are items of this module and every test keeps
    // its name and its path. The order is the order the old file had.
    /// Every GUI source file except the tests themselves, concatenated. The
    /// old pins read one named file; splitting a file then turned an invariant
    /// test red even though the invariant held. These scans carry no location
    /// claim, so they read the tree instead.
    fn gui_source() -> &'static str {
        static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SOURCE.get_or_init(|| {
            fn walk(dir: &std::path::Path, out: &mut String) {
                let Ok(entries) = std::fs::read_dir(dir) else { return };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if path.file_name().is_some_and(|n| n == "tests") {
                            continue;
                        }
                        walk(&path, out);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        if let Ok(text) = std::fs::read_to_string(&path) {
                            out.push_str(&text);
                            out.push('\n');
                        }
                    }
                }
            }
            let mut out = String::new();
            walk(
                &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bin/gui"),
                &mut out,
            );
            out
        })
    }

    include!("tests/cards_and_versions.rs");
    include!("tests/canvas_and_delivery.rs");
    include!("tests/theme_fonts_curves.rs");
    include!("tests/develop_and_stash.rs");
    include!("tests/ai_cards.rs");
    include!("tests/strip_and_landing.rs");
    include!("tests/resolution_and_language.rs");
    include!("tests/prefs_and_workers.rs");
    include!("tests/disclosures_and_census.rs");
    include!("tests/panels_and_denoise_prefs.rs");
    include!("tests/sections_and_dots.rs");
    include!("tests/ai_panel.rs");
    include!("tests/turns_and_layout.rs");
    include!("tests/headings_and_widths.rs");
    include!("tests/screenshot.rs");
    include!("tests/lens_blur.rs");
    include!("tests/normal.rs");
    include!("tests/recipes.rs");
    include!("tests/portrait.rs");
    include!("tests/fit_prompt.rs");
    include!("tests/architecture.rs");

    /// One function's text (signature through its matching brace), found
    /// anywhere in the GUI tree. The layout-robust form of a function-scoped
    /// pin: the function may move files, and the scan still reads it.
    fn gui_function(name: &str) -> String {
        let source = gui_source();
        let needle = format!("fn {name}(");
        let start = source.find(&needle).unwrap_or_else(|| panic!("{needle} moved"));
        let line = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let text = &source[line..];
        let brace = text.find('{').expect("function body");
        let mut depth = 0usize;
        let mut in_str = false;
        let mut esc = false;
        let mut end = brace;
        for (i, c) in text.char_indices().skip(brace) {
            if in_str {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    in_str = false;
                }
            } else if c == '"' {
                in_str = true;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
        }
        text[..=end].to_string()
    }
