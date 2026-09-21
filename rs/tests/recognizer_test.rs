// Pin tests for the pure recognizers in block.rs and inline.rs. The
// recognizers are the single home of each construct's recognition logic:
// the hand drivers delegate to them, and any other driver over the same
// syntax is expected to share them rather than reimplement them.
// ts/test/recognizer.test.ts and go/recognizer_test.go pin the same
// cases; the shapes differ where Rust idiom differs (`Option` and tuples
// for TS object results), the behavior may not.

use tabnas_markdown::block::{
    html_block_closes, html_block_open_kind, is_blank, is_bullet_list_marker, is_thematic_break,
    match_atx_heading_marker, match_closing_code_fence, match_code_fence,
    match_ordered_list_marker, match_setext_heading_line, parse_delimiter_row, segment_next_line,
    split_table_row, task_list_marker, LineSegment,
};
use tabnas_markdown::inline::{
    classify_break, code_point_at, code_point_before, match_spnl, scan_angle_autolink,
    scan_code_span, scan_delimiter_run, scan_entity, scan_escape, scan_html_tag,
    scan_inline_link_tail, scan_link_destination, scan_link_label, scan_link_title,
    skip_initial_spaces, AngleAutolinkScan, CodeSpanScan, DelimScan, EscapeKind, EscapeScan,
    LinkTailScan,
};
use tabnas_markdown::Align;

fn seg(text: &str, next: usize) -> Option<LineSegment> {
    Some(LineSegment {
        text: text.to_string(),
        next,
    })
}

#[test]
fn segment_next_line_cuts_physical_lines() {
    assert_eq!(segment_next_line("a\nb", 0), seg("a", 2), "LF line");
    assert_eq!(segment_next_line("a\nb", 2), seg("b", 3), "last line");
    assert_eq!(segment_next_line("a\nb", 3), None, "past end");
    // A final line ending does not introduce a trailing blank line.
    assert_eq!(segment_next_line("a\n", 0), seg("a", 2), "final newline");
    assert_eq!(segment_next_line("a\n", 2), None, "after final newline");
    // \r\n is one terminator; a lone \r is one too.
    assert_eq!(segment_next_line("a\r\nb", 0), seg("a", 3), "CRLF");
    assert_eq!(segment_next_line("a\rb", 0), seg("a", 2), "CR");
    // Blank line between terminators.
    assert_eq!(segment_next_line("a\n\nb", 2), seg("", 3), "blank line");
    // §2.3: NUL is replaced with U+FFFD.
    assert_eq!(
        segment_next_line("a\0b", 0),
        seg("a\u{fffd}b", 3),
        "NUL replacement"
    );
    // Empty source has no lines at all (the driver special-cases it).
    assert_eq!(segment_next_line("", 0), None, "empty source");
}

#[test]
fn block_line_recognizers() {
    assert!(
        is_blank("") && is_blank(" \t ") && !is_blank(" x"),
        "is_blank"
    );

    assert!(
        is_thematic_break("***") && is_thematic_break("- - -") && is_thematic_break("_ _ _ _"),
        "is_thematic_break accepts"
    );
    assert!(
        !is_thematic_break("**") && !is_thematic_break("*-*"),
        "is_thematic_break rejects"
    );

    assert_eq!(
        match_atx_heading_marker("# Hello"),
        Some((1, 2)),
        "atx '# Hello'"
    );
    assert_eq!(
        match_atx_heading_marker("###### x"),
        Some((6, 7)),
        "atx six"
    );
    assert_eq!(match_atx_heading_marker("##"), Some((2, 2)), "atx bare");
    assert_eq!(match_atx_heading_marker("####### x"), None, "atx seven");
    assert_eq!(match_atx_heading_marker("#x"), None, "atx no space");

    assert!(
        match_code_fence("```js") == 3 && match_code_fence("~~~~") == 4,
        "match_code_fence accepts"
    );
    // A backtick info string may not contain a backtick.
    assert!(
        match_code_fence("``` a`b") == 0 && match_code_fence("``") == 0,
        "match_code_fence rejects"
    );

    assert!(
        match_closing_code_fence("```") == 3 && match_closing_code_fence("````  ") == 4,
        "match_closing_code_fence accepts"
    );
    assert_eq!(
        match_closing_code_fence("``` x"),
        0,
        "match_closing_code_fence rejects"
    );

    assert!(
        match_setext_heading_line("===") == Some(b'=')
            && match_setext_heading_line("-  ") == Some(b'-'),
        "setext accepts"
    );
    assert!(
        match_setext_heading_line("=-").is_none() && match_setext_heading_line("x").is_none(),
        "setext rejects"
    );

    for (s, want) in [
        ("<script>", 1),
        ("<!-- c -->", 2),
        ("<?php", 3),
        ("<!DOCTYPE html>", 4),
        ("<![CDATA[x", 5),
        ("<div>", 6),
        ("<x-tag>", 7),
        ("plain", 0),
    ] {
        assert_eq!(html_block_open_kind(s), want, "html_block_open_kind({s:?})");
    }

    assert!(
        html_block_closes("x</script>y", 1)
            && html_block_closes("x -->", 2)
            && html_block_closes("?>", 3)
            && html_block_closes(">", 4)
            && html_block_closes("]]>", 5),
        "html_block_closes accepts"
    );
    assert!(!html_block_closes("x", 2), "html_block_closes rejects");

    assert!(
        is_bullet_list_marker("- x") && is_bullet_list_marker("+ x") && !is_bullet_list_marker("x"),
        "is_bullet_list_marker"
    );
    assert_eq!(
        match_ordered_list_marker("1. x"),
        Some((1, b'.')),
        "ordered 1."
    );
    assert_eq!(
        match_ordered_list_marker("123456789) x"),
        Some((9, b')')),
        "ordered nine digits"
    );
    // §5.2: at most nine digits.
    assert_eq!(
        match_ordered_list_marker("1234567890. x"),
        None,
        "ordered ten digits"
    );

    assert_eq!(
        task_list_marker("[x] done"),
        Some((b'x', 4)),
        "task marker checked"
    );
    assert_eq!(
        task_list_marker("[ ] todo"),
        Some((b' ', 4)),
        "task marker unchecked"
    );
    // The trailing whitespace is required: `[x]` alone is ordinary text.
    assert_eq!(task_list_marker("[x]"), None, "task marker bare");
    assert_eq!(task_list_marker("[y] no"), None, "task marker bad state");

    assert_eq!(
        parse_delimiter_row("| :-- | --: | :-: | --- |"),
        Some(vec![
            Some(Align::Left),
            Some(Align::Right),
            Some(Align::Center),
            None
        ]),
        "parse_delimiter_row"
    );
    assert_eq!(
        parse_delimiter_row("| a |"),
        None,
        "parse_delimiter_row rejects"
    );
    assert_eq!(
        split_table_row("| a | b |"),
        vec!["a", "b"],
        "split_table_row"
    );
    // `\|` is a literal pipe, resolved at split time.
    assert_eq!(
        split_table_row("a \\| b"),
        vec!["a | b"],
        "split_table_row escape"
    );
}

#[test]
fn inline_recognizers() {
    assert_eq!(
        scan_code_span("`a`", 0),
        Some(CodeSpanScan::Closed {
            end: 3,
            literal: "a".to_string()
        }),
        "code span"
    );
    // One leading and one trailing space strip together.
    assert_eq!(
        scan_code_span("` `` `", 0),
        Some(CodeSpanScan::Closed {
            end: 6,
            literal: "``".to_string()
        }),
        "code span strip"
    );
    // Unequal runs leave the opener literal.
    assert_eq!(
        scan_code_span("``a`", 0),
        Some(CodeSpanScan::Open {
            end: 2,
            ticks: "``".to_string()
        }),
        "code span open"
    );
    assert_eq!(scan_code_span("a`", 0), None, "code span none");

    let escape = |kind, literal: &str, end| EscapeScan {
        kind,
        literal: literal.to_string(),
        end,
    };
    assert_eq!(
        scan_escape("\\*", 0),
        escape(EscapeKind::Char, "*", 2),
        "escape char"
    );
    assert_eq!(
        scan_escape("\\a", 0),
        escape(EscapeKind::Literal, "\\", 1),
        "escape literal"
    );
    assert_eq!(
        scan_escape("\\\n  x", 0),
        escape(EscapeKind::Linebreak, "", 4),
        "escape linebreak"
    );
    assert_eq!(
        scan_escape("\\", 0),
        escape(EscapeKind::Literal, "\\", 1),
        "escape trailing"
    );

    assert_eq!(
        scan_angle_autolink("<http://x.y>", 0),
        Some(AngleAutolinkScan {
            dest: "http://x.y".to_string(),
            label: "http://x.y".to_string(),
            end: 12
        }),
        "uri autolink"
    );
    assert_eq!(
        scan_angle_autolink("<a@b.co>", 0),
        Some(AngleAutolinkScan {
            dest: "mailto:a@b.co".to_string(),
            label: "a@b.co".to_string(),
            end: 8
        }),
        "email autolink"
    );
    assert_eq!(
        scan_angle_autolink("<not a link>", 0),
        None,
        "autolink none"
    );

    assert_eq!(
        scan_html_tag("<em class=\"x\">", 0),
        Some(("<em class=\"x\">".to_string(), 14)),
        "html tag"
    );
    assert_eq!(scan_html_tag("<1bad>", 0), None, "html tag none");
    assert_eq!(
        scan_entity("&amp;", 0),
        Some(("&".to_string(), 5)),
        "entity named"
    );
    assert_eq!(
        scan_entity("&#65;", 0),
        Some(("A".to_string(), 5)),
        "entity numeric"
    );
    // An unknown named reference matches the entity shape but decodes to
    // itself: CommonMark leaves it as literal text.
    assert_eq!(
        scan_entity("&nosuch;", 0),
        Some(("&nosuch;".to_string(), 8)),
        "entity unknown"
    );
    assert_eq!(scan_entity("&;", 0), None, "entity none");

    assert_eq!(
        scan_delimiter_run("*a*", 0, b'*'),
        Some(DelimScan {
            numdelims: 1,
            can_open: true,
            can_close: false
        }),
        "delim open"
    );
    assert_eq!(
        scan_delimiter_run("*a*", 2, b'*'),
        Some(DelimScan {
            numdelims: 1,
            can_open: false,
            can_close: true
        }),
        "delim close"
    );
    // `_` may not open inside a word: snake_case stays intact.
    assert_eq!(
        scan_delimiter_run("snake_case", 5, b'_'),
        Some(DelimScan {
            numdelims: 1,
            can_open: false,
            can_close: false
        }),
        "delim snake"
    );
    assert_eq!(scan_delimiter_run("ab", 0, b'*'), None, "delim none");

    assert_eq!(classify_break(Some("foo  ")), (true, true), "hard break");
    assert_eq!(classify_break(Some("foo ")), (false, true), "soft trim");
    assert_eq!(classify_break(Some("foo")), (false, false), "soft plain");
    assert_eq!(classify_break(None), (false, false), "no prev");

    assert_eq!(
        scan_link_title("\"t\\\"x\"", 0),
        Some(("t\"x".to_string(), 6)),
        "link title"
    );
    assert_eq!(scan_link_title("x", 0), None, "link title none");
    assert_eq!(
        scan_link_destination("<u v>", 0),
        Some(("u v".to_string(), 5)),
        "braced destination"
    );
    assert_eq!(
        scan_link_destination("/a(b)c d", 0),
        Some(("/a(b)c".to_string(), 6)),
        "bare destination"
    );
    // Unbalanced parens are not a destination.
    assert_eq!(
        scan_link_destination("a(b", 0),
        None,
        "destination unbalanced"
    );

    assert_eq!(scan_link_label("[ref]:", 0), Some((5, 5)), "link label");
    assert_eq!(scan_link_label("[a[b]", 0), None, "link label nested");
    // Over the §4.7 limit: consumed but invalid.
    let long = format!("[{}]", "x".repeat(1000));
    assert_eq!(
        scan_link_label(&long, 0),
        Some((1002, 0)),
        "link label too long"
    );

    assert_eq!(
        scan_inline_link_tail("(/u \"t\")", 0),
        Some(LinkTailScan {
            dest: "/u".to_string(),
            title: Some("t".to_string()),
            end: 8
        }),
        "tail titled"
    );
    assert_eq!(
        scan_inline_link_tail("(/u)", 0),
        Some(LinkTailScan {
            dest: "/u".to_string(),
            title: None,
            end: 4
        }),
        "tail bare"
    );
    // A title must be separated from the destination by whitespace.
    assert_eq!(
        scan_inline_link_tail("(/u\"t\")", 0),
        Some(LinkTailScan {
            dest: "/u\"t\"".to_string(),
            title: None,
            end: 7
        }),
        "tail glued"
    );
    assert!(
        scan_inline_link_tail("(/u", 0).is_none() && scan_inline_link_tail("x", 0).is_none(),
        "tail none"
    );

    assert_eq!(skip_initial_spaces("  x", 0), 2, "skip_initial_spaces");
    assert_eq!(match_spnl("  \n  x", 0), 5, "match_spnl");
    // At most one line ending.
    assert_eq!(match_spnl("\n\nx", 0), 1, "match_spnl two endings");
    assert!(
        code_point_before("a\u{1D11E}", 5) == '\u{1D11E}' && code_point_before("x", 0) == '\n',
        "code_point_before"
    );
    assert!(
        code_point_at("\u{1D11E}a", 0) == '\u{1D11E}' && code_point_at("x", 1) == '\n',
        "code_point_at"
    );
}
