/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Phase 2 of the CommonMark parse (spec 0.31.2, Appendix A, "Phase 2:
//! inline structure"). Port of `ts/src/inline.ts` (canonical) and
//! `go/inline.go`; keep the three in step. The block phase left the raw
//! text of every paragraph and heading in `string_content`; this module
//! turns that text into inline children and clears it.
//!
//! The shape is the spec's own algorithm: one left-to-right scan over the
//! subject that appends nodes as it goes, plus two auxiliary stacks that
//! are resolved out of band, because neither construct can be decided at
//! the point where its opener is seen:
//!
//! - the delimiter stack (section 6.4) holds `*` / `_` (and, under GFM,
//!   `~`) runs, and is resolved by `process_emphasis`;
//! - the bracket stack (section 6.3) holds `[` and `![`, and is resolved
//!   by `parse_close_bracket` when a `]` turns up.
//!
//! Everything else (code spans, autolinks, raw HTML, entities, escapes,
//! breaks) is decided locally at the scan position. That is what produces
//! the precedence section 6.3 requires: a code span, autolink or raw HTML
//! tag is consumed whole during the scan, so it binds more tightly than
//! emphasis and more tightly than the brackets of a link label. Conversely
//! brackets bind more tightly than emphasis, because bracket matching
//! happens during the scan while emphasis matching happens afterwards over
//! the delimiter stack.
//!
//! What differs from the TypeScript, and why (the same list as the Go port):
//!
//! - The subject is scanned by BYTE offset. Every character the scan
//!   branches on is ASCII, and no UTF-8 continuation byte can collide with
//!   an ASCII value, so byte scanning is exact. Wherever a whole
//!   *character* is needed (emphasis flanking above all) the character is
//!   decoded before being handed to the Unicode classifiers.
//! - The sticky (`y`) regexes become `^`-anchored patterns matched against
//!   `subject[pos..]`.
//! - `parse_link_label` and the small whitespace regexes are hand-coded
//!   loops. The label pattern's `{0,1000}` repeat of an alternation would
//!   expand to a thousand copies under a linear-time engine, and the rest
//!   are cheaper as loops.
//! - The delimiter stack is a slab of entries linked by index rather than
//!   by pointer, and the bracket stack is a vector; both are what the
//!   arena tree asks for.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::common::{
    decode_entity, is_escapable, is_unicode_punctuation, is_unicode_whitespace, js_trim,
    normalize_reference, unescape_string, ENTITY_PATTERN, ESCAPABLE_ASCII,
};
use crate::node::{MdNode, NodeId, NodeType, Tree};
use crate::options::{Options, RefDef, RefMap};

// --- Section 6.6 raw HTML: the tag grammar, spelled out because it is
// stricter than `<[^>]*>` in every part. Attribute names are
// XML-restricted-to-ASCII; unquoted attribute values exclude quotes, `=`,
// `<`, `>`, backtick and all control characters; comments, PIs,
// declarations and CDATA each have their own terminator. `INLINE_SPACE*`
// stands in for "spaces, tabs, and up to one line ending": a paragraph can
// never contain a blank line, so a single run can only ever span one line
// ending anyway.

/// JavaScript's `\s`, spelled out: the `regex` crate's `\s` is a different
/// set, so using it here would reject tags the TypeScript accepts.
const INLINE_SPACE: &str = "[ \\t\\n\\x0B\\x0C\\r\
\\x{00a0}\\x{1680}\\x{2000}-\\x{200a}\\x{2028}\\x{2029}\\x{202f}\\x{205f}\\x{feff}\\x{3000}]";

const INLINE_TAG_NAME: &str = "[A-Za-z][A-Za-z0-9-]*";
const INLINE_ATTRIBUTE_NAME: &str = "[a-zA-Z_:][a-zA-Z0-9:._-]*";
const INLINE_UNQUOTED_VALUE: &str = "[^\"'=<>`\\x00-\\x20]+";
const INLINE_SINGLE_QUOTED_VALUE: &str = "'[^']*'";
const INLINE_DOUBLE_QUOTED_VALUE: &str = "\"[^\"]*\"";

/// Section 6.6: `<!-->` and `<!--->` are comments in their own right;
/// otherwise the body simply may not contain `-->`, which the lazy
/// quantifier enforces.
const INLINE_HTML_COMMENT: &str = r"<!-->|<!--->|<!--[\s\S]*?-->";
const INLINE_PROCESSING_INSTRUCTION: &str = r"[<][?][\s\S]*?[?][>]";
const INLINE_DECLARATION: &str = "<![A-Za-z][^>]*>";
/// The TypeScript compiles the whole tag pattern with the `i` flag, which
/// here only ever matters for the literal `CDATA`; every other letter in
/// the grammar is already spelled in both cases. Written out rather than
/// left to `(?i)`, because the `regex` crate folds case over all of
/// Unicode: `(?i)[A-Za-z]` also matches U+017F LATIN SMALL LETTER LONG S
/// and U+212A KELVIN SIGN, where JavaScript's non-unicode `i` does not.
const INLINE_CDATA: &str = r"<!\[[cC][dD][aA][tT][aA]\[[\s\S]*?\]\]>";

/// JavaScript's `.` without the `s` flag: it excludes `\n`, `\r`, U+2028
/// and U+2029.
const INLINE_JS_DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

fn re_html_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let attribute_value = format!(
            "(?:{INLINE_UNQUOTED_VALUE}|{INLINE_SINGLE_QUOTED_VALUE}|{INLINE_DOUBLE_QUOTED_VALUE})"
        );
        let attribute_value_spec = format!("(?:{INLINE_SPACE}*={INLINE_SPACE}*{attribute_value})");
        let attribute = format!("(?:{INLINE_SPACE}+{INLINE_ATTRIBUTE_NAME}{attribute_value_spec}?)");
        let open_tag = format!("<{INLINE_TAG_NAME}{attribute}*{INLINE_SPACE}*/?>");
        let close_tag = format!("</{INLINE_TAG_NAME}{INLINE_SPACE}*>");
        Regex::new(&format!(
            "^(?:{open_tag}|{close_tag}|{INLINE_HTML_COMMENT}|{INLINE_PROCESSING_INSTRUCTION}|{INLINE_DECLARATION}|{INLINE_CDATA})"
        ))
        .expect("the raw HTML pattern compiles")
    })
}

fn re_entity_here() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!("^(?:{ENTITY_PATTERN})")).expect("a literal pattern compiles")
    })
}

/// Section 6.3 link destination, pointy-bracket form: no line endings, no
/// unescaped `<` or `>`.
fn re_link_destination_braces() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(r"^<(?:[^<>\n\\\x00]|\\{INLINE_JS_DOT})*>"))
            .expect("a literal pattern compiles")
    })
}

/// Section 6.3 link title in `"`, `'` or `(...)` delimiters. The escape
/// alternative comes first so that `\"` inside a double-quoted title is
/// consumed as an escape rather than closing the title.
fn re_link_title() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let escaped = format!(r"\\[{}]", regex::escape(ESCAPABLE_ASCII));
        Regex::new(&format!(
            "^(?:\"(?:{escaped}|[^\"\\x00])*\"|'(?:{escaped}|[^'\\x00])*'|\\((?:{escaped}|[^()\\x00])*\\))"
        ))
        .expect("a literal pattern compiles")
    })
}

/// Section 6.5 autolinks. Scheme: an ASCII letter then 1 to 31 more of
/// letter / digit / `+` / `.` / `-`, i.e. 2 to 32 characters in total.
fn re_autolink() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^<[A-Za-z][A-Za-z0-9.+-]{1,31}:[^<>\x00-\x20]*>")
            .expect("a literal pattern compiles")
    })
}

fn re_email_autolink() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            "^<([a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+\
@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\
(?:\\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)>",
        )
        .expect("a literal pattern compiles")
    })
}

/// Maximum `(` nesting inside a bare link destination. The spec describes
/// balanced parentheses without stating a bound; cmark uses 32, and the
/// bound is what keeps an unbalanced run from being quadratic (see
/// `scan_link_destination`).
const MAX_LINK_PAREN_NESTING: usize = 32;

/// The most inline containers (emphasis, strong emphasis, strikethrough,
/// links and images) that may nest inside one block. A delimiter pair,
/// link or image whose content already holds that many nested containers
/// stays literal text.
///
/// The reason is the same as `block::MAX_CONTAINER_NESTING` (see there
/// for the measurements): each wrapper is two levels of the projected
/// `tabnas::Value`, whose destructor and `to_json` recurse per level, and
/// `****x****` nests one `strong` per two stars a side. The canonical
/// TypeScript has no such limit; `DIVERGENCE.md` records it, and
/// `tests/robust_test.rs` pins the boundary.
pub const MAX_INLINE_NESTING: usize = 50;

/// The code span search memo, cmark's `backticks[]`: the start of the
/// last backtick run of each length that a scan has passed, and whether
/// a scan has reached the end of the subject. Once one has, every run
/// after that scan's start is on record, so an opener whose length has
/// no run ahead of it is refused without rescanning. Without it, a
/// subject holding unmatched runs of distinct lengths rescanned its whole
/// tail once per run.
///
/// An entry only ever moves forward. A scan that succeeds part way stops
/// before the last run of its length, and letting it overwrite the
/// position a full scan recorded would hide a later closer from the next
/// opener.
#[derive(Debug, Clone, Default)]
pub struct BacktickMemo {
    last_run: HashMap<usize, usize>,
    scanned_to_end: bool,
}

fn text_node(tree: &mut Tree, literal: &str) -> NodeId {
    let mut node = MdNode::new(NodeType::Text);
    node.literal = literal.to_string();
    tree.add(node)
}

/// Whether a byte can start an inline construct, i.e. whether it
/// terminates a run of ordinary text. The TypeScript spells this as the
/// negated classes of `reMain` / `reMainGfm`; `~` is only special under
/// GFM.
fn is_inline_special_byte(c: u8, gfm: bool) -> bool {
    match c {
        b'\n' | b'`' | b'[' | b']' | b'\\' | b'!' | b'<' | b'&' | b'*' | b'_' => true,
        b'~' => gfm,
        _ => false,
    }
}

/// The TypeScript's `reWhitespaceChar`, `/[ \t\n\v\f\r]/`.
fn is_inline_whitespace_byte(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')
}

/// Drop `n` bytes from the end of a delimiter run's literal. The literal is
/// a run of ASCII `*`, `_` or `~`, so bytes and characters agree.
fn trim_delim_tail(s: &mut String, n: usize) {
    let keep = s.len().saturating_sub(n);
    s.truncate(keep);
}

/// One `*`, `_` or `~` run on the delimiter stack (section 6.4).
#[derive(Debug, Clone)]
struct Delimiter {
    /// The delimiter character.
    cc: u8,
    /// Delimiters still unused; drops as pairs are consumed.
    numdelims: usize,
    /// Run length as originally scanned, what the rule of three is computed
    /// on.
    origdelims: usize,
    node: NodeId,
    previous: Option<usize>,
    next: Option<usize>,
    can_open: bool,
    can_close: bool,
}

/// One `[` or `![` on the bracket stack (section 6.3).
#[derive(Debug, Clone)]
struct Bracket {
    node: NodeId,
    /// Top of the delimiter stack when this bracket opened: the emphasis
    /// floor.
    previous_delimiter: Option<usize>,
    /// Offset of the `[` in the subject.
    index: usize,
    image: bool,
    /// Cleared on earlier openers once a link is built: no links in links.
    active: bool,
    /// Another bracket opened after this one, so its text contains a `[`.
    bracket_after: bool,
}

/// What `scan_delimiter_run` reports about a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimScan {
    pub numdelims: usize,
    pub can_open: bool,
    pub can_close: bool,
}

// --- pure recognizers -------------------------------------------------------
//
// Recognition split from node building: each function inspects `subject`
// at `pos` and reports what it found, moving nothing and touching no tree.
// The scanner methods below delegate to them, and they are public so any
// other driver over the same syntax shares this exact recognition logic
// instead of reimplementing it.

/// Apply a `^`-anchored regex at `pos`; the match text, or `None`.
fn regex_at<'s>(re: &Regex, subject: &'s str, pos: usize) -> Option<&'s str> {
    re.find(&subject[pos..])
        .map(|m| &subject[pos + m.start()..pos + m.end()])
}

/// The whole character ending at `end`, so an astral character is
/// classified as one character by the flanking rules rather than as a
/// stray byte. Start of subject counts as a line ending (section 6.4).
pub fn code_point_before(subject: &str, end: usize) -> char {
    if end == 0 {
        return '\n';
    }
    subject[..end].chars().next_back().unwrap_or('\n')
}

/// The whole character at `i`; end of subject counts as a line ending.
pub fn code_point_at(subject: &str, i: usize) -> char {
    if i >= subject.len() {
        return '\n';
    }
    subject[i..].chars().next().unwrap_or('\n')
}

/// The end of the run of spaces starting at `pos`.
pub fn skip_initial_spaces(subject: &str, mut pos: usize) -> usize {
    let bytes = subject.as_bytes();
    while pos < bytes.len() && bytes[pos] == b' ' {
        pos += 1;
    }
    pos
}

/// The end of "spaces, and at most one line ending" starting at `pos`
/// (section 6.3).
pub fn match_spnl(subject: &str, pos: usize) -> usize {
    let mut pos = skip_initial_spaces(subject, pos);
    if pos < subject.len() && subject.as_bytes()[pos] == b'\n' {
        pos = skip_initial_spaces(subject, pos + 1);
    }
    pos
}

/// A code span (section 6.3), or an unmatched opener, which is literal
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeSpanScan {
    Closed { end: usize, literal: String },
    Open { end: usize, ticks: String },
}

/// The code span starting at `pos`: a backtick string to the next backtick
/// string of *exactly* the same length. `None` when no backtick run starts
/// at `pos`.
///
/// Hand-coded rather than the TypeScript's sticky and global regexes, so
/// that no regex is built from an input-derived run length.
pub fn scan_code_span(subject: &str, pos: usize) -> Option<CodeSpanScan> {
    scan_code_span_memo(subject, pos, &mut BacktickMemo::default())
}

/// [`scan_code_span`] with the search memo the inline parser keeps per
/// block, so that a subject full of unmatched runs is scanned once.
pub fn scan_code_span_memo(
    subject: &str,
    pos: usize,
    memo: &mut BacktickMemo,
) -> Option<CodeSpanScan> {
    let s = subject.as_bytes();
    let mut i = pos;
    while i < s.len() && s[i] == b'`' {
        i += 1;
    }
    if i == pos {
        return None;
    }
    let tick_len = i - pos;
    let after_open_ticks = i;

    // A full scan has been past here: a closer exists only if a run of
    // this length was recorded after the opener.
    if memo.scanned_to_end && memo.last_run.get(&tick_len).is_none_or(|&last| last <= pos) {
        return Some(CodeSpanScan::Open {
            end: after_open_ticks,
            ticks: subject[pos..after_open_ticks].to_string(),
        });
    }

    while i < s.len() {
        if s[i] != b'`' {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < s.len() && s[i] == b'`' {
            i += 1;
        }
        let run_len = i - run_start;
        memo.last_run
            .entry(run_len)
            .and_modify(|last| *last = (*last).max(run_start))
            .or_insert(run_start);
        if run_len == tick_len {
            // Line endings become spaces; then one leading and one trailing
            // space are stripped together, but only if the content is not
            // all spaces (which is what lets a code span hold backticks).
            let contents = subject[after_open_ticks..i - tick_len].replace('\n', " ");
            let has_non_space = contents.bytes().any(|b| b != b' ');
            let literal = if contents.len() > 2
                && contents.starts_with(' ')
                && contents.ends_with(' ')
                && has_non_space
            {
                contents[1..contents.len() - 1].to_string()
            } else {
                contents
            };
            return Some(CodeSpanScan::Closed { end: i, literal });
        }
    }

    memo.scanned_to_end = true;
    Some(CodeSpanScan::Open {
        end: after_open_ticks,
        ticks: subject[pos..after_open_ticks].to_string(),
    })
}

/// How a backslash escape resolves (sections 6.1 and 6.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeKind {
    Linebreak,
    Char,
    Literal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeScan {
    pub kind: EscapeKind,
    pub literal: String,
    pub end: usize,
}

/// The backslash escape with `pos` at the backslash. Always succeeds: a
/// backslash before anything unescapable is a literal backslash. The
/// linebreak form consumes the following line's leading spaces.
pub fn scan_escape(subject: &str, pos: usize) -> EscapeScan {
    let bytes = subject.as_bytes();
    let next = pos + 1;
    if next < bytes.len() && bytes[next] == b'\n' {
        return EscapeScan {
            kind: EscapeKind::Linebreak,
            literal: String::new(),
            end: skip_initial_spaces(subject, next + 1),
        };
    }
    if next < bytes.len() && is_escapable(bytes[next]) {
        return EscapeScan {
            kind: EscapeKind::Char,
            literal: subject[next..next + 1].to_string(),
            end: next + 1,
        };
    }
    EscapeScan {
        kind: EscapeKind::Literal,
        literal: "\\".to_string(),
        end: next,
    }
}

/// An email or URI autolink in angle brackets (section 6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AngleAutolinkScan {
    pub dest: String,
    pub label: String,
    pub end: usize,
}

pub fn scan_angle_autolink(subject: &str, pos: usize) -> Option<AngleAutolinkScan> {
    if let Some(m) = regex_at(re_email_autolink(), subject, pos) {
        let dest = &m[1..m.len() - 1];
        return Some(AngleAutolinkScan {
            dest: format!("mailto:{dest}"),
            label: dest.to_string(),
            end: pos + m.len(),
        });
    }
    if let Some(m) = regex_at(re_autolink(), subject, pos) {
        let dest = &m[1..m.len() - 1];
        return Some(AngleAutolinkScan {
            dest: dest.to_string(),
            label: dest.to_string(),
            end: pos + m.len(),
        });
    }
    None
}

/// A raw HTML tag (section 6.6) at `pos`: open, close, comment, PI,
/// declaration or CDATA. Returns the raw text and the end offset.
pub fn scan_html_tag(subject: &str, pos: usize) -> Option<(String, usize)> {
    regex_at(re_html_tag(), subject, pos).map(|m| (m.to_string(), pos + m.len()))
}

/// An entity or numeric character reference (section 6.2), decoded.
/// Returns the decoded text and the end offset.
pub fn scan_entity(subject: &str, pos: usize) -> Option<(String, usize)> {
    regex_at(re_entity_here(), subject, pos).map(|m| (decode_entity(m), pos + m.len()))
}

/// Classify the run of `cc` starting at `pos` without consuming it
/// (section 6.4): a run is left-flanking iff it is not followed by Unicode
/// whitespace and either not followed by Unicode punctuation or else
/// preceded by whitespace or punctuation; right-flanking is the mirror
/// image. `_` additionally may not open inside a word (rules 2, 4, 6, 8),
/// which is what keeps `snake_case_words` intact; `*` has no such
/// restriction.
///
/// The two neighbouring characters are decoded as characters, not read as
/// bytes: classify `é` or `。` by its first UTF-8 byte and most of section
/// 6.4 silently breaks on non-ASCII input.
pub fn scan_delimiter_run(subject: &str, pos: usize, cc: u8) -> Option<DelimScan> {
    let bytes = subject.as_bytes();
    let mut numdelims = 0;
    let mut p = pos;
    while p < bytes.len() && bytes[p] == cc {
        numdelims += 1;
        p += 1;
    }
    if numdelims == 0 {
        return None;
    }

    let char_before = code_point_before(subject, pos);
    let char_after = code_point_at(subject, p);

    let after_is_whitespace = is_unicode_whitespace(char_after);
    let after_is_punctuation = is_unicode_punctuation(char_after);
    let before_is_whitespace = is_unicode_whitespace(char_before);
    let before_is_punctuation = is_unicode_punctuation(char_before);

    let left_flanking = !after_is_whitespace
        && (!after_is_punctuation || before_is_whitespace || before_is_punctuation);
    let right_flanking = !before_is_whitespace
        && (!before_is_punctuation || after_is_whitespace || after_is_punctuation);

    let (can_open, can_close) = if cc == b'_' {
        (
            left_flanking && (!right_flanking || before_is_punctuation),
            right_flanking && (!left_flanking || after_is_punctuation),
        )
    } else {
        (left_flanking, right_flanking)
    };

    Some(DelimScan {
        numdelims,
        can_open,
        can_close,
    })
}

/// How a line ending resolves (sections 6.7 and 6.8), given the literal of
/// the text node immediately before it (`None` when that node is absent or
/// not text). Two or more trailing spaces make a hard break; `trim` says
/// the trailing spaces must be dropped from that node either way. Returns
/// `(hard, trim)`.
pub fn classify_break(prev_text_literal: Option<&str>) -> (bool, bool) {
    let Some(prev) = prev_text_literal else {
        return (false, false);
    };
    let bytes = prev.as_bytes();
    if bytes.last() != Some(&b' ') {
        return (false, false);
    }
    (bytes.len() > 1 && bytes[bytes.len() - 2] == b' ', true)
}

/// A link title at `pos` (section 6.3), without its delimiters, unescaped.
/// Returns the title and the end offset.
pub fn scan_link_title(subject: &str, pos: usize) -> Option<(String, usize)> {
    regex_at(re_link_title(), subject, pos)
        .map(|m| (unescape_string(&m[1..m.len() - 1]), pos + m.len()))
}

/// A link destination at `pos` (section 6.3): either `<...>`, or a bare run
/// with balanced unescaped parentheses and no ASCII control characters or
/// spaces. Returns the destination and the end offset.
///
/// The destination is backslash-unescaped and entity-decoded, but **not**
/// percent-encoded: `[l](/ä)` yields `/ä`, not `/%C3%A4`. Encoding is a
/// rendering concern and `html.rs` applies `normalize_uri` when it writes
/// the attribute; the public AST promises the decoded form.
pub fn scan_link_destination(subject: &str, pos: usize) -> Option<(String, usize)> {
    if let Some(braced) = regex_at(re_link_destination_braces(), subject, pos) {
        return Some((
            unescape_string(&braced[1..braced.len() - 1]),
            pos + braced.len(),
        ));
    }
    let bytes = subject.as_bytes();
    if pos < bytes.len() && bytes[pos] == b'<' {
        return None;
    }

    let mut p = pos;
    let mut openparens = 0usize;

    while p < bytes.len() {
        let c = bytes[p];
        if c == b'\\' && p + 1 < bytes.len() && is_escapable(bytes[p + 1]) {
            p += 1;
            if p < bytes.len() {
                p += 1;
            }
        } else if c == b'(' {
            // Bail rather than scan on once nesting passes the limit.
            // Without this, input like `[a](b` repeated makes every `]`
            // scan to end of input looking for parens that never balance,
            // which is quadratic in the document size. cmark caps nesting
            // at the same depth, and no spec example nests more than two
            // deep.
            if openparens >= MAX_LINK_PAREN_NESTING {
                return None;
            }
            p += 1;
            openparens += 1;
        } else if c == b')' {
            if openparens < 1 {
                break;
            }
            p += 1;
            openparens -= 1;
        } else if c <= b' ' || c == 0x7F {
            // Spaces and ASCII control characters end the bare form.
            break;
        } else {
            p += 1;
        }
    }

    // An empty destination is only legal immediately before the closing
    // paren of an inline link, as in `[link]()`.
    if p == pos && !(p < bytes.len() && bytes[p] == b')') {
        return None;
    }
    if openparens != 0 {
        return None;
    }

    Some((unescape_string(&subject[pos..p]), p))
}

/// A link label at `pos` (sections 4.7 and 6.3), brackets included.
/// Returns `(raw_length, length)`: `raw_length` is what was consumed, and
/// `length` is 0 when the label is too long to be valid (at most 999
/// characters between the brackets). `None` when no label is here at all.
///
/// Hand-coded where the TypeScript uses `/\[(?:[^\\[\]]|\\.){0,1000}\]/sy`.
/// The scan accepts the same language: every alternative of the repeat
/// begins on a character that is not `]`, so shortening the greedy run can
/// only put a non-`]` where the closing bracket must be.
pub fn scan_link_label(subject: &str, pos: usize) -> Option<(usize, usize)> {
    let s = subject.as_bytes();
    if pos >= s.len() || s[pos] != b'[' {
        return None;
    }

    let mut i = pos + 1;
    let mut iters = 0;
    // Length in UTF-16 code units, which is the unit the TypeScript's
    // `m.length > 1001` test counts in, hence the 2 for an astral character
    // below.
    let mut units = 1;

    loop {
        if i >= s.len() {
            return None;
        }
        let c = s[i];
        if c == b']' {
            i += 1;
            units += 1;
            let raw = i - pos;
            // Section 4.7: at most 999 characters between the brackets.
            if units > 1001 {
                return Some((raw, 0));
            }
            return Some((raw, raw));
        }
        // An unescaped `[` cannot be consumed by the repeat, and is not the
        // `]` the pattern then demands. Nor can a 1001st repeat run.
        if c == b'[' || iters == 1000 {
            return None;
        }
        if c == b'\\' {
            // `\\.`: with the `s` flag, any character at all, line endings
            // included.
            i += 1;
            units += 1;
            if i >= s.len() {
                return None;
            }
        }
        let ch = code_point_at(subject, i);
        i += ch.len_utf8();
        units += ch.len_utf16();
        iters += 1;
    }
}

/// The `(destination "title")` tail of an inline link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTailScan {
    pub dest: String,
    pub title: Option<String>,
    pub end: usize,
}

/// The inline-link tail with `pos` just after the `]`. `None` unless the
/// whole tail, through the closing paren, is present.
pub fn scan_inline_link_tail(subject: &str, pos: usize) -> Option<LinkTailScan> {
    let bytes = subject.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'(' {
        return None;
    }

    let mut p = match_spnl(subject, pos + 1);
    let (dest, d_end) = scan_link_destination(subject, p)?;

    p = match_spnl(subject, d_end);
    let mut title = None;
    // A title has to be separated from the destination by whitespace, so
    // `[a](/url"t")` is not a titled link.
    if p >= 1 && p - 1 < bytes.len() && is_inline_whitespace_byte(bytes[p - 1]) {
        if let Some((t, t_end)) = scan_link_title(subject, p) {
            title = Some(t);
            p = t_end;
        }
    }
    p = match_spnl(subject, p);

    if p < bytes.len() && bytes[p] == b')' {
        return Some(LinkTailScan {
            dest,
            title,
            end: p + 1,
        });
    }
    None
}

/// The inline scanner: the subject, the scan position, and the two
/// auxiliary stacks.
pub struct InlineParser {
    pub subject: String,
    pub pos: usize,
    delims: Vec<Delimiter>,
    /// Top of the delimiter stack, as an index into `delims`.
    delim_top: Option<usize>,
    brackets: Vec<Bracket>,
    pub refmap: RefMap,
    pub options: Options,
    /// The nesting height of every inline container made for the current
    /// block: 1 for one wrapping only leaves, and one more per level.
    /// Leaves are absent and count 0. `MAX_INLINE_NESTING` is judged from
    /// it; see `run_height`.
    nesting: HashMap<NodeId, usize>,
    /// The code span search memo for the current block's subject.
    backticks: BacktickMemo,
}

impl InlineParser {
    pub fn new(refmap: RefMap, options: Options) -> Self {
        InlineParser {
            subject: String::new(),
            pos: 0,
            delims: Vec::new(),
            delim_top: None,
            brackets: Vec::new(),
            refmap,
            options,
            nesting: HashMap::new(),
            backticks: BacktickMemo::default(),
        }
    }

    /// The tallest nesting height among the siblings from `from` up to
    /// but not including `until` (or to the end of the run). Stops at
    /// `MAX_INLINE_NESTING`, the only threshold the callers compare
    /// against, so the walk never costs more than it decides.
    fn run_height(&self, tree: &Tree, from: Option<NodeId>, until: Option<NodeId>) -> usize {
        let mut height = 0;
        let mut cur = from;
        while let Some(id) = cur {
            if cur == until || height >= MAX_INLINE_NESTING {
                break;
            }
            height = height.max(self.nesting.get(&id).copied().unwrap_or(0));
            cur = tree.get(id).next;
        }
        height
    }

    // --- scanning primitives ---

    /// The byte at `pos`, or `None` at the end of the subject.
    fn peek(&self) -> Option<u8> {
        self.subject.as_bytes().get(self.pos).copied()
    }

    /// Spaces, and at most one line ending, between the parts of a link.
    fn spnl(&mut self) {
        self.pos = match_spnl(&self.subject, self.pos);
    }

    fn skip_spaces(&mut self) {
        self.pos = skip_initial_spaces(&self.subject, self.pos);
    }

    /// The TypeScript's `reIndent`, `/ {0,3}/y`.
    fn skip_indent(&mut self) {
        let bytes = self.subject.as_bytes();
        let mut n = 0;
        while n < 3 && self.pos < bytes.len() && bytes[self.pos] == b' ' {
            self.pos += 1;
            n += 1;
        }
    }

    /// The TypeScript's `reSpaceAtEndOfLine`, `/ *(?:\n|$)/y`. On failure
    /// `pos` is left alone, as a failed sticky match would leave it.
    fn space_at_end_of_line(&mut self) -> bool {
        let bytes = self.subject.as_bytes();
        let mut i = self.pos;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i < bytes.len() {
            if bytes[i] != b'\n' {
                return false;
            }
            i += 1;
        }
        self.pos = i;
        true
    }

    // --- section 6.3 code spans ---

    /// A code span runs from a backtick string to the next backtick string
    /// of *exactly* the same length. Unmatched opening backticks are
    /// literal text.
    pub fn parse_backticks(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let Some(scan) = scan_code_span_memo(&self.subject, self.pos, &mut self.backticks) else {
            return false;
        };
        match scan {
            CodeSpanScan::Closed { end, literal } => {
                self.pos = end;
                let mut node = MdNode::new(NodeType::Code);
                node.literal = literal;
                let id = tree.add(node);
                tree.append_child(block, id);
            }
            CodeSpanScan::Open { end, ticks } => {
                // No closing run of the same length: the opener is literal
                // text.
                self.pos = end;
                let id = text_node(tree, &ticks);
                tree.append_child(block, id);
            }
        }
        true
    }

    // --- section 6.1 backslash escapes, section 6.7 hard line breaks ---

    pub fn parse_backslash(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let scan = scan_escape(&self.subject, self.pos);
        self.pos = scan.end;
        let id = if scan.kind == EscapeKind::Linebreak {
            tree.new_node(NodeType::Linebreak)
        } else {
            text_node(tree, &scan.literal)
        };
        tree.append_child(block, id);
        true
    }

    // --- section 6.5 autolinks ---

    pub fn parse_autolink(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let Some(scan) = scan_angle_autolink(&self.subject, self.pos) else {
            return false;
        };
        self.pos = scan.end;
        let id = make_autolink(tree, &scan.dest, &scan.label);
        tree.append_child(block, id);
        true
    }

    // --- section 6.6 raw HTML ---

    pub fn parse_html_tag(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let Some((raw, end)) = scan_html_tag(&self.subject, self.pos) else {
            return false;
        };
        self.pos = end;
        let mut node = MdNode::new(NodeType::HtmlInline);
        node.literal = raw;
        let id = tree.add(node);
        tree.append_child(block, id);
        true
    }

    // --- section 6.4 emphasis: delimiter runs and flanking ---

    /// Classify the run of `cc` starting at `pos` without consuming it.
    pub fn scan_delims(&self, cc: u8) -> Option<DelimScan> {
        scan_delimiter_run(&self.subject, self.pos, cc)
    }

    /// Emit a delimiter run as text and push it on the delimiter stack.
    pub fn handle_delim(&mut self, cc: u8, tree: &mut Tree, block: NodeId) -> bool {
        let Some(res) = self.scan_delims(cc) else {
            return false;
        };

        let numdelims = res.numdelims;
        let startpos = self.pos;
        self.pos += numdelims;

        let node = text_node(tree, &self.subject[startpos..self.pos]);
        tree.append_child(block, node);

        // GFM strikethrough only recognises runs of one or two tildes;
        // longer runs stay literal.
        let stackable = if cc == b'~' { numdelims <= 2 } else { true };

        if (res.can_open || res.can_close) && stackable {
            let index = self.delims.len();
            self.delims.push(Delimiter {
                cc,
                numdelims,
                origdelims: numdelims,
                node,
                previous: self.delim_top,
                next: None,
                can_open: res.can_open,
                can_close: res.can_close,
            });
            if let Some(prev) = self.delim_top {
                self.delims[prev].next = Some(index);
            }
            self.delim_top = Some(index);
        }

        true
    }

    fn remove_delimiter(&mut self, delim: usize) {
        let previous = self.delims[delim].previous;
        let next = self.delims[delim].next;
        if let Some(prev) = previous {
            self.delims[prev].next = next;
        }
        match next {
            // Top of stack.
            None => self.delim_top = previous,
            Some(n) => self.delims[n].previous = previous,
        }
    }

    /// The spec's "process emphasis" procedure. `stack_bottom` is the
    /// floor: for the whole-block call it is `None`, and for the inlines of
    /// a link it is the delimiter that was on top when the link's `[`
    /// opened.
    pub fn process_emphasis(&mut self, tree: &mut Tree, stack_bottom: Option<usize>) {
        // openers_bottom, keyed by (delimiter char, whether the closer can
        // also open, closer run length mod 3). Once a closer of some class
        // fails to find an opener, no later closer of the same class needs
        // to look past that point either; this is what keeps the search
        // linear.
        let mut openers_bottom: HashMap<(u8, bool, usize), Option<usize>> = HashMap::new();

        // First closer above stack_bottom.
        let mut closer = self.delim_top;
        while let Some(c) = closer {
            if self.delims[c].previous == stack_bottom {
                break;
            }
            closer = self.delims[c].previous;
        }

        while let Some(c) = closer {
            if !self.delims[c].can_close {
                closer = self.delims[c].next;
                continue;
            }

            let closercc = self.delims[c].cc;
            let key = (
                closercc,
                self.delims[c].can_open,
                self.delims[c].origdelims % 3,
            );
            let bottom = openers_bottom.get(&key).copied().unwrap_or(stack_bottom);

            let mut opener = self.delims[c].previous;
            let mut opener_found = false;
            while let Some(o) = opener {
                if opener == stack_bottom || opener == bottom {
                    break;
                }
                let od = &self.delims[o];
                if od.cc == closercc && od.can_open {
                    if closercc == b'~' {
                        // GFM strikethrough: opener and closer runs must be
                        // the same length, so `~~x~` is not a match.
                        if od.numdelims == self.delims[c].numdelims {
                            opener_found = true;
                            break;
                        }
                    } else {
                        // Section 6.4 rules 9 and 10, the "rule of three":
                        // when either side of the pair could both open and
                        // close, the two run lengths may not sum to a
                        // multiple of 3, unless both are themselves
                        // multiples of 3.
                        let cd = &self.delims[c];
                        let odd_match = (cd.can_open || od.can_close)
                            && cd.origdelims % 3 != 0
                            && (od.origdelims + cd.origdelims) % 3 == 0;
                        if !odd_match {
                            opener_found = true;
                            break;
                        }
                    }
                }
                opener = od.previous;
            }

            // The nesting limit: a pair whose content already holds
            // `MAX_INLINE_NESTING` nested containers stays literal, which
            // is the closer finding no opener.
            let mut height = 0;
            if let Some(o) = opener.filter(|_| opener_found) {
                height = self.run_height(
                    tree,
                    tree.get(self.delims[o].node).next,
                    Some(self.delims[c].node),
                );
                if height >= MAX_INLINE_NESTING {
                    opener_found = false;
                }
            }

            let old_closer = c;

            match opener {
                Some(o) if opener_found => {
                    // Two delimiters make strong, one makes emph;
                    // strikethrough always consumes the whole (equal-length)
                    // run.
                    let use_delims = if closercc == b'~' {
                        self.delims[c].numdelims
                    } else if self.delims[c].numdelims >= 2 && self.delims[o].numdelims >= 2 {
                        2
                    } else {
                        1
                    };
                    let node_type = if closercc == b'~' {
                        NodeType::Del
                    } else if use_delims == 1 {
                        NodeType::Emph
                    } else {
                        NodeType::Strong
                    };

                    let opener_inl = self.delims[o].node;
                    let closer_inl = self.delims[c].node;

                    self.delims[o].numdelims -= use_delims;
                    self.delims[c].numdelims -= use_delims;
                    trim_delim_tail(&mut tree.get_mut(opener_inl).literal, use_delims);
                    trim_delim_tail(&mut tree.get_mut(closer_inl).literal, use_delims);

                    // Everything between the two delimiter text nodes
                    // becomes the content of the new node.
                    let emph = tree.new_node(node_type);
                    let mut tmp = tree.get(opener_inl).next;
                    while let Some(t) = tmp {
                        if t == closer_inl {
                            break;
                        }
                        let nxt = tree.get(t).next;
                        tree.unlink(t);
                        tree.append_child(emph, t);
                        tmp = nxt;
                    }
                    tree.insert_after(opener_inl, emph);
                    self.nesting.insert(emph, height + 1);

                    // Delimiters between the pair can never match anything
                    // now.
                    if self.delims[o].next != Some(c) {
                        self.delims[o].next = Some(c);
                        self.delims[c].previous = Some(o);
                    }

                    if self.delims[o].numdelims == 0 {
                        tree.unlink(opener_inl);
                        self.remove_delimiter(o);
                    }
                    if self.delims[c].numdelims == 0 {
                        tree.unlink(closer_inl);
                        let tempstack = self.delims[c].next;
                        self.remove_delimiter(c);
                        closer = tempstack;
                    }
                }
                _ => {
                    closer = self.delims[c].next;
                    openers_bottom.insert(key, self.delims[old_closer].previous);
                    if !self.delims[old_closer].can_open {
                        // It found no opener and cannot be one: it is inert.
                        self.remove_delimiter(old_closer);
                    }
                }
            }
        }

        // Drop everything above the floor.
        while let Some(top) = self.delim_top {
            if Some(top) == stack_bottom {
                break;
            }
            self.remove_delimiter(top);
        }
    }

    // --- section 6.3 links and images ---

    /// The link title without its delimiters, unescaped; `None` when there
    /// is no title here.
    pub fn parse_link_title(&mut self) -> Option<String> {
        let (title, end) = scan_link_title(&self.subject, self.pos)?;
        self.pos = end;
        Some(title)
    }

    /// A destination that is backslash-unescaped and entity-decoded, but
    /// **not** percent-encoded; see `scan_link_destination`.
    pub fn parse_link_destination(&mut self) -> Option<String> {
        let (dest, end) = scan_link_destination(&self.subject, self.pos)?;
        self.pos = end;
        Some(dest)
    }

    /// The byte length of the link label at `pos`, brackets included,
    /// advancing past it; 0 if there is no label here or it is over the
    /// section 4.7 999-character limit.
    pub fn parse_link_label(&mut self) -> usize {
        let Some((raw_len, length)) = scan_link_label(&self.subject, self.pos) else {
            return 0;
        };
        // The raw match is consumed even when it is too long to be a valid
        // label; callers that need the position back reset it themselves.
        self.pos += raw_len;
        length
    }

    fn add_bracket(&mut self, node: NodeId, index: usize, image: bool) {
        if let Some(top) = self.brackets.last_mut() {
            top.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            previous_delimiter: self.delim_top,
            index,
            image,
            active: true,
            bracket_after: false,
        });
    }

    fn remove_bracket(&mut self) {
        self.brackets.pop();
    }

    pub fn parse_open_bracket(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let startpos = self.pos;
        self.pos += 1;
        let node = text_node(tree, "[");
        tree.append_child(block, node);
        self.add_bracket(node, startpos, false);
        true
    }

    /// `!` only matters when it introduces an image.
    pub fn parse_bang(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let startpos = self.pos;
        self.pos += 1;
        if self.peek() == Some(b'[') {
            self.pos += 1;
            let node = text_node(tree, "![");
            tree.append_child(block, node);
            self.add_bracket(node, startpos + 1, true);
        } else {
            let node = text_node(tree, "!");
            tree.append_child(block, node);
        }
        true
    }

    /// The spec's "look for link or image" procedure. On a `]`, walk back
    /// to the newest bracket opener and try, in order: an inline
    /// `(dest "title")`, a full `[label]`, a collapsed `[]` or a shortcut
    /// reference. Anything that fails backtracks to just after the `]` and
    /// leaves a literal `]`.
    pub fn parse_close_bracket(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        self.pos += 1;
        let startpos = self.pos;

        let Some(opener) = self.brackets.last().cloned() else {
            let node = text_node(tree, "]");
            tree.append_child(block, node);
            return true;
        };
        if !opener.active {
            // Deactivated by an enclosing link: not an opener any more.
            let node = text_node(tree, "]");
            tree.append_child(block, node);
            self.remove_bracket();
            return true;
        }

        let is_image = opener.image;
        let mut dest = String::new();
        let mut title: Option<String> = None;
        let mut matched = false;

        let savepos = self.pos;

        // Inline link: `](` destination title `)`.
        if let Some(tail) = scan_inline_link_tail(&self.subject, self.pos) {
            self.pos = tail.end;
            dest = tail.dest;
            title = tail.title;
            matched = true;
        }

        if !matched {
            // Reference forms. A second label gives the full form; an empty
            // or absent one means the link text is itself the label
            // (collapsed and shortcut), which is only possible if that text
            // holds no bracket.
            let mut reflabel: Option<String> = None;
            let beforelabel = self.pos;
            let n = self.parse_link_label();
            if n > 2 {
                reflabel = Some(self.subject[beforelabel + 1..beforelabel + n - 1].to_string());
            } else if !opener.bracket_after {
                let (lo, hi) = (opener.index + 1, startpos - 1);
                if lo <= hi && hi <= self.subject.len() {
                    reflabel = Some(self.subject[lo..hi].to_string());
                }
            }
            if n == 0 {
                self.pos = savepos;
            }

            if let Some(label) = reflabel {
                if let Some(link) = self.refmap.get(&normalize_reference(&label)) {
                    dest = link.destination.clone();
                    title = link.title.clone();
                    matched = true;
                }
            }
        }

        if !matched {
            self.remove_bracket();
            self.pos = startpos;
            let node = text_node(tree, "]");
            tree.append_child(block, node);
            return true;
        }

        let mut node = MdNode::new(if is_image {
            NodeType::Image
        } else {
            NodeType::Link
        });
        node.destination = dest;
        // An empty title is the same as no title at all: `[a](/url "")`
        // renders a bare `<a href="/url">`.
        node.title = title.filter(|t| !t.is_empty());
        let node = tree.add(node);

        let mut tmp = tree.get(opener.node).next;
        while let Some(t) = tmp {
            let nxt = tree.get(t).next;
            tree.unlink(t);
            tree.append_child(node, t);
            tmp = nxt;
        }
        tree.append_child(block, node);

        // Emphasis inside the link text resolves now, and only down to the
        // delimiter that was current when the `[` opened.
        self.process_emphasis(tree, opener.previous_delimiter);

        // The nesting limit, judged once the content's own nesting is
        // settled: a link or image whose text already holds
        // `MAX_INLINE_NESTING` nested containers stays literal. Its
        // children go back where they were, the `[` stays the text it
        // still is, and the `]` becomes text, as for any other failed
        // match; the tail after `]` is rescanned as text too.
        let height = self.run_height(tree, tree.get(node).first_child, None);
        if height >= MAX_INLINE_NESTING {
            tree.unlink(node);
            let mut child = tree.get(node).first_child;
            while let Some(c) = child {
                let nxt = tree.get(c).next;
                tree.unlink(c);
                tree.append_child(block, c);
                child = nxt;
            }
            self.remove_bracket();
            self.pos = startpos;
            let text = text_node(tree, "]");
            tree.append_child(block, text);
            return true;
        }
        self.nesting.insert(node, height + 1);

        self.remove_bracket();
        tree.unlink(opener.node);

        // Links may not contain links (section 6.3): every earlier link
        // opener is spent. Image openers survive, since images may nest in
        // links.
        if !is_image {
            for b in self.brackets.iter_mut() {
                if !b.image {
                    b.active = false;
                }
            }
        }

        true
    }

    // --- section 6.2 entities, sections 6.8 and 6.9 breaks and plain text ---

    pub fn parse_entity(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let Some((literal, end)) = scan_entity(&self.subject, self.pos) else {
            return false;
        };
        self.pos = end;
        let node = text_node(tree, &literal);
        tree.append_child(block, node);
        true
    }

    /// A chunk of ordinary text: everything up to the next character that
    /// could start an inline construct. A byte scan is exact, because the
    /// terminators are all ASCII.
    pub fn parse_string(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let bytes = self.subject.as_bytes();
        let startpos = self.pos;
        let gfm = self.options.gfm;
        while self.pos < bytes.len() && !is_inline_special_byte(bytes[self.pos], gfm) {
            self.pos += 1;
        }
        if self.pos == startpos {
            return false;
        }
        let node = text_node(tree, &self.subject[startpos..self.pos]);
        tree.append_child(block, node);
        true
    }

    /// Sections 6.7 and 6.8: two or more spaces before the line ending make
    /// a hard break, anything else a soft one. Trailing spaces are dropped
    /// either way, as is the indentation of the next line.
    pub fn parse_newline(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        self.pos += 1;
        let lastc = tree.get(block).last_child;
        let prev_text = lastc.and_then(|id| {
            let node = tree.get(id);
            (node.node_type == NodeType::Text).then_some(node.literal.as_str())
        });
        let (hard, trim) = classify_break(prev_text);
        if trim {
            if let Some(id) = lastc {
                let literal = &mut tree.get_mut(id).literal;
                let trimmed = literal.trim_end_matches(' ').len();
                literal.truncate(trimmed);
            }
            let brk = tree.new_node(if hard {
                NodeType::Linebreak
            } else {
                NodeType::Softbreak
            });
            tree.append_child(block, brk);
        } else {
            let brk = tree.new_node(NodeType::Softbreak);
            tree.append_child(block, brk);
        }
        self.skip_spaces();
        true
    }

    /// One inline construct at `pos`; false only at the end of the subject.
    pub fn parse_inline(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        let Some(c) = self.peek() else {
            return false;
        };

        let res = match c {
            b'\n' => self.parse_newline(tree, block),
            b'\\' => self.parse_backslash(tree, block),
            b'`' => self.parse_backticks(tree, block),
            b'*' | b'_' => self.handle_delim(c, tree, block),
            // Only GFM gives `~` a meaning; otherwise it is ordinary text
            // (and the non-GFM text scan swallows whole runs of it).
            b'~' => {
                if self.options.gfm {
                    self.handle_delim(c, tree, block)
                } else {
                    self.parse_string(tree, block)
                }
            }
            b'[' => self.parse_open_bracket(tree, block),
            b'!' => self.parse_bang(tree, block),
            b']' => self.parse_close_bracket(tree, block),
            b'<' => self.parse_autolink(tree, block) || self.parse_html_tag(tree, block),
            b'&' => self.parse_entity(tree, block),
            _ => self.parse_string(tree, block),
        };

        if !res {
            self.parse_literal_char(tree, block);
        }

        true
    }

    /// A character nothing claimed: consume it as literal text. Only `<`
    /// and `&` reach here from the dispatch, so the byte at `pos` is a
    /// whole character; the engine driver's literal fallback may reach it
    /// with any character, so the whole character is taken either way.
    pub fn parse_literal_char(&mut self, tree: &mut Tree, block: NodeId) -> bool {
        if self.pos >= self.subject.len() {
            return false;
        }
        let startpos = self.pos;
        self.pos += code_point_at(&self.subject, self.pos).len_utf8();
        let node = text_node(tree, &self.subject[startpos..self.pos]);
        tree.append_child(block, node);
        true
    }

    /// Reset for one block's raw content; false when the trimmed subject
    /// is empty. Trimming is what stops trailing spaces at the end of a
    /// block from making a hard break, and drops the leading indentation
    /// of the first line. The trim must remove exactly what
    /// `String.prototype.trim` removes, which is `js_trim`.
    pub fn begin_block(&mut self, tree: &Tree, block: NodeId) -> bool {
        self.subject = js_trim(&tree.get(block).string_content).to_string();
        self.pos = 0;
        self.delims.clear();
        self.delim_top = None;
        self.brackets.clear();
        self.nesting.clear();
        self.backticks = BacktickMemo::default();
        !self.subject.is_empty()
    }

    /// The tail of a block's inline parse: clear the raw content, resolve
    /// emphasis over the whole delimiter stack.
    pub fn finish_block(&mut self, tree: &mut Tree, block: NodeId) {
        tree.get_mut(block).string_content.clear();
        self.process_emphasis(tree, None);
    }

    /// Parse one paragraph's or heading's raw content into inline children.
    pub fn parse(&mut self, tree: &mut Tree, block: NodeId) {
        self.begin_block(tree, block);
        while self.parse_inline(tree, block) {}
        self.finish_block(tree, block);
    }

    /// Section 4.7: consume one link reference definition at the start
    /// of `s`, adding it to `refmap`. Returns the number of BYTES consumed.
    /// The block phase goes through [`parse_references`], which takes
    /// every leading definition of a paragraph over one copy of it.
    pub fn parse_reference(&mut self, s: &str, refmap: &mut RefMap) -> usize {
        self.subject = s.to_string();
        self.pos = 0;
        self.parse_reference_here(refmap)
    }

    /// [`parse_reference`] at the current position of the current
    /// subject: on success the position is past the definition, on
    /// failure it is back where it was and the result is 0.
    fn parse_reference_here(&mut self, refmap: &mut RefMap) -> usize {
        let startpos = self.pos;

        // Section 4.7 allows up to three spaces of indentation before the
        // label. The block phase normally strips a paragraph line's
        // indentation before it gets here, so this usually consumes
        // nothing.
        self.skip_indent();

        let label_start = self.pos;
        let match_chars = self.parse_link_label();
        if match_chars == 0 {
            return 0;
        }
        let rawlabel = self.subject[label_start + 1..label_start + match_chars - 1].to_string();

        if self.peek() == Some(b':') {
            self.pos += 1;
        } else {
            self.pos = startpos;
            return 0;
        }

        self.spnl();

        let Some(dest) = self.parse_link_destination() else {
            self.pos = startpos;
            return 0;
        };

        let beforetitle = self.pos;
        self.spnl();
        let mut title = None;
        if self.pos != beforetitle {
            title = self.parse_link_title();
        }
        if title.is_none() {
            self.pos = beforetitle;
        }

        // Nothing but spaces may follow on the definition's last line.
        let mut at_line_end = true;
        if !self.space_at_end_of_line() {
            if title.is_none() {
                at_line_end = false;
            } else {
                // What looked like a title is something else; the definition
                // may still be valid without it.
                title = None;
                self.pos = beforetitle;
                at_line_end = self.space_at_end_of_line();
            }
        }
        if !at_line_end {
            self.pos = startpos;
            return 0;
        }

        let normlabel = normalize_reference(&rawlabel);
        if normlabel.is_empty() {
            // A label must hold at least one non-whitespace character.
            self.pos = startpos;
            return 0;
        }

        // First definition wins.
        refmap.entry(normlabel).or_insert(RefDef {
            destination: dest,
            title,
        });

        self.pos - startpos
    }
}

/// Autolink text is literal: no backslash escapes, no entity references.
/// The destination is not percent-encoded here; see
/// `scan_link_destination`.
fn make_autolink(tree: &mut Tree, destination: &str, label: &str) -> NodeId {
    let mut node = MdNode::new(NodeType::Link);
    node.destination = destination.to_string();
    node.title = None;
    let id = tree.add(node);
    let text = text_node(tree, label);
    tree.append_child(id, text);
    id
}

// --- GFM extended autolinks (autolink literals) -----------------------------
//
// GFM recognises `www.x.com`, `https://x.com` and `a@x.com` without the
// `<...>` CommonMark requires. This runs as a post-pass over the finished
// inline tree rather than as another branch of the scan above, which is
// how cmark-gfm does it and is the only shape that keeps 652/652 safe: the
// scanner that decides code spans, raw HTML, emphasis and links is not
// touched at all, and an autolink can therefore never win against a
// construct CommonMark says comes first.
//
// The pass rewrites text nodes into text / link / text runs. It does not
// descend into link (links may not contain links) or image (its children
// are the alt text), and code / html_inline are leaves it never looks
// inside.
//
// Everything here scans by BYTE, like the rest of this file. Every
// character the pass branches on is ASCII, and no byte of a multi-byte
// UTF-8 sequence can collide with one, so a non-ASCII character is exactly
// as unmatchable here as its UTF-16 code unit is in the TypeScript: it ends
// a domain run, it is not a valid autolink start, and it is not trailing
// punctuation. Slices therefore never cut a character in half: every
// boundary this pass produces sits at an ASCII character.

/// RFC 1035's limit on a fully-qualified domain name. See
/// `scan_domain_end`.
const MAX_AUTOLINK_DOMAIN: usize = 253;

/// RFC 5321's limit on the local part of an address, before the `@`.
const MAX_EMAIL_LOCAL: usize = 64;

/// The three schemes the extension recognises without angle brackets.
const AUTOLINK_SCHEMES: [&str; 3] = ["https://", "http://", "ftp://"];

fn is_ascii_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_autolink_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')
}

/// A domain segment holds alphanumerics, `_` and `-`; `.` separates them.
fn is_domain_char(c: u8) -> bool {
    is_ascii_alnum(c) || c == b'_' || c == b'-' || c == b'.'
}

/// Before the `@`: alphanumerics and `.`, `-`, `_`, `+`.
fn is_email_local_char(c: u8) -> bool {
    is_ascii_alnum(c) || c == b'.' || c == b'-' || c == b'_' || c == b'+'
}

/// "All such recognized autolinks can only come at the beginning of a line,
/// after whitespace, or any of the delimiting characters `*`, `_`, `~`,
/// `(`."
///
/// Offset 0 has no preceding character in `s` at all, so the caller
/// answers for it through `at_start`; see `starts_after_delimiter`.
fn is_autolink_start(s: &[u8], i: usize, at_start: bool) -> bool {
    if i == 0 {
        return at_start;
    }
    let c = s[i - 1];
    is_autolink_space(c) || c == b'*' || c == b'_' || c == b'~' || c == b'('
}

/// Whether offset 0 of a text node may begin an autolink, given the sibling
/// in front of it.
///
/// The rule above is about the source character immediately before the
/// match, and by the time this pass runs the tree no longer carries source
/// offsets, but the previous sibling's *type* names that character
/// exactly:
///
/// - `emph` and `strong` end on the `*` or `_` that closed them, and `del`
///   on its second `~`. All three are delimiters the rule allows, so
///   `*a*www.b.com` links.
/// - `code` ends on a backtick, `link` and `image` on `)`, `]` or `>`, and
///   `html_inline` on `>`. None are delimiters, so `` `x`www.a.com ``,
///   `[l](/u)www.a.com` and `<b>www.a.com` do not link.
/// - a `softbreak` or `linebreak` puts offset 0 at the beginning of a line,
///   and so does having no previous sibling: that is the first inline of
///   the block, or of the emphasis span this pass descended into.
///
/// A `text` node never appears here: `linkify_children` merges every run
/// of adjacent text siblings before handing the first of them over.
fn starts_after_delimiter(tree: &Tree, prev: Option<NodeId>) -> bool {
    match prev {
        None => true,
        Some(id) => matches!(
            tree.get(id).node_type,
            NodeType::Softbreak
                | NodeType::Linebreak
                | NodeType::Emph
                | NodeType::Strong
                | NodeType::Del
        ),
    }
}

/// ASCII case-insensitive comparison against an already-lowercase literal.
fn matches_ignore_case(s: &[u8], at: usize, lower: &str) -> bool {
    let lower = lower.as_bytes();
    if at + lower.len() > s.len() {
        return false;
    }
    lower
        .iter()
        .enumerate()
        .all(|(k, &l)| s[at + k].to_ascii_lowercase() == l)
}

/// The end of the run of domain characters at `from`, capped at
/// `MAX_AUTOLINK_DOMAIN`.
///
/// The cap is what keeps this pass linear. `_` both continues a domain and
/// introduces a valid autolink start, so without a bound an input like
/// `"_www.a_b.c".repeat(n)` gives every underscore a candidate whose domain
/// run reaches the end of the text: quadratic. Anything past the cap is
/// still scanned, just as the path rather than as the domain, and no real
/// domain comes near it.
fn scan_domain_end(s: &[u8], from: usize) -> usize {
    let max = s.len().min(from + MAX_AUTOLINK_DOMAIN);
    let mut i = from;
    while i < max && is_domain_char(s[i]) {
        i += 1;
    }
    i
}

/// A valid domain: at least one period, and no underscore in either of the
/// last two segments. `from..to` must already be a run of domain
/// characters.
fn is_valid_domain(s: &[u8], from: usize, to: usize) -> bool {
    if to <= from {
        return false;
    }
    let mut periods = 0;
    let mut underscores_in_last = 0;
    let mut underscores_in_prev = 0;
    for &c in &s[from..to] {
        if c == b'_' {
            underscores_in_last += 1;
        } else if c == b'.' {
            underscores_in_prev = underscores_in_last;
            underscores_in_last = 0;
            periods += 1;
        }
    }
    periods > 0 && underscores_in_last == 0 && underscores_in_prev == 0
}

/// "Extended autolink path validation": pull the end of the match back off
/// trailing characters that read as sentence punctuation rather than as
/// part of a URL. One loop rather than three passes, because dropping a
/// `)` can expose a `.` and vice versa.
///
/// - `?` `!` `.` `,` `:` `*` `_` `~` are dropped wherever they trail;
/// - a trailing `)` is dropped only while the match holds more `)` than
///   `(`, so `(www.a.com/x_(y))` keeps its inner pair and loses the outer
///   one;
/// - a trailing `;` preceded by something shaped like an entity reference
///   takes the whole `&...;` with it.
///
/// The parenthesis counts are taken once and then maintained, not
/// recomputed per drop: a deeply parenthesised URL is otherwise quadratic
/// in its own length.
fn trim_autolink_end(s: &[u8], start: usize, mut end: usize) -> usize {
    let mut opening: Option<usize> = None;
    let mut closing = 0usize;

    while start < end {
        let c = s[end - 1];

        match c {
            b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' => {
                end -= 1;
                continue;
            }

            b')' => {
                if opening.is_none() {
                    let mut o = 0;
                    closing = 0;
                    for &d in &s[start..end] {
                        if d == b'(' {
                            o += 1;
                        } else if d == b')' {
                            closing += 1;
                        }
                    }
                    opening = Some(o);
                }
                if closing <= opening.unwrap_or(0) {
                    return end;
                }
                closing -= 1;
                end -= 1;
                continue;
            }

            b';' => {
                // `&` followed by one or more alphanumerics, then this `;`.
                let mut j = end as isize - 2;
                while j > start as isize && is_ascii_alnum(s[j as usize]) {
                    j -= 1;
                }
                if j < end as isize - 2 && j >= 0 && s[j as usize] == b'&' {
                    end = j as usize;
                    continue;
                }
                return end;
            }

            _ => return end,
        }
    }

    end
}

/// One recognised autolink literal: `s[start..end]` linking to `href`.
struct AutolinkMatch {
    start: usize,
    end: usize,
    href: String,
}

/// `www.`, `http://`, `https://` and `ftp://` all start with one of these.
fn could_start_url(c: u8) -> bool {
    matches!(c, b'w' | b'W' | b'h' | b'H' | b'f' | b'F')
}

fn has_period(s: &[u8], from: usize, to: usize) -> bool {
    s[from..to].contains(&b'.')
}

/// Every autolink literal in one text run, left to right and
/// non-overlapping. Returns an empty vector when there is nothing to do,
/// which is the overwhelmingly common case.
fn scan_autolinks(text: &str, at_start: bool) -> Vec<AutolinkMatch> {
    let s = text.as_bytes();
    let length = s.len();
    let mut out = Vec::new();

    // The maximal run of non-space, non-`<` characters containing the
    // current position: "zero or more non-space non-`<` characters may
    // follow" the domain. Cached because candidates are tried left to
    // right, so each run is scanned once however many candidates start
    // inside it.
    let mut run_start = usize::MAX;
    let mut run_end = 0usize;

    let mut i = 0;
    let mut last_end = 0;

    while i < length {
        let c = s[i];

        // --- email: found by its `@`, then rewound to the start of the
        // local part
        if c == b'@' {
            // The rewind is bounded to keep this pass linear, and RFC 5321
            // bounds a local part in the same place anyway, so the cap
            // costs no real address.
            let capped = i as isize - MAX_EMAIL_LOCAL as isize;
            let floor = (last_end as isize).max(capped);
            let mut k = i as isize;
            while k > floor && is_email_local_char(s[k as usize - 1]) {
                k -= 1;
            }

            // Stopping *on* the cap is not the same as finding a boundary.
            // If the character just outside it still belongs to the local
            // part, the local part is longer than the cap allows and there
            // is no address here at all: accepting the match would link the
            // 64-character tail of a longer run and leave the rest as text,
            // inventing an address the source never wrote. `k` can only
            // reach `capped` when the cap, rather than `last_end`, is what
            // ended the loop; and `k` of 0 is the start of the string,
            // where there is no character outside the cap to disqualify
            // anything.
            if k == capped && k > 0 && is_email_local_char(s[k as usize - 1]) {
                i += 1;
                continue;
            }

            let k = k as usize;
            if k < i && is_autolink_start(s, k, at_start) {
                // After the `@`: alphanumerics, `.`, `-`, `_`; at least one
                // period; a trailing `.` is not part of the address, and a
                // trailing `-` or `_` invalidates the whole thing.
                let mut end = i + 1;
                while end < length && is_domain_char(s[end]) {
                    end += 1;
                }
                while end > i + 1 && s[end - 1] == b'.' {
                    end -= 1;
                }

                let last = if end > i + 1 { s[end - 1] } else { 0 };
                if end > i + 1 && last != b'-' && last != b'_' && has_period(s, i + 1, end) {
                    out.push(AutolinkMatch {
                        start: k,
                        end,
                        href: format!("mailto:{}", &text[k..end]),
                    });
                    last_end = end;
                    i = end;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // --- www / scheme: both need a valid start and a valid domain
        if !could_start_url(c) || !is_autolink_start(s, i, at_start) {
            i += 1;
            continue;
        }

        let mut domain_start: Option<usize> = None;
        let mut prefix = "";
        if matches_ignore_case(s, i, "www.") {
            // The domain of a `www.` autolink includes the `www.` itself, so
            // the required period is already there and `www.foo_bar.com` is
            // rejected on the same footing as `http://foo_bar.com`.
            domain_start = Some(i);
            prefix = "http://";
        } else {
            for scheme in AUTOLINK_SCHEMES {
                if matches_ignore_case(s, i, scheme) {
                    domain_start = Some(i + scheme.len());
                    break;
                }
            }
        }
        let Some(domain_start) = domain_start else {
            i += 1;
            continue;
        };

        let domain_end = scan_domain_end(s, domain_start);
        // Checked before the run scan below, so a candidate that cannot
        // possibly match costs only its own domain run.
        if !is_valid_domain(s, domain_start, domain_end) {
            i += 1;
            continue;
        }

        if i >= run_end || i < run_start {
            run_start = i;
            run_end = i;
            while run_end < length {
                let d = s[run_end];
                if is_autolink_space(d) || d == b'<' {
                    break;
                }
                run_end += 1;
            }
        }

        let end = trim_autolink_end(s, i, run_end);
        // Trailing punctuation can eat back into the domain
        // (`www.commonmark.org.` loses its last period), so the domain is
        // validated again over what actually survived.
        let survived = domain_end.min(end);
        if end <= domain_start || !is_valid_domain(s, domain_start, survived) {
            i += 1;
            continue;
        }

        out.push(AutolinkMatch {
            start: i,
            end,
            href: format!("{prefix}{}", &text[i..end]),
        });
        last_end = end;
        i = end;
    }

    out
}

/// Replace one text node with the text / link / text run its literal
/// implies.
fn linkify_text_node(tree: &mut Tree, node: NodeId) {
    let at_start = starts_after_delimiter(tree, tree.get(node).prev);
    let s = tree.get(node).literal.clone();
    let matches = scan_autolinks(&s, at_start);
    if matches.is_empty() {
        return;
    }

    let mut at = 0;
    for m in matches {
        if at < m.start {
            let text = text_node(tree, &s[at..m.start]);
            tree.insert_before(node, text);
        }
        // Not percent-encoded here, for the same reason as
        // `scan_link_destination`: encoding belongs to the renderer, and
        // the AST promises the decoded form.
        let link = make_autolink(tree, &m.href, &s[m.start..m.end]);
        tree.insert_before(node, link);
        at = m.end;
    }
    if at < s.len() {
        let text = text_node(tree, &s[at..]);
        tree.insert_before(node, text);
    }

    tree.unlink(node);
}

/// Autolink the text children of one container, and add the child
/// containers still to visit to `pending`.
fn linkify_children(tree: &mut Tree, parent: NodeId, pending: &mut Vec<NodeId>) {
    let mut child = tree.get(parent).first_child;

    while let Some(c) = child {
        if tree.get(c).node_type == NodeType::Text {
            // Consolidate the run of adjacent text nodes first. The scan
            // above emits a node per delimiter run and per character it
            // could not claim, so `a.b-c_d@a.b` arrives as three siblings
            // and the address is only visible once they are one string.
            // Merging changes nothing downstream: the renderer writes text
            // literals back to back, and the AST projection already
            // coalesces adjacent runs.
            let mut sibling = tree.get(c).next;
            if sibling.is_some_and(|s| tree.get(s).node_type == NodeType::Text) {
                let mut merged = tree.get(c).literal.clone();
                while let Some(s) = sibling {
                    if tree.get(s).node_type != NodeType::Text {
                        break;
                    }
                    merged.push_str(&tree.get(s).literal);
                    let next = tree.get(s).next;
                    tree.unlink(s);
                    sibling = next;
                }
                tree.get_mut(c).literal = merged;
            }

            let after = tree.get(c).next;
            linkify_text_node(tree, c);
            child = after;
            continue;
        }

        // No links inside links (section 6.3), and an image's children are
        // its alt text.
        let node = tree.get(c);
        if node.node_type != NodeType::Link
            && node.node_type != NodeType::Image
            && node.is_container()
        {
            pending.push(c);
        }
        child = node.next;
    }
}

/// Recognise GFM autolink literals throughout one finished block's inlines.
///
/// Iterative rather than recursive: emphasis nests as deeply as the input
/// says.
pub fn linkify_autolinks(tree: &mut Tree, block: NodeId) {
    let mut pending = vec![block];
    while let Some(node) = pending.pop() {
        linkify_children(tree, node, &mut pending);
    }
}

/// Blocks whose raw content the block phase left for this phase to parse.
/// GFM table cells hold inlines and nothing else ("block-level elements
/// cannot be inserted in a table"), so a cell is parsed exactly as a
/// paragraph is, autolink post-pass included.
pub fn is_inline_content_block(t: NodeType) -> bool {
    matches!(
        t,
        NodeType::Paragraph | NodeType::Heading | NodeType::TableCell
    )
}

/// The inline content blocks of `tree`, in the order the walk exits them:
/// the one iteration both inline drivers share (the hand scanner and the
/// engine path), so which blocks get inline parsing is decided exactly
/// once. Inline parsing only adds children beneath these blocks, so the
/// list taken before the phase runs is the walk the phase would make.
pub fn inline_content_blocks(tree: &Tree) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut walker = tree.walker(tree.root());
    while let Some(event) = walker.next(tree) {
        if !event.entering && is_inline_content_block(tree.get(event.node).node_type) {
            out.push(event.node);
        }
    }
    out
}

/// Walk the finished block tree, handing every inline content block
/// (paragraphs, headings, table cells) to `f` and running the GFM
/// autolink-literal post-pass after it, so when linkify runs is decided
/// exactly once for both drivers.
pub fn for_each_inline_block(
    tree: &mut Tree,
    options: &Options,
    mut f: impl FnMut(&mut Tree, NodeId),
) {
    for block in inline_content_blocks(tree) {
        f(tree, block);
        if options.gfm {
            linkify_autolinks(tree, block);
        }
    }
}

/// Phase 2 entry point: walk the block tree and parse the string content of
/// every paragraph, heading and table cell into inline children.
pub fn parse_inlines(tree: &mut Tree, refmap: RefMap, options: Options) {
    let mut parser = InlineParser::new(refmap, options);
    for_each_inline_block(tree, &options, |tree, block| parser.parse(tree, block));
}

/// Consume link reference definitions from the front of a paragraph's raw
/// content, adding them to `refmap`. Returns the number of BYTES consumed,
/// or 0 if `s` does not start with a definition.
///
/// Reference definitions are parsed during the block phase, before any
/// inline parser for the document exists, so a scratch parser is built per
/// call; nothing here depends on the parse options.
pub fn parse_reference(s: &str, refmap: &mut RefMap) -> usize {
    let mut parser = InlineParser::new(RefMap::new(), Options::COMMONMARK);
    parser.parse_reference(s, refmap)
}

/// Section 4.7 over a whole paragraph: every link reference definition
/// at the start of `s`, in order, each added to `refmap`. Returns the
/// number of BYTES they take up together.
///
/// One parser over one copy of `s`, advancing through it. Copying the
/// remaining tail once per definition, as the per-definition entry
/// point does, made a paragraph of n short definitions cost O(n^2).
pub fn parse_references(s: &str, refmap: &mut RefMap) -> usize {
    let mut parser = InlineParser::new(RefMap::new(), Options::COMMONMARK);
    parser.subject = s.to_string();
    parser.pos = 0;
    while parser.pos < s.len() && s.as_bytes()[parser.pos] == b'[' {
        if parser.parse_reference_here(refmap) == 0 {
            break;
        }
    }
    parser.pos
}
