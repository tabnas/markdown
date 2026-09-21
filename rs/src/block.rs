/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Phase 1 of the CommonMark parse: block structure. Port of
//! `ts/src/block.ts` (canonical) and `go/block.go`; keep the three in
//! step, function for function. Spec 0.31.2, Appendix A "A parsing
//! strategy".
//!
//! The document is a tree of blocks whose right spine (the document, then
//! last child, then last child of that, and so on) is *open*: each open
//! block imposes a continuation condition on the next line. For every line
//! we
//!
//! 1. walk that spine, consuming each block's continuation marker (`>` for
//!    a block quote, the content indent for a list item, ...) and remember
//!    the last container that matched;
//! 2. look for new block starts at the offset that walk reached, closing
//!    the unmatched tail of the spine as soon as a start actually matches;
//! 3. add whatever is left of the line to the deepest open block, opening a
//!    paragraph if there is nowhere else for the text to go.
//!
//! Unmatched blocks are closed in step 2/3 rather than in step 1 because a
//! paragraph line may be a *lazy continuation*: its containers' markers are
//! missing, but the paragraph carries on regardless (section 5.1 rule 2).
//!
//! Positions on a line are tracked twice: `offset`, a BYTE index used to
//! slice the text we keep (the TypeScript's is a UTF-16 index), and
//! `column`, the tab-expanded display column used for every indent
//! decision (section 2.2). Every block delimiter is ASCII and no UTF-8
//! continuation byte can collide with one, so scanning by byte is exact;
//! `column` and sourcepos columns still count CHARACTERS, which is what the
//! spec means, so byte offsets pass through `char_count` on the way to a
//! column. The two must move together, including the case where a marker
//! consumes only part of a tab; see `advance_offset` and `add_line`.
//!
//! The two regexes in `block.ts` that use lookahead (the code fences) are
//! hand-coded here, as they are in Go: the `regex` crate has no lookahead.
//! Fence runs are counted with a loop, never with a regex built from an
//! input-derived repeat count.
//!
//! This file owns everything the block phase sets on a node: type, level,
//! literal (code_block / html_block), info / is_fenced, list_data
//! (including the final `tight`), sourcepos, and `string_content`, the
//! raw, unparsed text of paragraphs and headings that the inline phase
//! consumes.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::common::{js_trim, unescape_string};
use crate::inline::parse_references;
use crate::node::{
    Align, ListData, ListType, MdNode, NodeId, NodeType, SourcePos, TableAlign, Tree,
};
use crate::options::{Options, RefMap};

/// Section 4.4: four columns of indentation open an indented code block.
const CODE_INDENT: usize = 4;

/// One physical line of the source, with section 2.3's NUL replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSegment {
    /// The line's text, terminator excluded.
    pub text: String,
    /// The byte offset past the terminator.
    pub next: usize,
}

/// Cut the next physical line out of `src` at `pos`: up to (not including)
/// the next `\n`, `\r` or `\r\n`, with `next` past the terminator. `None`
/// at the end of the source, which is why a final line ending does not
/// introduce a trailing blank line. Insecure NULs are replaced here, before
/// anything else looks at the text.
pub fn segment_next_line(src: &str, pos: usize) -> Option<LineSegment> {
    if pos >= src.len() {
        return None;
    }
    let bytes = src.as_bytes();
    let mut i = pos;
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    let mut next = i;
    if next < bytes.len() {
        if bytes[next] == b'\r' && next + 1 < bytes.len() && bytes[next + 1] == b'\n' {
            next += 2;
        } else {
            next += 1;
        }
    }
    let text = &src[pos..i];
    let text = if text.contains('\0') {
        text.replace('\0', "\u{FFFD}")
    } else {
        text.to_string()
    };
    Some(LineSegment { text, next })
}

/// Section 4.1: 3+ matching `*`, `_` or `-`, each optionally followed by
/// spaces or tabs, and nothing else.
pub fn is_thematic_break(s: &str) -> bool {
    let bytes = s.as_bytes();
    let Some(&c) = bytes.first() else {
        return false;
    };
    if c != b'*' && c != b'_' && c != b'-' {
        return false;
    }
    let mut count = 0;
    for &b in bytes {
        if b == c {
            count += 1;
        } else if b != b' ' && b != b'\t' {
            return false;
        }
    }
    count >= 3
}

/// Step 2's fast reject: every block start begins with one of these
/// characters.
pub fn maybe_special(ln: &str, pos: usize) -> bool {
    match ln.as_bytes().get(pos) {
        Some(c) => {
            matches!(
                c,
                b'#' | b'`' | b'~' | b'*' | b'+' | b'_' | b'=' | b'<' | b'>' | b'-'
            ) || c.is_ascii_digit()
        }
        None => false,
    }
}

/// The same fast reject with `|` and `:` added, used when GFM is on: a GFM
/// table's delimiter row may begin with either (`| --- |`, `:-: | ---:`),
/// and neither is a CommonMark block start.
///
/// Kept separate so that a GFM-off parse takes exactly the path it always
/// did: same characters rejected, same lines never offered to the block
/// starts at all.
pub fn maybe_special_gfm(ln: &str, pos: usize) -> bool {
    match ln.as_bytes().get(pos) {
        Some(c) => {
            matches!(
                c,
                b'#' | b'`' | b'~' | b'*' | b'+' | b'_' | b'=' | b'<' | b'>' | b'-' | b':' | b'|'
            ) || c.is_ascii_digit()
        }
        None => false,
    }
}

/// Nothing but spaces, tabs, form feeds, vertical tabs and line endings.
/// All members of the set are ASCII, so a byte scan classifies UTF-8
/// correctly: a continuation byte counts as non-space, as the character it
/// belongs to would.
pub fn is_blank(s: &str) -> bool {
    s.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\x0c' | b'\x0b' | b'\r' | b'\n'))
}

/// Section 5.2: a bullet list marker is one of `*`, `+`, `-`.
pub fn is_bullet_list_marker(s: &str) -> bool {
    matches!(s.as_bytes().first(), Some(b'*' | b'+' | b'-'))
}

/// Section 5.2: an ordered list marker is 1 to 9 digits and `.` or `)`.
/// Returns the digit count and the delimiter. Ten or more digits do not
/// match.
pub fn match_ordered_list_marker(s: &str) -> Option<(usize, u8)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=9).contains(&i) || i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'.' || bytes[i] == b')' {
        return Some((i, bytes[i]));
    }
    None
}

/// Section 4.2: 1 to 6 `#` followed by spaces or tabs, or the end of the
/// line. Returns the level and the whole marker's length. Seven or more `#`
/// do not match.
pub fn match_atx_heading_marker(s: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut n = 0;
    while n < bytes.len() && bytes[n] == b'#' {
        n += 1;
    }
    if !(1..=6).contains(&n) {
        return None;
    }
    if n == bytes.len() {
        return Some((n, n));
    }
    if bytes[n] != b' ' && bytes[n] != b'\t' {
        return None;
    }
    let mut i = n;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    Some((n, i))
}

/// Section 4.2: drop an optional closing sequence of `#`s, preceded by
/// spaces or tabs; a line of nothing but `#`s (and spaces) has empty
/// content. Hand-coded from the two regexes the TypeScript uses.
pub fn strip_atx_closing(content: &str) -> &str {
    let bytes = content.as_bytes();
    let space = |b: u8| b == b' ' || b == b'\t';

    // /^[ \t]*#+[ \t]*$/ : the whole line is one closing sequence.
    let mut i = 0;
    while i < bytes.len() && space(bytes[i]) {
        i += 1;
    }
    let hashes_start = i;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    let hashes = i - hashes_start;
    while i < bytes.len() && space(bytes[i]) {
        i += 1;
    }
    if hashes > 0 && i == bytes.len() {
        return "";
    }

    // /[ \t]+#+[ \t]*$/ : a trailing closing sequence after content.
    let mut end = bytes.len();
    while end > 0 && space(bytes[end - 1]) {
        end -= 1;
    }
    let hashes_end = end;
    while end > 0 && bytes[end - 1] == b'#' {
        end -= 1;
    }
    if end == hashes_end {
        return content;
    }
    let spaces_end = end;
    while end > 0 && space(bytes[end - 1]) {
        end -= 1;
    }
    if end == spaces_end {
        return content;
    }
    &content[..end]
}

/// Section 4.5: the opening code fence at the start of `s`, as its length,
/// or 0. A backtick fence is only a fence if the rest of the line (the info
/// string) holds no backtick; a tilde fence has no such restriction.
pub fn match_code_fence(s: &str) -> usize {
    let bytes = s.as_bytes();
    let Some(&c) = bytes.first() else {
        return 0;
    };
    if c != b'`' && c != b'~' {
        return 0;
    }
    let mut n = 0;
    while n < bytes.len() && bytes[n] == c {
        n += 1;
    }
    if n < 3 {
        return 0;
    }
    if c == b'`' && bytes[n..].contains(&b'`') {
        return 0;
    }
    n
}

/// Section 4.5: the closing-fence candidate at the start of `s`, as its
/// length, or 0: the maximal run must be followed by spaces or tabs only.
/// The caller still checks the fence character and length against the open
/// block.
pub fn match_closing_code_fence(s: &str) -> usize {
    let bytes = s.as_bytes();
    let Some(&c) = bytes.first() else {
        return 0;
    };
    if c != b'`' && c != b'~' {
        return 0;
    }
    let mut n = 0;
    while n < bytes.len() && bytes[n] == c {
        n += 1;
    }
    if n < 3 {
        return 0;
    }
    if bytes[n..].iter().any(|&b| b != b' ' && b != b'\t') {
        return 0;
    }
    n
}

/// Section 4.3: a run of `=` or `-` with only spaces or tabs after it.
/// Returns the run's character, or `None` when the line is not an
/// underline.
pub fn match_setext_heading_line(s: &str) -> Option<u8> {
    let bytes = s.as_bytes();
    let &c = bytes.first()?;
    if c != b'=' && c != b'-' {
        return None;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i] == c {
        i += 1;
    }
    if bytes[i..].iter().any(|&b| b != b' ' && b != b'\t') {
        return None;
    }
    Some(c)
}

// --- HTML blocks (section 4.6) ---------------------------------------------
//
// Seven kinds, each with its own start and end condition. Types 1 to 5 end
// on a line *containing* their closing string (checked in
// `incorporate_line` after the line is added); types 6 and 7 end at a blank
// line (checked in the html_block continuation condition). Type 7 alone may
// not interrupt a paragraph.

/// Tag-shape fragments shared by HTML block type 7 (section 6.6 raw HTML).
/// The block forms allow only spaces and tabs where the inline forms allow
/// any whitespace.
const BLOCK_TAG_NAME: &str = "[A-Za-z][A-Za-z0-9-]*";
const BLOCK_ATTRIBUTE_NAME: &str = "[a-zA-Z_:][a-zA-Z0-9:._-]*";
const BLOCK_UNQUOTED_VALUE: &str = "[^\"'=<>`\\x00-\\x20]+";
const BLOCK_SINGLE_QUOTED_VALUE: &str = "'[^']*'";
const BLOCK_DOUBLE_QUOTED_VALUE: &str = "\"[^\"]*\"";

/// The type 6 tag list, verbatim from section 4.6 (0.31.2 has `search`,
/// not `source`).
const BLOCK_TAGS: &str = "address|article|aside|base|basefont|blockquote|body|caption|\
center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|\
figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|\
li|link|main|menu|menuitem|nav|noframes|ol|optgroup|option|p|param|search|\
section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul";

/// The type 1 tag list: the four whose content is raw text.
const BLOCK_RAW_TEXT_TAGS: &str = "script|pre|textarea|style";

/// Spell a lowercase pattern in both cases, mapping each ASCII letter to a
/// two-element class and passing everything else through (so `h[1-6]`
/// becomes `[hH][1-6]` and `|` stays an alternation).
///
/// This is what the TypeScript's `i` flag means and what a Unicode-aware
/// `(?i)` does not: the `regex` crate folds case over all of Unicode, so
/// `(?i)script` also matches `ſcript`, and `(?i)[A-Za-z]` also matches
/// U+017F LATIN SMALL LETTER LONG S and U+212A KELVIN SIGN, where
/// JavaScript's non-unicode `i` deliberately never folds a non-ASCII code
/// point onto an ASCII one.
fn ascii_fold_pattern(s: &str) -> String {
    let mut out = String::with_capacity(4 * s.len());
    for c in s.chars() {
        if c.is_ascii_lowercase() {
            out.push('[');
            out.push(c);
            out.push(c.to_ascii_uppercase());
            out.push(']');
        } else if c.is_ascii_uppercase() {
            out.push('[');
            out.push(c.to_ascii_lowercase());
            out.push(c);
            out.push(']');
        } else {
            out.push(c);
        }
    }
    out
}

/// Indexed by HTML block type 1 to 7; slot 0 is unused.
fn re_html_block_open() -> &'static [Regex; 8] {
    static RE: OnceLock<[Regex; 8]> = OnceLock::new();
    RE.get_or_init(|| {
        let attribute_value = format!(
            "(?:{BLOCK_UNQUOTED_VALUE}|{BLOCK_SINGLE_QUOTED_VALUE}|{BLOCK_DOUBLE_QUOTED_VALUE})"
        );
        let attribute_value_spec = format!("(?:[ \\t]*=[ \\t]*{attribute_value})");
        let attribute = format!("(?:[ \\t]+{BLOCK_ATTRIBUTE_NAME}{attribute_value_spec}?)");
        let open_tag = format!("<{BLOCK_TAG_NAME}{attribute}*[ \\t]*/?>");
        let close_tag = format!("</{BLOCK_TAG_NAME}[ \\t]*>");
        let compile = |p: String| Regex::new(&p).expect("the HTML block patterns compile");
        [
            compile("^$".to_string()),
            compile(format!(
                "^<(?:{})(?:[ \\t]|>|$)",
                ascii_fold_pattern(BLOCK_RAW_TEXT_TAGS)
            )),
            compile("^<!--".to_string()),
            compile("^<[?]".to_string()),
            compile("^<![A-Za-z]".to_string()),
            compile("^<!\\[CDATA\\[".to_string()),
            compile(format!(
                "^</?(?:{})(?:[ \\t]|/?>|$)",
                ascii_fold_pattern(BLOCK_TAGS)
            )),
            compile(format!("^(?:{open_tag}|{close_tag})[ \\t]*$")),
        ]
    })
}

fn re_html_block_close_raw_text() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            "</(?:{})>",
            ascii_fold_pattern(BLOCK_RAW_TEXT_TAGS)
        ))
        .expect("the raw-text close pattern compiles")
    })
}

/// Section 4.6: which of the seven HTML block kinds `s` opens, or 0. The
/// type-7 paragraph-interrupt restriction is contextual and stays with the
/// caller.
pub fn html_block_open_kind(s: &str) -> u8 {
    let patterns = re_html_block_open();
    for kind in 1..=7u8 {
        if patterns[kind as usize].is_match(s) {
            return kind;
        }
    }
    0
}

/// Section 4.6: does `s` contain the closing condition for HTML block
/// `kind` (types 1 to 5; 6 and 7 end at a blank line instead)?
pub fn html_block_closes(s: &str, kind: u8) -> bool {
    match kind {
        1 => re_html_block_close_raw_text().is_match(s),
        2 => s.contains("-->"),
        3 => s.contains("?>"),
        4 => s.contains('>'),
        5 => s.contains("]]>"),
        _ => false,
    }
}

// --- small lexical helpers --------------------------------------------------

/// The byte at `pos`, or `None` past the end of the line. Every caller
/// compares against an ASCII constant, which no UTF-8 continuation byte can
/// equal, so a byte here is as good as the TypeScript's code unit.
fn peek(ln: &str, pos: usize) -> Option<u8> {
    ln.as_bytes().get(pos).copied()
}

fn is_space_or_tab(c: Option<u8>) -> bool {
    matches!(c, Some(b' ') | Some(b'\t'))
}

/// The number of characters in `s`. `offset` is a byte index in this port;
/// sourcepos columns count characters, so the conversion happens here.
pub fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// The index at which the trailing `(\n *)+` of `b` begins, or `b.len()`
/// when there is none: the offset the JavaScript `.replace(/(\n *)+$/, ..)`
/// would rewrite from.
fn trailing_blank_run_start(b: &[u8]) -> usize {
    let mut end = b.len();
    loop {
        let mut j = end;
        while j > 0 && b[j - 1] == b' ' {
            j -= 1;
        }
        if j > 0 && b[j - 1] == b'\n' {
            end = j - 1;
            continue;
        }
        return end;
    }
}

// --- GFM tables (extension) -------------------------------------------------
//
// Everything the extension needs from a line is here: how a row splits into
// cells, and whether a line is a delimiter row. Both are pure functions of
// one line of text, which is what lets the block start below stay a few
// lines long.
//
// Byte scanning throughout, as everywhere else in this file: every
// character these functions test for (space, tab, `|`, `\`, `-`, `:`) is
// ASCII, and no UTF-8 continuation byte can equal one. The cell text itself
// is only ever *sliced* at those positions, never decoded character by
// character, so a multi-byte character inside a cell is carried through
// whole.

/// The set `cmark_isspace` trims from a table cell: ASCII whitespace only.
fn is_ascii_space(c: u8) -> bool {
    c == 32 || (9..=13).contains(&c)
}

fn trim_ascii_space(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && is_ascii_space(bytes[start]) {
        start += 1;
    }
    while end > start && is_ascii_space(bytes[end - 1]) {
        end -= 1;
    }
    &s[start..end]
}

/// Split one row of a table into its cell texts.
///
/// Cells are separated by *unescaped* pipes; a leading and a trailing pipe
/// are both optional and may differ from row to row, so they are stripped
/// before the split rather than producing empty cells at the ends. A
/// trailing pipe is only a delimiter if it is itself unescaped, which is
/// why the backslash run before it is counted.
///
/// `\|` is resolved to a literal `|` *here*, as the extension requires
/// ("include a pipe in a cell's content by escaping it, including inside
/// other inline spans"). Doing it at split time is the whole point: the
/// inline phase never sees the backslash, so a code span in the cell yields
/// a real pipe, which no amount of unescaping *after* inline parsing could
/// produce. Every other backslash escape is left exactly as written for the
/// inline phase to handle in the usual way, so no cell text is unescaped
/// twice.
///
/// Scanning skips the byte after any backslash, so `\\` cannot shield the
/// pipe that follows it: `a\\|b` is two cells, the first holding `a\\`.
///
/// A row of nothing but whitespace has no cells at all, which is how a
/// header candidate that is really empty fails the cell-count test instead
/// of matching a one-column delimiter row.
pub fn split_table_row(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && is_ascii_space(bytes[start]) {
        start += 1;
    }
    while end > start && is_ascii_space(bytes[end - 1]) {
        end -= 1;
    }
    if start == end {
        return Vec::new();
    }

    // Optional leading pipe.
    if bytes[start] == b'|' {
        start += 1;
    }

    // Optional trailing pipe, but an escaped one is content, not a delimiter.
    if start < end && bytes[end - 1] == b'|' {
        let mut bs = end as isize - 2;
        while bs >= start as isize && bytes[bs as usize] == b'\\' {
            bs -= 1;
        }
        // An even-length backslash run leaves the pipe unescaped.
        if (end as isize - 2 - bs) % 2 == 0 {
            end -= 1;
        }
    }

    let mut cells = Vec::with_capacity(4);
    // `cur` holds the parts of the current cell that have already been
    // rewritten; `seg` is the start of the run since then, so the common
    // case (no escapes) costs one slice per cell rather than one per byte.
    let mut cur = String::new();
    let mut seg = start;
    let mut i = start;

    while i < end {
        let c = bytes[i];
        if c == b'\\' && i + 1 < end {
            if bytes[i + 1] == b'|' {
                cur.push_str(&line[seg..i]);
                cur.push('|');
                i += 2;
                seg = i;
            } else {
                i += 2;
            }
            continue;
        }
        if c == b'|' {
            cur.push_str(&line[seg..i]);
            cells.push(trim_ascii_space(&cur).to_string());
            cur.clear();
            i += 1;
            seg = i;
            continue;
        }
        i += 1;
    }
    cur.push_str(&line[seg..end]);
    cells.push(trim_ascii_space(&cur).to_string());

    cells
}

/// A delimiter cell is hyphens with an optional leading and/or trailing
/// colon, and nothing else. One `+`, one space in the middle, one stray
/// character and the line is not a delimiter row. Hand-coded from
/// `/^(:?)-+(:?)$/`.
fn delimiter_cell_align(cell: &str) -> Option<TableAlign> {
    let bytes = cell.as_bytes();
    let mut i = 0;
    let left = bytes.first() == Some(&b':');
    if left {
        i += 1;
    }
    let hyphens_start = i;
    while i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }
    if i == hyphens_start {
        return None;
    }
    let right = i < bytes.len() && bytes[i] == b':';
    if right {
        i += 1;
    }
    if i != bytes.len() {
        return None;
    }
    Some(match (left, right) {
        (true, true) => Some(Align::Center),
        (true, false) => Some(Align::Left),
        (false, true) => Some(Align::Right),
        (false, false) => None,
    })
}

/// Read a line as a table's delimiter row, returning one alignment per
/// column, or `None` if it is not one.
pub fn parse_delimiter_row(line: &str) -> Option<Vec<TableAlign>> {
    let cells = split_table_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut align = Vec::with_capacity(cells.len());
    for cell in &cells {
        align.push(delimiter_cell_align(cell)?);
    }
    Some(align)
}

/// Cap on the empty cells inserted to pad short body rows, across a whole
/// document: cmark-gfm's `MAX_AUTOCOMPLETED_CELLS`, and for the same reason.
///
/// Padding is the one place where a table's node count is not bounded by
/// the size of its source: a 10000-column header followed by 10000 one-cell
/// rows is 60 KB of input asking for 10^8 cells. Every other part of the
/// extension is linear in the input, so this single budget is what keeps
/// the whole of it so. Reaching it needs input that is already
/// pathological; ordinary tables are nowhere near.
const MAX_AUTOCOMPLETED_CELLS: usize = 0x80000;

/// The most container blocks (block quotes, lists and list items, each
/// counted) that may be open at once. A block quote marker or list marker
/// that would open a deeper one is not a block start, so it is paragraph
/// text, which is what markdown-it's `maxNesting` does at its limit.
///
/// This bound exists for the caller's stack, not the parser's: every
/// phase here is iterative, but the AST is a `tabnas::Value`, whose
/// destructor and `to_json` recurse once per nesting level. Unbounded,
/// a few thousand nested block quotes made an ordinary caller's drop
/// abort the process. Measured in a debug build on a 2 MiB thread: drop
/// survives about 5,300 nested levels and `to_json` about 1,250, and
/// each container is two levels (an object and its children array), so
/// 100 containers leaves the deepest document well inside both. The
/// canonical TypeScript has no such limit; `DIVERGENCE.md` records it,
/// and `tests/robust_test.rs` pins the boundary. The inline phase bounds
/// its own wrappers the same way: `inline::MAX_INLINE_NESTING`.
pub const MAX_CONTAINER_NESTING: usize = 100;

/// The blocks `MAX_CONTAINER_NESTING` counts.
fn is_container(t: NodeType) -> bool {
    matches!(t, NodeType::BlockQuote | NodeType::List | NodeType::Item)
}

/// Section 5.3: two markers make the same list only if type, bullet and
/// delimiter agree.
fn lists_match(list_data: &ListData, item_data: &ListData) -> bool {
    list_data.list_type == item_data.list_type
        && list_data.delimiter == item_data.delimiter
        && list_data.bullet_char == item_data.bullet_char
}

/// `continue` result: matched, not matched, or line fully consumed (fenced
/// code close), stop processing this line entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContinueResult {
    Matched,
    NotMatched,
    LineDone,
}

/// A block start's result: no match, a container started, or a leaf
/// started (stop looking).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartResult {
    None,
    Container,
    Leaf,
}

/// The per-type block behaviour the TypeScript keeps in `BLOCK_HANDLERS`:
/// which children a block may hold, whether it accumulates raw text, and
/// whether its lines are literal.
fn can_contain(t: NodeType, child: NodeType) -> bool {
    match t {
        NodeType::Document | NodeType::BlockQuote | NodeType::Item => child != NodeType::Item,
        NodeType::List => child == NodeType::Item,
        NodeType::Table => child == NodeType::TableRow,
        NodeType::TableRow => child == NodeType::TableCell,
        _ => false,
    }
}

/// Leaves that accumulate raw text: code blocks, HTML blocks, paragraphs,
/// tables.
fn accepts_lines(t: NodeType) -> bool {
    matches!(
        t,
        NodeType::CodeBlock | NodeType::HtmlBlock | NodeType::Paragraph | NodeType::Table
    )
}

/// Lines belonging to this block are literal text, so Appendix A step 2
/// does not look for block starts while it is the last matched container.
///
/// True for exactly the two blocks the spec names: code blocks and HTML
/// blocks. Paragraphs and GFM tables also accept lines, but a block start
/// may still interrupt either: that is how `> bar` ends a table, and how
/// `# h` ends a paragraph.
fn verbatim(t: NodeType) -> bool {
    matches!(t, NodeType::CodeBlock | NodeType::HtmlBlock)
}

/// The block-phase parser: the open spine, the current line, and the
/// tree being built.
pub struct BlockParser {
    pub options: Options,
    pub refmap: RefMap,

    pub tree: Tree,
    doc: NodeId,
    /// The deepest open block; `None` only once the document itself is
    /// finalized.
    tip: Option<NodeId>,
    /// The tip as it was before the current line was walked.
    oldtip: NodeId,
    last_matched_container: NodeId,
    /// False while some open block failed its continuation condition.
    all_closed: bool,

    current_line: String,
    line_number: usize,
    last_line_length: usize,

    /// The GFM table probe gate. True by default: the batch driver probes
    /// every candidate line, as it always has. The engine driver sets it
    /// per line from the `#LB` token's `tblArm` bit (a provable superset of
    /// "could be a delimiter row"; see `engine_block.rs`), which is what
    /// lets the GFM alt be a genuine, subtractable extension seam without
    /// moving any table decision out of this file.
    pub table_armed: bool,

    /// The length of the line before the one `last_line_length` measures.
    ///
    /// `finalize` dates a block's end from `last_line_length`, which is
    /// right for every block that ends on the line before the current one.
    /// A GFM table splits its paragraph *two* lines back (the header row is
    /// the previous line and stays with the table, so the paragraph left
    /// behind ends on the line before that) and needs this one instead.
    prev_line_length: usize,

    /// A byte index into `current_line`.
    offset: usize,
    /// The tab-expanded display column matching `offset` (section 2.2),
    /// counted in characters.
    column: usize,
    /// True when `offset` sits inside a tab whose columns were partly
    /// consumed.
    partially_consumed_tab: bool,

    /// Memoize the last byte offset in `current_line` whose character count
    /// is known, so sourcepos columns cost amortized O(1) rather than a
    /// fresh count from the start of the line. Both reset per line. See
    /// `source_column`.
    col_anchor_offset: usize,
    col_anchor_chars: usize,

    next_nonspace: usize,
    next_nonspace_column: usize,
    /// Columns between `column` and the next non-space character.
    indent: usize,
    indented: bool,
    blank: bool,

    /// Empty cells inserted so far to pad short table rows. See
    /// `MAX_AUTOCOMPLETED_CELLS`.
    autocompleted_cells: usize,

    /// The text `add_line` last appended, the column it started in, and the
    /// block it went to.
    ///
    /// `try_open_table` needs the *last line* of an open paragraph, and
    /// reading it back out of `string_content` is not affordable in the
    /// canonical runtime: that content is built by repeated `+=`, so every
    /// index into it flattens a fresh rope. The field is carried across
    /// anyway: it is what makes the three runtimes the same function, and
    /// scanning back for the previous newline is its own O(n) per line.
    ///
    /// The node is recorded alongside it because "the previous line went to
    /// this very paragraph" is the extension's own rule: a header row is by
    /// definition the line immediately above the delimiter row. If some
    /// other block took that line, there is no header row.
    ///
    /// The column is what a table's sourcepos starts at: a table opens on
    /// its header row, which is a line already consumed by the time the
    /// delimiter row identifies it, so its column has to have been recorded
    /// when it went past.
    last_added_line: String,
    last_added_column: usize,
    last_added_to: Option<NodeId>,

    /// Step 2's fast reject, widened for a table's delimiter row when GFM is
    /// on.
    gfm_special: bool,

    /// `MdNode` has no field for the HTML block type, and the tree contract
    /// is not ours to change, so open HTML blocks carry it here.
    html_block_types: HashMap<NodeId, u8>,
}

impl BlockParser {
    pub fn new(options: Options) -> Self {
        let mut tree = Tree::new(NodeType::Document);
        let doc = tree.root();
        tree.get_mut(doc).sourcepos = [[1, 1], [0, 0]];
        BlockParser {
            options,
            refmap: RefMap::new(),
            tree,
            doc,
            tip: Some(doc),
            oldtip: doc,
            last_matched_container: doc,
            all_closed: true,
            current_line: String::new(),
            line_number: 0,
            last_line_length: 0,
            table_armed: true,
            prev_line_length: 0,
            offset: 0,
            column: 0,
            partially_consumed_tab: false,
            col_anchor_offset: 0,
            col_anchor_chars: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            autocompleted_cells: 0,
            last_added_line: String::new(),
            last_added_column: 1,
            last_added_to: None,
            gfm_special: options.gfm,
            html_block_types: HashMap::new(),
        }
    }

    /// The deepest open block. Only meaningful while a line is being
    /// processed. The TypeScript throws when the tip is null; a missing tip
    /// means the document itself has been closed, which cannot happen
    /// mid-line, and this port must not panic on any input, so the document
    /// stands in.
    fn open_tip(&self) -> NodeId {
        self.tip.unwrap_or(self.doc)
    }

    fn html_block_type(&self, block: NodeId) -> u8 {
        self.html_block_types.get(&block).copied().unwrap_or(0)
    }

    fn set_html_block_type(&mut self, block: NodeId, kind: u8) {
        self.html_block_types.insert(block, kind);
    }

    fn node_type(&self, id: NodeId) -> NodeType {
        self.tree.get(id).node_type
    }

    /// Advance `count` characters (`columns` false) or `count` columns
    /// (`columns` true), expanding tabs to the next 4-column tab stop
    /// (section 2.2). A tab whose columns are only partly consumed leaves
    /// `offset` on the tab itself and sets `partially_consumed_tab`, so
    /// `add_line` can re-emit the remainder as spaces.
    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        let bytes = self.current_line.as_bytes();
        while count > 0 && self.offset < bytes.len() {
            if bytes[self.offset] == b'\t' {
                let chars_to_tab = 4 - (self.column % 4);
                if columns {
                    self.partially_consumed_tab = chars_to_tab > count;
                    let chars_to_advance = chars_to_tab.min(count);
                    self.column += chars_to_advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= chars_to_advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += chars_to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                // One character, not one byte: offset is a byte index but
                // column counts characters. Block markers are all ASCII, so
                // the two only part company on the advance-to-end-of-line
                // calls.
                let size = self.current_line[self.offset..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
                self.offset += size;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    /// Locate the next non-space character and the indent leading up to
    /// it.
    fn find_next_nonspace(&mut self) {
        let bytes = self.current_line.as_bytes();
        let mut i = self.offset;
        let mut cols = self.column;

        while i < bytes.len() {
            if bytes[i] == b' ' {
                i += 1;
                cols += 1;
            } else if bytes[i] == b'\t' {
                i += 1;
                cols += 4 - (cols % 4);
            } else {
                break;
            }
        }

        self.blank = i >= bytes.len() || bytes[i] == b'\n' || bytes[i] == b'\r';
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = self.next_nonspace_column - self.column;
        self.indented = self.indent >= CODE_INDENT;
    }

    /// The number of characters of `current_line` before byte `offset`:
    /// what a sourcepos column needs.
    ///
    /// Counted incrementally from the last offset already measured.
    /// `add_child` runs once per container opened, so counting from the
    /// start of the line every time would make a line of n nested
    /// containers (`> - > - ...`) cost O(n^2); untrusted input picks n.
    /// Offsets advance monotonically as a line is consumed, making this
    /// amortized O(1); a smaller offset than the anchor is still correct,
    /// just recounted from the start.
    fn source_column(&mut self, offset: usize) -> usize {
        if offset < self.col_anchor_offset {
            self.col_anchor_offset = 0;
            self.col_anchor_chars = 0;
        }
        self.col_anchor_chars += char_count(&self.current_line[self.col_anchor_offset..offset]);
        self.col_anchor_offset = offset;
        self.col_anchor_chars
    }

    /// Add the rest of the current line to the tip's raw content.
    fn add_line(&mut self) {
        let tip = self.open_tip();
        // Taken before the tab fixup below moves `offset`, so a line's
        // recorded column is the one `add_child` would report for that
        // same position.
        let column = self.source_column(self.offset) + 1; // offset 0 is column 1
        let text = if self.partially_consumed_tab {
            self.offset += 1; // step over the tab
            let chars_to_tab = 4 - (self.column % 4);
            let mut text = " ".repeat(chars_to_tab);
            text.push_str(&self.current_line[self.offset..]);
            text
        } else {
            self.current_line[self.offset..].to_string()
        };
        let node = self.tree.get_mut(tip);
        node.string_content.push_str(&text);
        node.string_content.push('\n');
        self.last_added_line = text;
        self.last_added_column = column;
        self.last_added_to = Some(tip);
    }

    /// How many container blocks the spine holds from the document down
    /// to `id`, `id` included.
    fn container_depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut cur = Some(id);
        while let Some(n) = cur {
            if is_container(self.node_type(n)) {
                depth += 1;
            }
            cur = self.tree.get(n).parent;
        }
        depth
    }

    /// The container depth a new block of `tag` would open at under `tip`.
    /// `add_child` closes the blocks that cannot hold it first, so the
    /// holder is the nearest of `tip` and its ancestors that can (or the
    /// document), and the new block is one deeper than that. The walk is
    /// bounded by `MAX_CONTAINER_NESTING` itself, since the spine never
    /// holds more containers than that.
    fn depth_of_new(&self, tip: NodeId, tag: NodeType) -> usize {
        let mut holder = tip;
        while holder != self.doc && !can_contain(self.node_type(holder), tag) {
            holder = self
                .tree
                .get(holder)
                .parent
                .expect("every block below the document has a parent");
        }
        self.container_depth(holder) + 1
    }

    /// Open a block of `tag` as a child of the tip, closing blocks that
    /// cannot contain it first.
    fn add_child(&mut self, tag: NodeType, offset: usize) -> NodeId {
        // The document accepts everything but an item, and the list-item
        // start always opens a list first, so this terminates at the
        // document at the latest. Stopping there rather than closing the
        // document keeps the parser total where the TypeScript would throw
        // on the next `openTip`.
        loop {
            let tip = self.open_tip();
            if tip == self.doc || can_contain(self.node_type(tip), tag) {
                break;
            }
            self.finalize(tip, self.line_number - 1);
        }

        let column_number = self.source_column(offset) + 1; // offset 0 is column 1
        let mut node = MdNode::new(tag);
        node.sourcepos = [[self.line_number, column_number], [0, 0]];
        let id = self.tree.add(node);
        let tip = self.open_tip();
        self.tree.append_child(tip, id);
        self.tip = Some(id);
        id
    }

    /// The GFM tables block start: read the current line as the delimiter
    /// row of a table whose header row is the last line of the open
    /// paragraph `container`.
    ///
    /// The two rows must agree on the number of cells; if they do not,
    /// there is no table and the line is nothing special (spec example 6
    /// keeps the whole thing a paragraph). The header row is the paragraph's
    /// **last** line only, so a paragraph that had earlier lines is split:
    /// those lines stay a paragraph, finalized here, which is what still
    /// lets them contribute link reference definitions, and the table is
    /// opened after it.
    ///
    /// On success the table is the tip and the caller returns "leaf
    /// started", so step 3 adds the delimiter row itself to the table's raw
    /// content. It is skipped again by `finalize_table`; keeping it costs
    /// nothing and makes the table's accumulated lines line up one-for-one
    /// with the source lines.
    fn try_open_table(&mut self, container: NodeId) -> bool {
        if !self.table_armed {
            return false;
        }
        if !self.options.gfm || self.indented || self.node_type(container) != NodeType::Paragraph {
            return false;
        }

        let Some(align) = parse_delimiter_row(&self.current_line[self.next_nonspace..]) else {
            return false;
        };

        // The header row is the line directly above, which is the line
        // `add_line` last wrote: to this paragraph, or there is no header
        // row at all.
        if self.last_added_to != Some(container) {
            return false;
        }
        let header_cells = split_table_row(&self.last_added_line);
        if header_cells.len() != align.len() {
            return false;
        }

        self.close_unmatched_blocks();

        let header_line = self.line_number - 1;
        let header_column = self.last_added_column;
        // `string_content` ends with that same line plus the newline
        // `add_line` put after it, so the header row starts this far in: no
        // scan needed, and the content is only ever touched on the split
        // branch, at most once per table.
        let content_len = self.tree.get(container).string_content.len();
        let header_start = content_len as isize - self.last_added_line.len() as isize - 1;
        if header_start > 0 {
            // Split: everything above the header row stays a paragraph. It
            // ends two lines back, not one, so `finalize`'s default end
            // column (the previous line's, which here is the header row's)
            // would be somebody else's.
            self.tree
                .get_mut(container)
                .string_content
                .truncate(header_start as usize);
            let end_column = self.prev_line_length;
            self.finalize_at(container, header_line - 1, end_column);
        } else {
            let parent = self.tree.get(container).parent;
            self.tree.unlink(container);
            self.tip = parent;
        }

        let table = self.add_child(NodeType::Table, self.next_nonspace);
        // `add_child` both dated and placed the table from the delimiter
        // row, and neither belongs to it: the table begins on the header
        // row, one line above, in the column where *that* row's content
        // starts. The two rows are indented independently, so the delimiter
        // row's column is simply somebody else's: `  a | b` over `| - | -`
        // starts in column 3, and `a | b` over `  | - | -` starts in column
        // 1.
        {
            let node = self.tree.get_mut(table);
            node.sourcepos[0][0] = header_line;
            node.sourcepos[0][1] = header_column;
            node.table_align = Some(align);
        }

        // The row is measured over its own text, exactly as `finalize_table`
        // measures the body rows: `last_line_length` counts the whole source
        // line, container markers included, so inside a block quote it would
        // end the header row two columns past where every body row of the
        // same table ends.
        let end_column = char_count(&self.last_added_line);
        self.add_table_row(table, &header_cells, true, header_line, end_column);
        true
    }

    /// Append one row of `cells` to `table`, padded with empty cells or
    /// truncated so that it is exactly as wide as the delimiter row said.
    ///
    /// Rows and cells are built closed: they are not part of the open
    /// spine, and the line walk in `incorporate_line` must stop at the
    /// table itself.
    fn add_table_row(
        &mut self,
        table: NodeId,
        cells: &[String],
        is_header: bool,
        line_number: usize,
        end_column: usize,
    ) {
        let width = self
            .tree
            .get(table)
            .table_align
            .as_ref()
            .map_or(0, Vec::len);

        // See MAX_AUTOCOMPLETED_CELLS: padding is the only unbounded part of
        // a table.
        let mut limit = width;
        if cells.len() < width {
            let budget = MAX_AUTOCOMPLETED_CELLS.saturating_sub(self.autocompleted_cells);
            let padding = width - cells.len();
            if padding > budget {
                limit = cells.len() + budget;
            }
            self.autocompleted_cells += limit - cells.len();
        }

        // A row and its cells occupy one source line. Columns within that
        // line are not tracked: nothing reads them (the AST drops sourcepos
        // and the renderer ignores it), and threading offsets through the
        // cell split to invent them would be cost with no reader.
        let span: SourcePos = [[line_number, 1], [line_number, end_column]];

        let mut row = MdNode::new(NodeType::TableRow);
        row.sourcepos = span;
        row.is_header_row = is_header;
        row.open = false;
        let row = self.tree.add(row);
        self.tree.append_child(table, row);

        for i in 0..limit {
            let mut cell = MdNode::new(NodeType::TableCell);
            cell.sourcepos = span;
            cell.open = false;
            // Consumed by the inline phase, exactly as a paragraph's content
            // is.
            if i < cells.len() {
                cell.string_content = cells[i].clone();
            }
            let cell = self.tree.add(cell);
            self.tree.append_child(row, cell);
        }
    }

    /// Close a table: turn the raw lines it accumulated into body rows.
    ///
    /// The first accumulated line is the delimiter row that opened the
    /// table. It carries no data (its alignments are already on the node)
    /// so it is skipped here, which is also what keeps the source line
    /// numbering of the rows below it straightforward: the table accepts
    /// exactly one line per source line, and a blank line closes it, so
    /// there are no gaps.
    fn finalize_table(&mut self, table: NodeId) {
        let content = std::mem::take(&mut self.tree.get_mut(table).string_content);
        let header_line = self.tree.get(table).sourcepos[0][0];

        // Line 0 is the delimiter row; the last element is the empty string
        // after the final newline.
        for (i, line) in content.split('\n').enumerate() {
            if i == 0 || line.is_empty() {
                continue;
            }
            let cells = split_table_row(line);
            self.add_table_row(table, &cells, false, header_line + i + 1, char_count(line));
        }
    }

    /// Close every block that failed its continuation condition on this
    /// line.
    fn close_unmatched_blocks(&mut self) {
        if self.all_closed {
            return;
        }
        while self.oldtip != self.last_matched_container {
            let parent = self.tree.get(self.oldtip).parent;
            self.finalize(self.oldtip, self.line_number - 1);
            match parent {
                Some(p) => self.oldtip = p,
                None => break,
            }
        }
        self.all_closed = true;
    }

    /// Close a block, record its end position, and run its finalizer. The
    /// end column is the previous line's length, which is where every block
    /// that ends on `line_number` ends.
    fn finalize(&mut self, block: NodeId, line_number: usize) {
        self.finalize_at(block, line_number, self.last_line_length);
    }

    /// `finalize` with the end column given rather than assumed. Only a
    /// table's paragraph split needs it: that one closes a block two lines
    /// back, where `finalize`'s default is one line too recent.
    fn finalize_at(&mut self, block: NodeId, line_number: usize, end_column: usize) {
        let above = self.tree.get(block).parent;
        {
            let node = self.tree.get_mut(block);
            node.open = false;
            node.sourcepos[1] = [line_number, end_column];
        }
        self.finalize_block(block);
        self.html_block_types.remove(&block);
        self.tip = above;
    }

    /// `block.listData.tight = false` on a list, as the other runtimes see
    /// it. In TypeScript the list and the item that opened it hold the SAME
    /// `listData` object (`addChild('list').listData = data` and then
    /// `addChild('item').listData = data`), and Go shares the pointer the
    /// same way, so loosening the list also loosens that first item's
    /// `listData`. Here `list_data` is an owned value on each node, so the
    /// aliasing has to be spelled out: the native tree (and its goldens)
    /// carry the item's `tight`, and the projection and the renderer read
    /// only the list's. Later items keep their own `tight: true`, in every
    /// runtime.
    fn mark_list_loose(&mut self, list: NodeId) {
        if let Some(data) = self.tree.get_mut(list).list_data.as_mut() {
            data.tight = false;
        }
        if let Some(first) = self.tree.get(list).first_child {
            if let Some(data) = self.tree.get_mut(first).list_data.as_mut() {
                data.tight = false;
            }
        }
    }

    /// The per-type finalizer the TypeScript keeps in `BLOCK_HANDLERS`.
    fn finalize_block(&mut self, block: NodeId) {
        match self.node_type(block) {
            NodeType::List => {
                // Section 5.3: the list is loose if any item is followed by
                // a blank line, or if any item directly contains two blocks
                // separated by one. A blank line at the very end of the
                // last item does not count, hence the `next` guards.
                let mut item = self.tree.get(block).first_child;
                'items: while let Some(it) = item {
                    let item_next = self.tree.get(it).next;
                    if self.ends_with_blank_line(it) && item_next.is_some() {
                        self.mark_list_loose(block);
                        break;
                    }
                    let mut subitem = self.tree.get(it).first_child;
                    while let Some(sub) = subitem {
                        let sub_next = self.tree.get(sub).next;
                        if self.ends_with_blank_line(sub)
                            && (item_next.is_some() || sub_next.is_some())
                        {
                            self.mark_list_loose(block);
                            break 'items;
                        }
                        subitem = sub_next;
                    }
                    item = item_next;
                }
                if let Some(last) = self.tree.get(block).last_child {
                    let end = self.tree.get(last).sourcepos[1];
                    self.tree.get_mut(block).sourcepos[1] = end;
                }
            }

            NodeType::Item => {
                let node = self.tree.get(block);
                if let Some(last) = node.last_child {
                    let end = self.tree.get(last).sourcepos[1];
                    self.tree.get_mut(block).sourcepos[1] = end;
                } else if let Some(data) = &node.list_data {
                    let end = [node.sourcepos[0][0], data.marker_offset + data.padding];
                    self.tree.get_mut(block).sourcepos[1] = end;
                }
            }

            NodeType::CodeBlock => {
                let node = self.tree.get_mut(block);
                if node.is_fenced {
                    // The first accumulated line is the info string, not
                    // content.
                    let content = std::mem::take(&mut node.string_content);
                    match content.find('\n') {
                        Some(nl) => {
                            node.info = Some(unescape_string(js_trim(&content[..nl])));
                            node.literal = content[nl + 1..].to_string();
                        }
                        None => {
                            node.info = Some(unescape_string(js_trim(&content)));
                            node.literal = String::new();
                        }
                    }
                } else {
                    // Section 4.4: trailing blank lines are not part of an
                    // indented code block. This is `.replace(/(\n *)+$/, '\n')`.
                    let content = std::mem::take(&mut node.string_content);
                    let start = trailing_blank_run_start(content.as_bytes());
                    if start < content.len() {
                        node.literal = format!("{}\n", &content[..start]);
                    } else {
                        node.literal = content;
                    }
                }
            }

            NodeType::HtmlBlock => {
                // `.replace(/(\n *)+$/, '')`
                let node = self.tree.get_mut(block);
                let content = std::mem::take(&mut node.string_content);
                let start = trailing_blank_run_start(content.as_bytes());
                node.literal = content[..start].to_string();
            }

            NodeType::Paragraph => {
                // Section 4.7: a paragraph may begin with link reference
                // definitions. Consume them from the front; if nothing is
                // left, the paragraph disappears.
                if self.consume_reference_defs(block)
                    && is_blank(&self.tree.get(block).string_content)
                {
                    self.tree.unlink(block);
                }
            }

            // GFM tables. The header row is built when the table opens (see
            // `try_open_table`); every later line is accumulated raw and
            // split into a body row at finalize, so the table behaves like a
            // paragraph that happens to print as a grid: it ends at a blank
            // line, and any block start interrupts it, because it is not
            // verbatim and `can_contain` refuses everything the block starts
            // can open.
            NodeType::Table => self.finalize_table(block),

            _ => {}
        }
    }

    /// Section 5.3 looseness input: does this block end with a blank line?
    /// For lists and items the question recurses into the last child, since
    /// the blank line is recorded on the deepest block that saw it. The
    /// `last_line_checked` flag makes the recursion linear over the whole
    /// document rather than quadratic.
    fn ends_with_blank_line(&mut self, block: NodeId) -> bool {
        let mut cur = Some(block);
        while let Some(id) = cur {
            let node = self.tree.get_mut(id);
            if node.last_line_blank {
                return true;
            }
            let t = node.node_type;
            if !node.last_line_checked && (t == NodeType::List || t == NodeType::Item) {
                node.last_line_checked = true;
                cur = node.last_child;
            } else {
                node.last_line_checked = true;
                return false;
            }
        }
        false
    }

    /// Strip leading link reference definitions (section 4.7) from a
    /// block's raw content, adding each to the refmap, and report whether
    /// any were consumed.
    fn consume_reference_defs(&mut self, block: NodeId) -> bool {
        let content = std::mem::take(&mut self.tree.get_mut(block).string_content);
        let consumed = parse_references(&content, &mut self.refmap);
        let node = self.tree.get_mut(block);
        if consumed == 0 {
            node.string_content = content;
            return false;
        }
        node.string_content = content[consumed..].to_string();
        true
    }

    /// Can `container` absorb the current line?
    fn continue_block(&mut self, container: NodeId) -> ContinueResult {
        match self.node_type(container) {
            NodeType::Document | NodeType::List => ContinueResult::Matched,

            NodeType::BlockQuote => {
                if !self.indented && peek(&self.current_line, self.next_nonspace) == Some(b'>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    // Section 5.1: one optional space of indentation after
                    // `>`, tab-aware.
                    if is_space_or_tab(peek(&self.current_line, self.offset)) {
                        self.advance_offset(1, true);
                    }
                    return ContinueResult::Matched;
                }
                ContinueResult::NotMatched
            }

            NodeType::Item => {
                let (marker_offset, padding, first_child) = {
                    let node = self.tree.get(container);
                    let Some(data) = &node.list_data else {
                        return ContinueResult::NotMatched;
                    };
                    (data.marker_offset, data.padding, node.first_child)
                };

                if self.blank {
                    // An item whose first line is blank cannot be continued
                    // by a second blank line (section 5.2 rule 3: at most
                    // one leading blank line).
                    if first_child.is_none() {
                        return ContinueResult::NotMatched;
                    }
                    self.advance_next_nonspace();
                    return ContinueResult::Matched;
                }

                // Section 5.2: subsequent lines belong to the item if
                // indented to the content column derived from the marker.
                if self.indent >= marker_offset + padding {
                    self.advance_offset(marker_offset + padding, true);
                    return ContinueResult::Matched;
                }
                ContinueResult::NotMatched
            }

            // A heading is always exactly one line (setext headings are
            // converted from an already-complete paragraph).
            NodeType::Heading | NodeType::ThematicBreak => ContinueResult::NotMatched,

            NodeType::CodeBlock => {
                let indent = self.indent;
                let (is_fenced, fence_char, fence_length, fence_offset) = {
                    let node = self.tree.get(container);
                    (
                        node.is_fenced,
                        node.fence_char,
                        node.fence_length,
                        node.fence_offset,
                    )
                };

                if is_fenced {
                    // Section 4.5: the closing fence is the same character,
                    // at least as long, at most 3 columns indented, and
                    // followed only by spaces or tabs.
                    let mut match_len = 0;
                    if indent <= 3
                        && peek(&self.current_line, self.next_nonspace) == Some(fence_char)
                    {
                        match_len =
                            match_closing_code_fence(&self.current_line[self.next_nonspace..]);
                    }
                    if match_len > 0 && match_len >= fence_length {
                        self.last_line_length =
                            char_count(&self.current_line[..self.offset]) + indent + match_len;
                        self.finalize(container, self.line_number);
                        return ContinueResult::LineDone;
                    }
                    // Content is de-indented by up to the opening fence's own
                    // indent.
                    let mut i = fence_offset;
                    while i > 0 && is_space_or_tab(peek(&self.current_line, self.offset)) {
                        self.advance_offset(1, true);
                        i -= 1;
                    }
                    return ContinueResult::Matched;
                }

                // Section 4.4 indented code: four columns, or a blank line
                // (which is kept).
                if indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                    return ContinueResult::Matched;
                }
                if self.blank {
                    self.advance_next_nonspace();
                    return ContinueResult::Matched;
                }
                ContinueResult::NotMatched
            }

            NodeType::HtmlBlock => {
                // Types 6 and 7 end at a blank line; 1 to 5 end on their
                // closing string, which `incorporate_line` checks after
                // adding the line.
                let kind = self.html_block_type(container);
                if self.blank && (kind == 6 || kind == 7) {
                    return ContinueResult::NotMatched;
                }
                ContinueResult::Matched
            }

            NodeType::Paragraph | NodeType::Table => {
                if self.blank {
                    ContinueResult::NotMatched
                } else {
                    ContinueResult::Matched
                }
            }

            // Rows and cells are created closed, and are never the parser's
            // tip, so nothing here is reached in a normal parse. An inert
            // leaf is the safe reading: no continuation, no children, no
            // lines.
            _ => ContinueResult::NotMatched,
        }
    }

    /// Parse a list marker at `next_nonspace` and derive the item's
    /// content indent.
    ///
    /// The content indent is marker width + the spaces that follow it,
    /// with two exceptions that both fall back to marker width + 1:
    ///
    /// - rule 2, item starting with indented code: 5+ spaces after the
    ///   marker means only the first space belongs to the marker, the rest
    ///   is code;
    /// - rule 3, item starting with a blank line: nothing follows the
    ///   marker at all, so there are no spaces to measure.
    ///
    /// Returns `None` when this is not a list item start. Advances the
    /// parser to the item's content column when it is.
    fn parse_list_marker(&mut self, container: NodeId) -> Option<ListData> {
        // Four columns of indentation is indented code, never a list marker.
        if self.indent >= CODE_INDENT {
            return None;
        }

        let container_is_paragraph = self.node_type(container) == NodeType::Paragraph;
        let rest = &self.current_line[self.next_nonspace..];

        let mut data = ListData {
            list_type: ListType::Bullet,
            tight: true, // lists are tight until finalize proves otherwise
            start: 1,
            delimiter: String::new(),
            bullet_char: String::new(),
            padding: 0,
            marker_offset: self.indent,
        };

        let marker_len;
        if is_bullet_list_marker(rest) {
            data.list_type = ListType::Bullet;
            data.bullet_char = rest[..1].to_string();
            marker_len = 1;
        } else {
            let (digits, delimiter) = match_ordered_list_marker(rest)?;
            // At most nine digits, which always fit.
            let start: i64 = rest[..digits].parse().ok()?;
            // Section 5.2 exception 1(b): an ordered item interrupting a
            // paragraph must start at 1.
            if container_is_paragraph && start != 1 {
                return None;
            }
            data.list_type = ListType::Ordered;
            data.start = start;
            data.delimiter = (delimiter as char).to_string();
            marker_len = digits + 1;
        }

        // At least one space or tab (or the end of the line) must follow the
        // marker.
        let nextc = peek(&self.current_line, self.next_nonspace + marker_len);
        if !(nextc.is_none() || nextc == Some(b'\t') || nextc == Some(b' ')) {
            return None;
        }

        // Section 5.2 exception 1(a): an item interrupting a paragraph may
        // not start blank.
        if container_is_paragraph && is_blank(&self.current_line[self.next_nonspace + marker_len..])
        {
            return None;
        }

        self.advance_next_nonspace(); // to the marker
        self.advance_offset(marker_len, true); // past the marker

        let spaces_start_col = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let nextc = peek(&self.current_line, self.offset);
            if !(self.column - spaces_start_col < 5 && is_space_or_tab(nextc)) {
                break;
            }
        }

        let blank_item = peek(&self.current_line, self.offset).is_none();
        let spaces_after_marker = self.column - spaces_start_col;

        if !(1..5).contains(&spaces_after_marker) || blank_item {
            data.padding = marker_len + 1;
            self.column = spaces_start_col;
            self.offset = spaces_start_offset;
            if is_space_or_tab(peek(&self.current_line, self.offset)) {
                self.advance_offset(1, true);
            }
        } else {
            data.padding = marker_len + spaces_after_marker;
        }

        Some(data)
    }

    /// The block starts, tried in order, and the order is the spec's: a
    /// setext underline beats a thematic break (so `Foo\n---` is a
    /// heading), a thematic break beats a list item (so `- - -` is a
    /// break), and indented code comes last because any of the others may
    /// be indented up to three columns. The GFM table start is last of all;
    /// see `try_open_table`.
    fn block_start(&mut self, index: usize, container: NodeId) -> StartResult {
        match index {
            // Block quote (section 5.1).
            0 => {
                if !self.indented && peek(&self.current_line, self.next_nonspace) == Some(b'>') {
                    // The nesting limit: past it the marker is text. The
                    // holder is judged from `container`, which is what the
                    // tip becomes once `close_unmatched_blocks` has run.
                    if self.depth_of_new(container, NodeType::BlockQuote) > MAX_CONTAINER_NESTING {
                        return StartResult::None;
                    }
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if is_space_or_tab(peek(&self.current_line, self.offset)) {
                        self.advance_offset(1, true);
                    }
                    self.close_unmatched_blocks();
                    self.add_child(NodeType::BlockQuote, self.next_nonspace);
                    return StartResult::Container;
                }
                StartResult::None
            }

            // ATX heading (section 4.2).
            1 => {
                if self.indented {
                    return StartResult::None;
                }
                let Some((level, match_len)) =
                    match_atx_heading_marker(&self.current_line[self.next_nonspace..])
                else {
                    return StartResult::None;
                };

                self.advance_next_nonspace();
                self.advance_offset(match_len, false);
                self.close_unmatched_blocks();

                let heading = self.add_child(NodeType::Heading, self.next_nonspace);
                let content = strip_atx_closing(&self.current_line[self.offset..]).to_string();
                {
                    let node = self.tree.get_mut(heading);
                    node.level = level;
                    node.string_content = content;
                }
                let rest = self.current_line.len() - self.offset;
                self.advance_offset(rest, false);
                StartResult::Leaf
            }

            // Fenced code block (section 4.5).
            2 => {
                if self.indented {
                    return StartResult::None;
                }
                let rest = &self.current_line[self.next_nonspace..];
                let fence_length = match_code_fence(rest);
                if fence_length == 0 {
                    return StartResult::None;
                }
                let fence_char = rest.as_bytes()[0];

                self.close_unmatched_blocks();
                let code = self.add_child(NodeType::CodeBlock, self.next_nonspace);
                {
                    let node = self.tree.get_mut(code);
                    node.is_fenced = true;
                    node.fence_length = fence_length;
                    node.fence_char = fence_char;
                    node.fence_offset = self.indent;
                }
                self.advance_next_nonspace();
                self.advance_offset(fence_length, false);
                // The rest of the line is added as the block's first line:
                // the info string.
                StartResult::Leaf
            }

            // HTML block (section 4.6), types 1 to 7.
            3 => {
                if self.indented || peek(&self.current_line, self.next_nonspace) != Some(b'<') {
                    return StartResult::None;
                }
                let kind = html_block_open_kind(&self.current_line[self.next_nonspace..]);
                if kind == 0 {
                    return StartResult::None;
                }
                // Type 7 may not interrupt a paragraph, including a
                // paragraph we are only about to continue lazily.
                if kind == 7
                    && (self.node_type(container) == NodeType::Paragraph
                        || (!self.all_closed
                            && !self.blank
                            && self.node_type(self.open_tip()) == NodeType::Paragraph))
                {
                    return StartResult::None;
                }
                self.close_unmatched_blocks();
                // The offset is deliberately not advanced: leading spaces
                // are part of the raw HTML.
                let block = self.add_child(NodeType::HtmlBlock, self.offset);
                self.set_html_block_type(block, kind);
                StartResult::Leaf
            }

            // Setext heading underline (section 4.3): converts the whole
            // open paragraph.
            4 => {
                if self.indented || self.node_type(container) != NodeType::Paragraph {
                    return StartResult::None;
                }
                let Some(underline) =
                    match_setext_heading_line(&self.current_line[self.next_nonspace..])
                else {
                    return StartResult::None;
                };

                self.close_unmatched_blocks();

                // Section 4.7: the paragraph may still have led with
                // reference definitions; only what remains becomes the
                // heading. If nothing remains this is not a setext underline
                // at all (it falls through to thematic break / paragraph).
                self.consume_reference_defs(container);
                if self.tree.get(container).string_content.is_empty() {
                    return StartResult::None;
                }

                let start = self.tree.get(container).sourcepos[0];
                let content = std::mem::take(&mut self.tree.get_mut(container).string_content);
                let mut heading = MdNode::new(NodeType::Heading);
                heading.sourcepos = [[start[0], start[1]], [0, 0]];
                heading.level = if underline == b'=' { 1 } else { 2 };
                heading.string_content = content;
                let heading = self.tree.add(heading);
                self.tree.insert_after(container, heading);
                self.tree.unlink(container);
                self.tip = Some(heading);
                let rest = self.current_line.len() - self.offset;
                self.advance_offset(rest, false);
                StartResult::Leaf
            }

            // Thematic break (section 4.1).
            5 => {
                if self.indented {
                    return StartResult::None;
                }
                if !is_thematic_break(&self.current_line[self.next_nonspace..]) {
                    return StartResult::None;
                }
                self.close_unmatched_blocks();
                self.add_child(NodeType::ThematicBreak, self.next_nonspace);
                let rest = self.current_line.len() - self.offset;
                self.advance_offset(rest, false);
                StartResult::Leaf
            }

            // List item (section 5.2). `parse_list_marker` rejects a marker
            // indented four or more columns on its own, so there is no
            // `indented` test here.
            6 => {
                // `parse_list_marker` moves the cursor when it matches, so
                // the position is kept for the nesting limit below, which
                // has to leave the line exactly as it found it.
                let before = (self.offset, self.column, self.partially_consumed_tab);
                let Some(data) = self.parse_list_marker(container) else {
                    return StartResult::None;
                };

                // Section 5.3: a change of bullet character or ordered
                // delimiter starts a new list. Judged from `container`,
                // which is what the tip becomes once
                // `close_unmatched_blocks` has run.
                let same_list = self.node_type(container) == NodeType::List
                    && self
                        .tree
                        .get(container)
                        .list_data
                        .as_ref()
                        .is_some_and(|existing| lists_match(existing, &data));

                // The nesting limit: an item in the open list is one
                // container deeper, a new list and its item are two. Past
                // it the marker is text.
                let depth = if same_list {
                    self.depth_of_new(container, NodeType::Item)
                } else {
                    self.depth_of_new(container, NodeType::List) + 1
                };
                if depth > MAX_CONTAINER_NESTING {
                    (self.offset, self.column, self.partially_consumed_tab) = before;
                    return StartResult::None;
                }

                self.close_unmatched_blocks();

                let tip = self.open_tip();
                debug_assert_eq!(tip, container, "the tip is the last matched container");
                if !same_list {
                    let list = self.add_child(NodeType::List, self.next_nonspace);
                    self.tree.get_mut(list).list_data = Some(data.clone());
                }

                let item = self.add_child(NodeType::Item, self.next_nonspace);
                self.tree.get_mut(item).list_data = Some(data);
                StartResult::Container
            }

            // Indented code block (section 4.4): may not interrupt a
            // paragraph.
            7 => {
                if !self.indented
                    || self.node_type(self.open_tip()) == NodeType::Paragraph
                    || self.blank
                {
                    return StartResult::None;
                }
                self.advance_offset(CODE_INDENT, true);
                self.close_unmatched_blocks();
                self.add_child(NodeType::CodeBlock, self.offset);
                StartResult::Leaf
            }

            // GFM table (extension): a delimiter row directly under an open
            // paragraph.
            //
            // Last, and the position is load-bearing in both directions. It
            // must come after the setext underline so that `foo` over `---`
            // stays an `<h2>` rather than becoming a one-column table, and
            // after the list marker so that `- | -` stays a list item;
            // cmark-gfm reaches its extensions from the same place, once
            // every built-in start has refused the line.
            8 => {
                if self.try_open_table(container) {
                    StartResult::Leaf
                } else {
                    StartResult::None
                }
            }

            _ => StartResult::None,
        }
    }

    /// How many block starts `block_start` knows.
    const BLOCK_STARTS: usize = 9;

    /// Incorporate one line into the tree: the three steps of Appendix A.
    pub fn incorporate_line(&mut self, ln: &str) {
        let mut all_matched = true;
        let mut container = self.doc;

        // `last_line_length` still measures the previous line here.
        // Captured before anything can overwrite it (an HTML block closing
        // mid-line does) and installed as `prev_line_length` at whichever
        // exit updates `last_line_length`, so the two always describe
        // consecutive lines.
        let length_before_this_line = self.last_line_length;

        self.oldtip = self.open_tip();
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.line_number += 1;
        self.current_line.clear();
        self.current_line.push_str(ln);
        // The column anchor memoizes offsets into `current_line`; a new line
        // invalidates it. See `source_column`.
        self.col_anchor_offset = 0;
        self.col_anchor_chars = 0;

        // Step 1: walk the open blocks, consuming continuation markers.
        // Stop at the first block that does not match; `container` is then
        // the last matched one.
        let mut last_child = self.tree.get(container).last_child;
        while let Some(child) = last_child {
            if !self.tree.get(child).open {
                break;
            }
            container = child;
            self.find_next_nonspace();

            match self.continue_block(container) {
                ContinueResult::NotMatched => all_matched = false,
                // A fenced code block was closed by this line; nothing else
                // to do.
                ContinueResult::LineDone => return,
                ContinueResult::Matched => {}
            }

            if !all_matched {
                if let Some(parent) = self.tree.get(container).parent {
                    container = parent;
                }
                break;
            }

            last_child = self.tree.get(container).last_child;
        }

        self.all_closed = container == self.oldtip;
        self.last_matched_container = container;

        // Step 2: look for new block starts under the last matched
        // container, unless its lines are literal text, in which case there
        // is nothing to look for.
        let mut matched_leaf = verbatim(self.node_type(container));

        while !matched_leaf {
            self.find_next_nonspace();

            let special = if self.gfm_special {
                maybe_special_gfm(&self.current_line, self.next_nonspace)
            } else {
                maybe_special(&self.current_line, self.next_nonspace)
            };
            if !self.indented && !special {
                self.advance_next_nonspace();
                break;
            }

            let mut i = 0;
            while i < Self::BLOCK_STARTS {
                match self.block_start(i, container) {
                    StartResult::Container => {
                        container = self.open_tip();
                        break;
                    }
                    StartResult::Leaf => {
                        container = self.open_tip();
                        matched_leaf = true;
                        break;
                    }
                    StartResult::None => i += 1,
                }
            }

            if i == Self::BLOCK_STARTS {
                // Nothing matched: the rest of the line is plain text.
                self.advance_next_nonspace();
                break;
            }
        }

        // Step 3: whatever is left of the line is text for the deepest open
        // block.
        if !self.all_closed && !self.blank && self.node_type(self.open_tip()) == NodeType::Paragraph
        {
            // Lazy continuation (section 5.1 rule 2): the markers are
            // missing but an open paragraph absorbs the line anyway, and
            // its containers stay open.
            self.add_line();
            self.prev_line_length = length_before_this_line;
            self.last_line_length = char_count(ln);
            return;
        }

        self.close_unmatched_blocks();

        if self.blank {
            if let Some(last) = self.tree.get(container).last_child {
                self.tree.get_mut(last).last_line_blank = true;
            }
        }

        let t = self.node_type(container);

        // A block quote line is never blank (it has a `>`), blank lines
        // inside fenced code do not affect list looseness, and an item's own
        // first blank line is part of the item, not a separator.
        let last_line_blank = self.blank && {
            let node = self.tree.get(container);
            !(t == NodeType::BlockQuote
                || (t == NodeType::CodeBlock && node.is_fenced)
                || (t == NodeType::Item
                    && node.first_child.is_none()
                    && node.sourcepos[0][0] == self.line_number))
        };

        // Propagate up: a container whose last line was blank ends with a
        // blank line.
        let mut cont = Some(container);
        while let Some(id) = cont {
            let node = self.tree.get_mut(id);
            node.last_line_blank = last_line_blank;
            cont = node.parent;
        }

        if accepts_lines(t) {
            self.add_line();
            // Section 4.6: HTML block types 1 to 5 end on the line that
            // contains their closing string, which may be the line that
            // opened them.
            if t == NodeType::HtmlBlock {
                let kind = self.html_block_type(container);
                if (1..=5).contains(&kind)
                    && html_block_closes(&self.current_line[self.offset..], kind)
                {
                    self.last_line_length = char_count(ln);
                    self.finalize(container, self.line_number);
                }
            }
        } else if self.offset < self.current_line.len() && !self.blank {
            // Nowhere else for the text to go: open a paragraph for it.
            self.add_child(NodeType::Paragraph, self.offset);
            self.advance_next_nonspace();
            self.add_line();
        }

        self.prev_line_length = length_before_this_line;
        self.last_line_length = char_count(ln);
    }

    /// Feed every line of `input` and finish. An empty document still
    /// incorporates one empty line, exactly as the historical split-based
    /// loop did: line count and sourcepos depend on it. Line cutting and
    /// section 2.3 NUL replacement both live in `segment_next_line`.
    pub fn parse(mut self, input: &str) -> (Tree, RefMap) {
        if input.is_empty() {
            self.incorporate_line("");
        } else {
            let mut pos = 0;
            while let Some(seg) = segment_next_line(input, pos) {
                self.incorporate_line(&seg.text);
                pos = seg.next;
            }
        }
        self.finish()
    }

    /// Close every still-open block, stamp the parse-time `gfm` flag, and
    /// run the task-list post-pass. `line_number` is the count of
    /// incorporated lines, so an incremental driver (one that feeds
    /// `incorporate_line` itself instead of calling `parse`) finishes with
    /// exactly the same finalization the batch loop gets.
    pub fn finish(mut self) -> (Tree, RefMap) {
        while let Some(tip) = self.tip {
            self.finalize(tip, self.line_number);
        }

        self.tree.get_mut(self.doc).gfm = self.options.gfm;
        if self.options.gfm {
            mark_task_list_items(&mut self.tree, self.doc);
        }

        (self.tree, self.refmap)
    }
}

// --- GFM task list items ----------------------------------------------------

/// The task list item marker: optional spaces, `[`, either a whitespace
/// character or `x`/`X`, `]`, and then at least one whitespace character
/// before any other content. Returns the character between the brackets
/// and the byte length of the whole marker.
///
/// The trailing whitespace is required by the extension ("...and at least
/// one whitespace character before any other content"), so `- [x]` with
/// nothing after it is an ordinary item whose text is `[x]`. It is matched
/// as a space or a tab rather than as any whitespace: a line ending there
/// would mean the content starts on the *next* line, and consuming it would
/// swallow the paragraph's first soft break.
pub fn task_list_marker(s: &str) -> Option<(u8, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'[' {
        return None;
    }
    i += 1;
    if i >= bytes.len() {
        return None;
    }
    let state = bytes[i];
    if state != b' ' && state != b'\t' && state != b'x' && state != b'X' {
        return None;
    }
    i += 1;
    if i >= bytes.len() || bytes[i] != b']' {
        return None;
    }
    i += 1;

    let spaces = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i == spaces {
        return None;
    }
    Some((state, i))
}

/// GFM task list items: mark every item whose first block is a paragraph
/// beginning with a task list item marker, and consume the marker so it
/// does not survive into the text.
///
/// Run at the end of the block phase, over `string_content`, before the
/// inline phase has seen it. That is what makes the marker win over
/// anything the inline scanner would otherwise make of the brackets: with
/// a `[x]: /url` definition in the document, `- [x] foo` is still a task
/// item rather than a link. It also means the item's checked state is
/// settled before `ast.rs` or `html.rs` looks at the tree.
///
/// Iterative rather than recursive: container nesting depth is chosen by
/// the input, and `"> ".repeat(20000)` is a document this parser otherwise
/// handles.
fn mark_task_list_items(tree: &mut Tree, doc: NodeId) {
    let mut stack = vec![doc];

    while let Some(node) = stack.pop() {
        let t = tree.get(node).node_type;

        if t == NodeType::Item {
            if let Some(first) = tree.get(node).first_child {
                if tree.get(first).node_type == NodeType::Paragraph {
                    if let Some((state, length)) = task_list_marker(&tree.get(first).string_content)
                    {
                        // Whitespace between the brackets is unchecked;
                        // `x`/`X` is checked.
                        tree.get_mut(node).checked = Some(state != b' ' && state != b'\t');
                        let content = &mut tree.get_mut(first).string_content;
                        content.drain(..length);
                    }
                }
            }
        }

        if matches!(
            t,
            NodeType::Document | NodeType::BlockQuote | NodeType::List | NodeType::Item
        ) {
            let mut c = tree.get(node).last_child;
            while let Some(id) = c {
                stack.push(id);
                c = tree.get(id).prev;
            }
        }
    }
}

/// Phase 1: build the block tree and collect link reference definitions.
/// Paragraph and heading text is left raw in `string_content` for phase 2.
pub fn parse_blocks(input: &str, options: Options) -> (Tree, RefMap) {
    BlockParser::new(options).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atx_closing_sequences() {
        assert_eq!(strip_atx_closing("Hello ###"), "Hello");
        assert_eq!(strip_atx_closing("###"), "");
        assert_eq!(strip_atx_closing("Hello #x"), "Hello #x");
        assert_eq!(strip_atx_closing("Hello ###  "), "Hello");
        assert_eq!(strip_atx_closing("a ## b ##"), "a ## b");
        assert_eq!(strip_atx_closing("foo#"), "foo#");
        assert_eq!(strip_atx_closing("  ###  "), "");
        assert_eq!(strip_atx_closing(""), "");
    }

    #[test]
    fn trailing_blank_runs() {
        assert_eq!(trailing_blank_run_start(b"a\n  \n"), 1);
        assert_eq!(trailing_blank_run_start(b"a\nb"), 3);
        assert_eq!(trailing_blank_run_start(b""), 0);
    }
}
