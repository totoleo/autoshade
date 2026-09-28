//! XML text helpers: ACR's number spelling, character admission, and attribute / text escaping.

/// Format an integer-valued slider the way ACR writes it: explicit `+` for
/// positives (`"+14"`, `"-12"`, `"0"`).
pub(super) fn signed(v: f32) -> String {
    let i = v.round() as i64;
    if i > 0 {
        format!("+{i}")
    } else {
        i.to_string()
    }
}

pub(super) fn xml_char_allowed(c: char) -> bool {
    (!c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        && !matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

pub(super) fn xml_text_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().filter(|&c| xml_char_allowed(c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub(super) fn xml_attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().filter(|&c| xml_char_allowed(c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Attribute-value normalization (XML 1.0 §3.3.3) folds a RAW
            // tab/newline/CR to a space in every compliant parser — a mask
            // name holding one would change on its first round trip through
            // Lightroom. Character references are exempt from normalization,
            // and our own reader's xml_unescape decodes them back.
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

pub(super) fn attr(buf: &mut String, key: &str, val: &str) {
    let val = xml_attr_escape(val);
    buf.push_str(&format!("\n    crs:{key}=\"{val}\""));
}



/// Format a LOCAL adjustment value the way ACR writes it: a bare decimal, no
/// forced `+` (e.g. `"-0.075"`, `"0"`). Distinct from the global `signed()`.
pub(super) fn local_fmt(v: f32) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}
