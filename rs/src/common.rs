/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Shared lexical helpers. Port of `ts/src/common.ts` and `go/common.go`:
//! the character classes the spec names, backslash and entity unescaping
//! (sections 6.1 and 6.2), link-label normalisation (section 4.7), and the
//! URL and XML escaping the HTML renderer needs.
//!
//! These are the pieces where "close enough" silently costs conformance
//! examples, so each one names the spec rule it implements.
//!
//! Two things differ from the TypeScript, both because Rust has a better
//! tool for the job:
//!
//! - Unicode punctuation is asked of the `regex` crate's Unicode tables
//!   (`\p{P}` and `\p{S}`), which is the same property escape the
//!   TypeScript uses, rather than an enumerated code-point table.
//! - Case folding for label matching uses `str::to_lowercase` and
//!   `str::to_uppercase`, which implement the FULL Unicode mappings (so
//!   `ß` uppercases to `SS`, and CommonMark example 540 holds) exactly as
//!   JavaScript's do. The Go port needs a hand-built table for this.

use std::sync::OnceLock;

use regex::Regex;

use crate::entities;

/// The full ASCII punctuation set of section 2.1: everything that may
/// follow a backslash.
pub const ESCAPABLE_ASCII: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// Section 6.2: a valid entity is always semicolon-terminated. Both cases
/// are spelled out, so no `(?i)` flag is needed; the TypeScript applies `i`
/// to a lowercase pattern.
pub const ENTITY_PATTERN: &str =
    r"&(?:#[xX][a-fA-F0-9]{1,6}|#[0-9]{1,7}|[a-zA-Z][a-zA-Z0-9]{1,31});";

fn re_entity_or_escaped_char() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"\\[{}]|{}",
            regex::escape(ESCAPABLE_ASCII),
            ENTITY_PATTERN
        ))
        .expect("the unescape pattern is a literal and compiles")
    })
}

fn re_entity_whole() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!("^(?:{ENTITY_PATTERN})$"))
            .expect("the entity pattern is a literal and compiles")
    })
}

fn re_whitespace_run() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[ \t\r\n]+").expect("a literal pattern compiles"))
}

/// Section 2.1 Unicode punctuation, which 0.31.2 widened from the P*
/// categories alone to P* union S*; that is why `$ + < = > ^ ` | ~ £ €`
/// count. Expressed with Unicode property escapes rather than an
/// enumerated table: the table form silently rots against each Unicode
/// revision, and a P-only table is exactly the near-miss that costs
/// emphasis-flanking examples.
fn re_unicode_punctuation() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[\p{P}\p{S}]$").expect("a literal pattern compiles"))
}

/// Whether `b` is ASCII punctuation, i.e. may follow a backslash (section
/// 6.1).
pub fn is_escapable(b: u8) -> bool {
    ESCAPABLE_ASCII.as_bytes().contains(&b)
}

/// Section 2.3 Unicode whitespace. Deliberately not `char::is_whitespace`:
/// that set differs from the spec's at the edges, and emphasis flanking
/// (section 6.4) is decided on exactly this set.
pub fn is_unicode_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' ')
        || matches!(
            c,
            '\u{00A0}'
                | '\u{1680}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
        )
        || ('\u{2000}'..='\u{200A}').contains(&c)
}

/// Section 2.1 Unicode punctuation: P* union S*. Used only by the emphasis
/// flanking rules.
pub fn is_unicode_punctuation(c: char) -> bool {
    if c.is_ascii() {
        // Every ASCII character in P or S is in the section 2.1 ASCII set,
        // and vice versa.
        return is_escapable(c as u8);
    }
    let mut buf = [0u8; 4];
    re_unicode_punctuation().is_match(c.encode_utf8(&mut buf))
}

/// Section 6.2. Named references resolve from the HTML5 table; numeric
/// ones from the code point, with U+0000, surrogates and anything out of
/// range folded to U+FFFD as the spec requires. Anything that is not a
/// valid reference comes back unchanged.
pub fn decode_entity(text: &str) -> String {
    if !re_entity_whole().is_match(text) {
        return text.to_string();
    }

    let body = &text[1..text.len() - 1];

    if let Some(numeric) = body.strip_prefix('#') {
        let (digits, radix) = match numeric.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (numeric, 10),
        };
        let cp = u32::from_str_radix(digits, radix).ok();
        return match cp {
            Some(cp) if cp != 0 && cp <= 0x10FFFF && !(0xD800..=0xDFFF).contains(&cp) => {
                char::from_u32(cp)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "\u{FFFD}".to_string())
            }
            _ => "\u{FFFD}".to_string(),
        };
    }

    match entities::lookup(body) {
        Some(replacement) => replacement.to_string(),
        None => text.to_string(),
    }
}

/// Sections 6.1 and 6.2: resolve backslash escapes and entity references
/// together.
pub fn unescape_string(s: &str) -> String {
    if !s.contains(['\\', '&']) {
        return s.to_string();
    }
    re_entity_or_escaped_char()
        .replace_all(s, |captures: &regex::Captures<'_>| {
            let m = &captures[0];
            if let Some(escaped) = m.strip_prefix('\\') {
                escaped.to_string()
            } else {
                decode_entity(m)
            }
        })
        .into_owned()
}

/// JavaScript whitespace: the ECMAScript WhiteSpace and LineTerminator
/// productions, which are both what `\s` matches and what
/// `String.prototype.trim()` removes. That is the spec's section 2.3
/// Unicode whitespace plus the vertical tab and U+FEFF.
///
/// Deliberately distinct from [`is_unicode_whitespace`], which is section
/// 2.3 exactly and answers a different question (emphasis flanking). And
/// deliberately used in place of `str::trim` wherever the canonical
/// runtime uses a JavaScript trim or `\s`: `char::is_whitespace` trims
/// U+0085, which JavaScript keeps, and keeps U+FEFF, which JavaScript
/// trims. Four separate places turn on exactly this set (the fence info
/// string, the trailing-space test that decides a hard line break, the
/// code fence's language/meta split, and section 4.7 label matching), so
/// it is defined once.
pub fn is_js_space(c: char) -> bool {
    c == '\u{b}' || c == '\u{FEFF}' || is_unicode_whitespace(c)
}

/// Exactly what `String.prototype.trim()` removes.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// The byte index of the first JavaScript-whitespace character in `s`,
/// with its UTF-8 width, or `None` when there is none. It is the Rust
/// spelling of `s.search(/\s/)`, except that the index is a byte offset
/// and the width comes back too: JavaScript can advance past the match
/// with `+ 1` because every character in the class is BMP, where Rust
/// must step a whole character.
pub fn js_space_index(s: &str) -> Option<(usize, usize)> {
    s.char_indices()
        .find(|(_, c)| is_js_space(*c))
        .map(|(index, c)| (index, c.len_utf8()))
}

/// Section 4.7 label matching: case-insensitive under Unicode case folding,
/// with internal whitespace collapsed.
///
/// Lowercase then uppercase is the approximation of full case folding the
/// reference implementation uses; it makes ẞ/ß and Σ/ς agree. Rust's
/// `to_lowercase` and `to_uppercase` are the full mappings, the same ones
/// JavaScript's `toLowerCase` and `toUpperCase` apply.
pub fn normalize_reference(raw_label: &str) -> String {
    let trimmed = js_trim(raw_label);
    let collapsed = re_whitespace_run().replace_all(trimmed, " ");
    if collapsed.is_ascii() {
        // For ASCII, lowercasing then uppercasing is just uppercasing.
        return collapsed.to_ascii_uppercase();
    }
    collapsed.to_lowercase().to_uppercase()
}

/// HTML output escaping, applied to text and, identically, to attribute
/// values.
pub fn escape_xml(s: &str) -> String {
    if !s.contains(['&', '<', '>', '"']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The set left unencoded by [`normalize_uri`], matching the reference
/// renderer so hrefs come out byte-identical to the spec's expected HTML.
const URL_SAFE: &str = ";/?:@&=+$,-_.!~*'()#";

fn is_url_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric() || URL_SAFE.as_bytes().contains(&b)
}

const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";

fn push_percent(out: &mut String, b: u8) {
    out.push('%');
    out.push(UPPER_HEX[(b >> 4) as usize] as char);
    out.push(UPPER_HEX[(b & 0x0F) as usize] as char);
}

/// Percent-encode a destination for output, preserving sequences that are
/// already valid `%XX` triplets so a pre-encoded URL is not double-encoded.
/// A multi-byte character is encoded as its UTF-8 bytes, which is what the
/// canonical `encodeURIComponent` produces for it.
pub fn normalize_uri(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = String::with_capacity(uri.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&uri[i..i + 3]);
            i += 3;
            continue;
        }
        if c.is_ascii() {
            if is_url_safe(c) {
                out.push(c as char);
            } else {
                push_percent(&mut out, c);
            }
            i += 1;
            continue;
        }
        // Multi-byte: percent-encode every UTF-8 byte of the character.
        push_percent(&mut out, c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_decode_per_section_6_2() {
        assert_eq!(decode_entity("&amp;"), "&");
        assert_eq!(decode_entity("&#65;"), "A");
        assert_eq!(decode_entity("&#x41;"), "A");
        assert_eq!(decode_entity("&#0;"), "\u{FFFD}");
        assert_eq!(decode_entity("&#xD800;"), "\u{FFFD}");
        assert_eq!(decode_entity("&#1114112;"), "\u{FFFD}");
        assert_eq!(decode_entity("&nosuch;"), "&nosuch;");
        assert_eq!(decode_entity("&ampa;"), "&ampa;");
        assert_eq!(decode_entity("&amp"), "&amp");
        assert_eq!(decode_entity("&nGt;"), "\u{226B}\u{20D2}");
    }

    #[test]
    fn unescape_resolves_both_forms_together() {
        assert_eq!(unescape_string(r"\*a\* &amp; b"), "*a* & b");
        assert_eq!(unescape_string("plain"), "plain");
        assert_eq!(unescape_string(r"\a"), r"\a");
    }

    #[test]
    fn label_normalisation_uses_full_case_mapping() {
        assert_eq!(normalize_reference("  Foo   Bar "), "FOO BAR");
        assert_eq!(normalize_reference("ẞ"), normalize_reference("SS"));
        assert_eq!(normalize_reference("Σ"), normalize_reference("ς"));
    }

    #[test]
    fn punctuation_is_p_union_s() {
        for c in [
            '$', '+', '<', '=', '>', '^', '`', '|', '~', '£', '€', '«', '。',
        ] {
            assert!(is_unicode_punctuation(c), "{c:?}");
        }
        for c in ['a', '1', ' ', 'é', '\u{200B}'] {
            assert!(!is_unicode_punctuation(c), "{c:?}");
        }
    }

    #[test]
    fn uri_normalisation_matches_the_reference_renderer() {
        assert_eq!(normalize_uri("/ä"), "/%C3%A4");
        assert_eq!(normalize_uri("/a%20b"), "/a%20b");
        assert_eq!(normalize_uri("/a b"), "/a%20b");
        assert_eq!(
            normalize_uri("http://x.y/?q=1&r=(2)"),
            "http://x.y/?q=1&r=(2)"
        );
    }

    #[test]
    fn xml_escaping() {
        assert_eq!(escape_xml("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }
}
