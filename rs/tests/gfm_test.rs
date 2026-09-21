// GFM extension conformance and behaviour, the Rust twin of the GFM half
// of ts/test/commonmark.test.ts and of go/gfm_test.go.
//
// Five extensions are implemented (tables, strikethrough, task list
// items, autolink literals and the disallowed-raw-HTML filter) and all
// five are gated on the single GFM option.
//
// The corpus is test/gfm/spec.json, the same 24 extension examples
// ts/tools/gfm-conformance.mjs reports on, so the runtimes are held to
// one standard rather than to each other. Run just the table with:
//
//	cargo test --test gfm_test gfm_spec -- --nocapture

mod common;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use common::{best_of, load_gfm_cases, to_json, CM_OPTS, GFM_OPTS};
use serde_json::json;
use tabnas_markdown::{
    parse_document, parse_tree, render_html, render_html_with, to_html, NodeType, Options,
    SourcePos, DISALLOWED_TAGS,
};

/// Every vendored section must be implemented.
const IMPLEMENTED_SECTIONS: [&str; 5] = [
    "Tables (extension)",
    "Task list items (extension)",
    "Strikethrough (extension)",
    "Autolinks (extension)",
    "Disallowed Raw HTML (extension)",
];

#[derive(Default)]
struct Tally {
    pass: usize,
    total: usize,
}

#[test]
fn gfm_spec() {
    let cases = load_gfm_cases();

    let mut by_section: BTreeMap<String, Tally> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut failures = Vec::new();

    for c in &cases {
        if !by_section.contains_key(&c.section) {
            order.push(c.section.clone());
        }
        let tally = by_section.entry(c.section.clone()).or_default();
        tally.total += 1;

        let actual = to_html(&c.markdown, &GFM_OPTS);
        if actual == c.html {
            tally.pass += 1;
            continue;
        }
        failures.push(format!(
            "example {} [{}]\n  markdown: {:?}\n  expected: {:?}\n  actual:   {:?}",
            c.example, c.section, c.markdown, c.html, actual
        ));
    }

    for name in IMPLEMENTED_SECTIONS {
        assert!(
            by_section.contains_key(name),
            "corpus has no {name:?} section"
        );
    }

    let mut total = 0;
    let mut passed = 0;
    order.sort_by(|a, b| {
        let ta = &by_section[a];
        let tb = &by_section[b];
        let ra = ta.pass as f64 / ta.total as f64;
        let rb = tb.pass as f64 / tb.total as f64;
        ra.partial_cmp(&rb).expect("ratios are finite")
    });
    for name in &order {
        let tally = &by_section[name];
        total += tally.total;
        passed += tally.pass;
        let mark = if tally.pass == tally.total {
            "OK "
        } else {
            "   "
        };
        println!("  {mark} {name:<40} {:>3}/{:<3}", tally.pass, tally.total);
    }
    println!("  TOTAL {passed}/{total}");

    assert!(
        failures.is_empty(),
        "{} of {} examples failed:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
    assert_eq!(passed, 24, "the full corpus must pass");
}

// --- tables -----------------------------------------------------------------
//
// A table's HTML is verbose enough that a literal expectation buries the
// thing being tested, so rows are assembled from their cells here.
// Newline placement is still byte-exact: that is what the corpus judges.

fn trow(cells: &[String]) -> String {
    format!("<tr>\n{}\n</tr>\n", cells.join("\n"))
}

fn thead(cells: &[String]) -> String {
    format!("<thead>\n{}</thead>\n", trow(cells))
}

fn tbody(rows: &[Vec<String>]) -> String {
    let mut out = String::from("<tbody>\n");
    for row in rows {
        out.push_str(&trow(row));
    }
    out.push_str("</tbody>\n");
    out
}

/// The <thead> block and zero or one <tbody> block: a table with no body
/// rows has no <tbody> at all.
fn table_html(head: &str, body: &str) -> String {
    format!("<table>\n{head}{body}</table>\n")
}

fn th(s: &str) -> String {
    format!("<th>{s}</th>")
}

fn th_align(s: &str, align: &str) -> String {
    format!("<th align=\"{align}\">{s}</th>")
}

fn td(s: &str) -> String {
    format!("<td>{s}</td>")
}

fn td_align(s: &str, align: &str) -> String {
    format!("<td align=\"{align}\">{s}</td>")
}

fn check(src: &str, want: &str, why: &str) {
    let got = to_html(src, &GFM_OPTS);
    assert_eq!(got, want, "{why}: {src:?}");
}

// --- the eight spec behaviours, each asserted on its own ---

#[test]
fn gfm_tables_spec_behaviours() {
    // 1. header row, delimiter row, data row.
    check(
        "| foo | bar |\n| --- | --- |\n| baz | bim |\n",
        &table_html(
            &thead(&[th("foo"), th("bar")]),
            &tbody(&[vec![td("baz"), td("bim")]]),
        ),
        "1. header, delimiter, data",
    );

    // 2. colons set alignment, and pipes may be inconsistent. Cell widths
    // need not match, and a leading/trailing pipe is optional on every
    // row independently.
    check(
        "| abc | defghi |\n:-: | -----------:\nbar | baz\n",
        &table_html(
            &thead(&[th_align("abc", "center"), th_align("defghi", "right")]),
            &tbody(&[vec![td_align("bar", "center"), td_align("baz", "right")]]),
        ),
        "2. alignment and inconsistent pipes",
    );
    // All four delimiter shapes, and "none" means no attribute at all.
    check(
        "| a | b | c | d |\n| :- | -: | :-: | --- |\n| 1 | 2 | 3 | 4 |\n",
        &table_html(
            &thead(&[
                th_align("a", "left"),
                th_align("b", "right"),
                th_align("c", "center"),
                th("d"),
            ]),
            &tbody(&[vec![
                td_align("1", "left"),
                td_align("2", "right"),
                td_align("3", "center"),
                td("4"),
            ]]),
        ),
        "2. all four delimiter shapes",
    );

    // 3. an escaped pipe is content, inside other inline spans too.
    check(
        "| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| b **\\|** im |\n",
        &table_html(
            &thead(&[th("f|oo")]),
            &tbody(&[
                vec![td("b <code>|</code> az")],
                vec![td("b <strong>|</strong> im")],
            ]),
        ),
        "3. escaped pipes",
    );

    // 4. another block-level structure breaks the table.
    check(
        "| abc | def |\n| --- | --- |\n| bar | baz |\n> bar\n",
        &format!(
            "{}<blockquote>\n<p>bar</p>\n</blockquote>\n",
            table_html(
                &thead(&[th("abc"), th("def")]),
                &tbody(&[vec![td("bar"), td("baz")]]),
            )
        ),
        "4. a block quote breaks the table",
    );

    // 5. a blank line breaks the table.
    check(
        "| abc | def |\n| --- | --- |\n| bar | baz |\nbar\n\nbar\n",
        &format!(
            "{}<p>bar</p>\n",
            table_html(
                &thead(&[th("abc"), th("def")]),
                &tbody(&[vec![td("bar"), td("baz")], vec![td("bar"), td("")]]),
            )
        ),
        "5. a blank line breaks the table",
    );

    // 6. header and delimiter must agree on the cell count; otherwise
    // there is no table at all and the whole thing stays one paragraph.
    check(
        "| abc | def |\n| --- |\n| bar |\n",
        "<p>| abc | def |\n| --- |\n| bar |</p>\n",
        "6. cell counts disagree",
    );
    // The mismatch is on the count, not the widths: 1-vs-1 matches.
    check(
        "| abc |\n| --- |\n",
        &table_html(&thead(&[th("abc")]), ""),
        "6. 1-vs-1",
    );
    let got = to_html("| abc | def |\n| --- | --- | --- |\n", &GFM_OPTS);
    assert!(
        got.starts_with("<p>"),
        "6. 2-vs-3 should stay a paragraph, got {got:?}"
    );

    // 7. short rows are padded, long rows are truncated.
    check(
        "| abc | def |\n| --- | --- |\n| bar |\n| bar | baz | boo |\n",
        &table_html(
            &thead(&[th("abc"), th("def")]),
            &tbody(&[vec![td("bar"), td("")], vec![td("bar"), td("baz")]]),
        ),
        "7. padded and truncated rows",
    );

    // 8. no body rows means no <tbody> at all.
    let out = to_html("| abc | def |\n| --- | --- |\n", &GFM_OPTS);
    assert_eq!(
        out,
        table_html(&thead(&[th("abc"), th("def")]), ""),
        "8. no body rows"
    );
    assert!(
        !out.contains("tbody"),
        "8. no body rows must emit no <tbody>: {out:?}"
    );
}

// --- the rules those examples rest on ---

#[test]
fn gfm_tables_rules() {
    let html = |src: &str| to_html(src, &GFM_OPTS);

    // A delimiter cell is hyphens and at most two colons. Leading and
    // trailing pipes are optional here as everywhere. A row of nothing
    // but hyphens (`-`, `---`) is left out: it is a setext underline
    // first, which the test below covers.
    for delim in [
        "| --- |", "| :-- |", "| --: |", "| :-: |", "| - |", "|-|", "|--", "--|", ":-", "-:",
        ":-:", ":---:",
    ] {
        let got = html(&format!("| a |\n{delim}\n"));
        assert!(
            got.starts_with("<table>"),
            "{delim:?} should be a delimiter row, got {got:?}"
        );
    }
    for delim in [
        "| === |", "| -+- |", "| - - |", "| :: |", "| :  |", "| a |", "|  |", "| ::- |", "| -:- |",
        "| *** |", "| — |",
    ] {
        let got = html(&format!("| a |\n{delim}\n"));
        assert!(
            got.starts_with("<p>"),
            "{delim:?} should not be a delimiter row, got {got:?}"
        );
    }

    // Spaces and tabs between pipes and content are trimmed.
    check(
        "|   a   |\t b\t|\n| - | - |\n|\tx\t|   y   |\n",
        &table_html(
            &thead(&[th("a"), th("b")]),
            &tbody(&[vec![td("x"), td("y")]]),
        ),
        "spaces and tabs are trimmed",
    );

    // The header row is the paragraph's last line, so the paragraph
    // splits.
    check(
        "aaa\nbbb\n| a | b |\n| - | - |\n| c | d |\n",
        &format!(
            "<p>aaa\nbbb</p>\n{}",
            table_html(
                &thead(&[th("a"), th("b")]),
                &tbody(&[vec![td("c"), td("d")]]),
            )
        ),
        "the paragraph above the header row is kept",
    );
    // The lines left behind are a real paragraph: their reference
    // definitions still register, because splitting finalizes them.
    check(
        "[r]: /url\n| a |\n| - |\n\n[r]\n",
        &format!(
            "{}<p><a href=\"/url\">r</a></p>\n",
            table_html(&thead(&[th("a")]), "")
        ),
        "a split paragraph still contributes reference definitions",
    );

    // A setext underline still wins over a one-column delimiter row:
    // `---` is both, and the block starts are ordered so the heading
    // wins, exactly as cmark-gfm orders them. `| foo |` as heading text
    // is the giveaway.
    check(
        "foo\n---\n",
        "<h2>foo</h2>\n",
        "setext beats a delimiter row",
    );
    check(
        "| foo |\n---\n",
        "<h2>| foo |</h2>\n",
        "setext beats a delimiter row",
    );
    // A list marker wins too: `- | -` is a bullet, not a two-column
    // delimiter.
    check(
        "| a | b |\n- | -\n",
        "<p>| a | b |</p>\n<ul>\n<li>| -</li>\n</ul>\n",
        "a list marker beats a delimiter row",
    );

    // A delimiter row with no paragraph above it is just a paragraph.
    check("| - |\n", "<p>| - |</p>\n", "no paragraph above");
    check("\n| - |\n", "<p>| - |</p>\n", "no paragraph above");
    // ...and it must be the line *directly* above.
    check(
        "| a |\n\n| - |\n",
        "<p>| a |</p>\n<p>| - |</p>\n",
        "not the line directly above",
    );

    // A table nests in a block quote and in a list item.
    check(
        "> | a | b |\n> | - | - |\n> | c | d |\n",
        &format!(
            "<blockquote>\n{}</blockquote>\n",
            table_html(
                &thead(&[th("a"), th("b")]),
                &tbody(&[vec![td("c"), td("d")]]),
            )
        ),
        "inside a block quote",
    );
    check(
        "- | a | b |\n  | - | - |\n  | c | d |\n",
        &format!(
            "<ul>\n<li>\n{}</li>\n</ul>\n",
            table_html(
                &thead(&[th("a"), th("b")]),
                &tbody(&[vec![td("c"), td("d")]]),
            )
        ),
        "inside a list item",
    );
    // A lazy continuation is not a delimiter row: the quote's marker is
    // missing, so the paragraph simply carries on.
    check(
        "> | a | b |\n| - | - |\n",
        "<blockquote>\n<p>| a | b |\n| - | - |</p>\n</blockquote>\n",
        "a lazy continuation is not a delimiter row",
    );

    // A table immediately followed by another table. Separated by a
    // blank line: two tables.
    check(
        "| a |\n| - |\n| 1 |\n\n| b |\n| - |\n| 2 |\n",
        &format!(
            "{}{}",
            table_html(&thead(&[th("a")]), &tbody(&[vec![td("1")]])),
            table_html(&thead(&[th("b")]), &tbody(&[vec![td("2")]]))
        ),
        "two tables separated by a blank line",
    );
    // Without one, the second "table" is body rows of the first: a
    // delimiter row is only special directly under a *paragraph*.
    check(
        "| a |\n| - |\n| b |\n| - |\n",
        &table_html(&thead(&[th("a")]), &tbody(&[vec![td("b")], vec![td("-")]])),
        "no blank line means body rows",
    );

    // Inlines inside cells are parsed, and only inlines.
    check(
        "| a |\n| - |\n| *e* ~~s~~ `c` [l](/u) www.x.com |\n",
        &table_html(
            &thead(&[th("a")]),
            &tbody(&[vec![td(
                "<em>e</em> <del>s</del> <code>c</code> <a href=\"/u\">l</a> <a href=\"http://www.x.com\">www.x.com</a>",
            )]]),
        ),
        "inlines in cells, autolink literals included",
    );
    // The tagfilter is a render-time step and still applies inside a
    // cell.
    check(
        "| a |\n| - |\n| <title>x |\n",
        &table_html(&thead(&[th("a")]), &tbody(&[vec![td("&lt;title>x")]])),
        "the tagfilter applies inside a cell",
    );
}

// --- escaped pipes, in detail ---

#[test]
fn gfm_tables_escaped_pipes() {
    // Splitting is on unescaped pipes only: one cell, not two.
    check(
        "| a\\|b |\n| - |\n",
        &table_html(&thead(&[th("a|b")]), ""),
        "an escaped pipe never separates",
    );
    // `\\` is an escaped backslash, so the pipe after it *does* separate.
    check(
        "| x | y |\n| - | - |\n| a\\\\ | b |\n",
        &table_html(
            &thead(&[th("x"), th("y")]),
            &tbody(&[vec![td("a\\"), td("b")]]),
        ),
        "a backslash cannot shield the pipe after it",
    );
    // A trailing pipe that is itself escaped is content, not the optional
    // trailing delimiter.
    check(
        "| a\\| |\n| - |\n",
        &table_html(&thead(&[th("a|")]), ""),
        "an escaped trailing pipe is content",
    );

    // An escaped pipe becomes a literal pipe BEFORE inline parsing. This
    // is the whole point of resolving `\|` at split time: a code span is
    // a literal, so nothing downstream could turn `\|` into `|` inside
    // one.
    check(
        "| a |\n| - |\n| `\\|` |\n",
        &table_html(&thead(&[th("a")]), &tbody(&[vec![td("<code>|</code>")]])),
        "a code span sees a raw pipe",
    );
    check(
        "| a |\n| - |\n| **\\|** |\n",
        &table_html(
            &thead(&[th("a")]),
            &tbody(&[vec![td("<strong>|</strong>")]]),
        ),
        "strong emphasis around a raw pipe",
    );
    // ...and the cell text is not unescaped twice: every other escape is
    // left for the inline phase, which handles it exactly once.
    check(
        "| a |\n| - |\n| \\*not em\\* |\n",
        &table_html(&thead(&[th("a")]), &tbody(&[vec![td("*not em*")]])),
        "other escapes are left to the inline phase",
    );
    check(
        "| a |\n| - |\n| \\\\ |\n",
        &table_html(&thead(&[th("a")]), &tbody(&[vec![td("\\")]])),
        "an escaped backslash is unescaped exactly once",
    );

    // Byte-vs-character: a multi-byte character on either side of a split
    // or a trim must survive whole.
    check(
        "| é | 日本語 |\n| - | - |\n|  🎉  | a\\|é |\n",
        &table_html(
            &thead(&[th("é"), th("日本語")]),
            &tbody(&[vec![td("🎉"), td("a|é")]]),
        ),
        "non-ASCII cell content survives splitting and trimming",
    );
}

// --- the AST projection ---

fn cell(v: &str) -> serde_json::Value {
    json!({"type": "tableCell", "children": [{"type": "text", "value": v}]})
}

#[test]
fn gfm_tables_ast() {
    let doc = to_json(&parse_document(
        "| h1 | h2 |\n| :- | -: |\n| b1 | b2 |\n",
        &GFM_OPTS,
    ));
    let children = doc["children"].as_array().expect("children");
    assert_eq!(children.len(), 1, "expected 1 block");

    let table = &children[0];
    assert_eq!(table["type"], "table");
    assert_eq!(table["align"], json!(["left", "right"]));
    let rows = table["children"].as_array().expect("rows");
    assert_eq!(rows.len(), 2, "expected 2 rows");

    // mdast has no header flag: the FIRST row is the header, by
    // convention.
    assert_eq!(
        rows[0],
        json!({"type": "tableRow", "children": [cell("h1"), cell("h2")]}),
        "header row"
    );
    assert_eq!(
        rows[1],
        json!({"type": "tableRow", "children": [cell("b1"), cell("b2")]}),
        "body row"
    );

    // align carries null for a column with no colon: one entry per
    // column, always, and `[null, ...]` rather than `[""]` or `null`.
    let doc = to_json(&parse_document(
        "| a | b | c |\n| --- | :-: | --: |\n",
        &GFM_OPTS,
    ));
    let table = &doc["children"][0];
    assert_eq!(table["align"], json!([null, "center", "right"]));
    assert_eq!(
        table["children"][0]["children"]
            .as_array()
            .expect("cells")
            .len(),
        3,
        "header row has 3 cells"
    );

    // Padded cells are real, empty cells: `children: []`, not null.
    let doc = to_json(&parse_document("| a | b |\n| - | - |\n| x |\n", &GFM_OPTS));
    let body = &doc["children"][0]["children"][1];
    let cells = body["children"].as_array().expect("cells");
    assert_eq!(cells.len(), 2, "padded row has 2 cells");
    assert_eq!(cells[1], json!({"type": "tableCell", "children": []}));

    // Cells hold inline nodes.
    let doc = to_json(&parse_document("| a |\n| - |\n| *x* |\n", &GFM_OPTS));
    let got = &doc["children"][0]["children"][1]["children"][0];
    assert_eq!(
        *got,
        json!({
            "type": "tableCell",
            "children": [{"type": "emphasis", "children": [{"type": "text", "value": "x"}]}]
        })
    );

    // A table nested in a list item projects in place.
    let doc = to_json(&parse_document("- | a |\n  | - |\n", &GFM_OPTS));
    let item = &doc["children"][0]["children"][0];
    let blocks = item["children"].as_array().expect("blocks");
    assert_eq!(blocks.len(), 1, "item has 1 block");
    assert_eq!(blocks[0]["type"], "table");

    // An empty align array serializes as [], never null. Unreachable from
    // a real parse (parse_delimiter_row rejects a zero-cell row), so it
    // is asserted on a hand-built node, which is where a missing array
    // would leak out.
    let mut tree = parse_tree("| a |\n| - |\n", &GFM_OPTS);
    let root = tree.root();
    let table = tree.get(root).first_child.expect("a table");
    tree.get_mut(table).table_align = None;
    let raw = tabnas_markdown::to_ast(&tree, &GFM_OPTS)
        .to_json()
        .to_string();
    assert!(
        raw.contains("\"align\":[]"),
        "empty align must serialize as [], got {raw}"
    );
}

// --- sourcepos ---

/// The sourcepos start of the first table in src.
fn table_start(src: &str) -> [usize; 2] {
    let tree = parse_tree(src, &GFM_OPTS);
    let mut walker = tree.walker(tree.root());
    while let Some(e) = walker.next(&tree) {
        if e.entering && tree.get(e.node).node_type == NodeType::Table {
            return tree.get(e.node).sourcepos[0];
        }
    }
    [usize::MAX, usize::MAX]
}

/// A table opened by its delimiter row but which begins on the header
/// row above it, the two rows being indented independently. Taking the
/// start column from the delimiter row would report a column belonging
/// to a different line, visible through `parse_tree`, which is public.
#[test]
fn gfm_table_sourcepos() {
    for (src, want, why) in [
        ("a | b\n| - | -\n", [1, 1], "control: neither row indented"),
        ("  a | b\n| - | -\n", [1, 3], "the header row is indented"),
        ("a | b\n  | - | -\n", [1, 1], "only the delimiter row is"),
        ("  a | b\n  | - | -\n", [1, 3], "both rows are"),
    ] {
        assert_eq!(table_start(src), want, "{why}: {src:?} start");
    }
}

/// The header row at the *end* of a longer paragraph, so the column
/// cannot come from the paragraph node either: that one belongs to the
/// first line, which stays a paragraph.
#[test]
fn gfm_table_sourcepos_split_paragraph() {
    for (src, want, why) in [
        (
            "x\n  a | b\n| - | -\n",
            [2, 3],
            "header row indented, paragraph's is not",
        ),
        (
            "  x\na | b\n| - | -\n",
            [2, 1],
            "paragraph indented, header row is not",
        ),
        // A table inside a block quote counts columns from the start of
        // the line, marker included, exactly as every other block does.
        ("> a | b\n> | - | -\n", [1, 3], "block quote"),
        (
            ">   a | b\n> | - | -\n",
            [1, 5],
            "block quote, indented header row",
        ),
    ] {
        assert_eq!(table_start(src), want, "{why}: {src:?} start");
    }
}

/// The sourcepos end of the first paragraph in src.
fn first_paragraph_end(src: &str) -> [usize; 2] {
    let tree = parse_tree(src, &GFM_OPTS);
    let mut walker = tree.walker(tree.root());
    while let Some(e) = walker.next(&tree) {
        if e.entering && tree.get(e.node).node_type == NodeType::Paragraph {
            return tree.get(e.node).sourcepos[1];
        }
    }
    [usize::MAX, usize::MAX]
}

/// The end of the paragraph a table splits. The split closes a block
/// *two* lines back (the header row in between belongs to the table), so
/// the end column cannot come from the previous line the way every other
/// finalize takes it.
#[test]
fn gfm_table_split_paragraph_ends_on_its_own_line() {
    // `aaaaaaaaaa` is 10 characters and the header row below it is 9, so
    // the wrong column there is a *shorter* one, which no clamp could
    // explain; `aa` under a 20-character header row is the other
    // direction.
    for (src, want) in [
        ("aaaaaaaaaa\n| a | b |\n| - | - |\n", [1, 10]),
        ("aa\n| aaaaaaaaaaaa | b |\n| - | - |\n", [1, 2]),
    ] {
        assert_eq!(first_paragraph_end(src), want, "{src:?} paragraph end");
    }

    // Every other interrupter already ends this paragraph at column 10;
    // the table must not be the odd one out.
    for after in [
        "# h\n",
        "```\nx\n```\n",
        "> q\n",
        "- i\n",
        "***\n",
        "<div>\n",
        "\nx\n",
    ] {
        let src = format!("aaaaaaaaaa\n{after}");
        assert_eq!(first_paragraph_end(&src), [1, 10], "{src:?} paragraph end");
    }
}

/// Rows span their own text, so two rows of one table on equally
/// indented lines report the same end column. The header row is built in
/// `try_open_table` and the body rows in `finalize_table`; measuring the
/// header against the whole source line would put it two columns past the
/// body rows inside a block quote.
#[test]
fn gfm_table_rows_share_one_basis() {
    let row_spans = |src: &str| -> Vec<SourcePos> {
        let tree = parse_tree(src, &GFM_OPTS);
        let mut out = Vec::new();
        let mut walker = tree.walker(tree.root());
        while let Some(e) = walker.next(&tree) {
            if e.entering && tree.get(e.node).node_type == NodeType::TableRow {
                out.push(tree.get(e.node).sourcepos);
            }
        }
        out
    };

    let want: Vec<SourcePos> = vec![[[1, 1], [1, 9]], [[3, 1], [3, 9]]];
    for src in [
        "| a | b |\n| - | - |\n| c | d |\n",
        "> | a | b |\n> | - | - |\n> | c | d |\n",
        "  | a | b |\n  | - | - |\n  | c | d |\n",
    ] {
        assert_eq!(row_spans(src), want, "{src:?} rows");
    }
}

// --- gfm:false ---

#[test]
fn gfm_tables_disabled() {
    let sources = [
        "| foo | bar |\n| --- | --- |\n| baz | bim |\n",
        "| abc | defghi |\n:-: | -----------:\nbar | baz\n",
        "| f\\|oo  |\n| ------ |\n| b `\\|` az |\n",
        "| abc | def |\n| --- | --- |\n",
        "aaa\n| a | b |\n| - | - |\n",
    ];
    for src in sources {
        let out = to_html(src, &CM_OPTS);
        assert!(
            !out.contains("<table"),
            "{src:?}: gfm:false produced a table: {out:?}"
        );
        // Exactly what a paragraph of these lines renders as, the
        // escaping and the soft breaks included.
        let want = render_html_with(&parse_tree(src, &CM_OPTS), CM_OPTS);
        assert_eq!(out, want, "{src:?}");
        assert!(
            out.starts_with("<p>"),
            "{src:?}: expected a paragraph, got {out:?}"
        );
        let doc = to_json(&parse_document(src, &CM_OPTS));
        for block in doc["children"].as_array().expect("children") {
            assert_ne!(
                block["type"], "table",
                "{src:?}: gfm:false produced a table node"
            );
        }
    }
    // The delimiter row is not even a paragraph break: it all stays one.
    assert_eq!(
        to_html("| a | b |\n| - | - |\n| c | d |\n", &CM_OPTS),
        "<p>| a | b |\n| - | - |\n| c | d |</p>\n"
    );

    // render_html on a tree parsed with GFM off, with no options: the
    // flavour follows the parse.
    let off = parse_tree("| a |\n| - |\n", &CM_OPTS);
    assert_eq!(render_html(&off, None), "<p>| a |\n| - |</p>\n");
    let on = parse_tree("| a |\n| - |\n", &GFM_OPTS);
    assert!(
        render_html(&on, None).starts_with("<table>"),
        "want a table"
    );
}

// --- robustness -------------------------------------------------------------
//
// Nothing here may panic, and nothing may become super-linear: the
// inputs are adversarial, and untrusted input picks them.

#[test]
fn gfm_tables_robustness() {
    // A 10000-column delimiter row.
    const COLS: usize = 10000;
    let src = format!(
        "|{}\n|{}\n|{}\n",
        " h |".repeat(COLS),
        " --- |".repeat(COLS),
        " b |".repeat(COLS)
    );
    let out = to_html(&src, &GFM_OPTS);
    assert_eq!(out.matches("<th>").count(), COLS, "wide table: <th>");
    assert_eq!(out.matches("<td>").count(), COLS, "wide table: <td>");
    let doc = to_json(&parse_document(&src, &GFM_OPTS));
    let table = &doc["children"][0];
    assert_eq!(
        table["align"].as_array().expect("align").len(),
        COLS,
        "wide table: align entries"
    );
    assert_eq!(
        table["children"][0]["children"]
            .as_array()
            .expect("cells")
            .len(),
        COLS,
        "wide table: header row cells"
    );

    // 10000 body rows.
    const ROWS: usize = 10000;
    let src = format!("| a | b |\n| - | - |\n{}", "| x | y |\n".repeat(ROWS));
    let out = to_html(&src, &GFM_OPTS);
    assert_eq!(out.matches("<tr>").count(), ROWS + 1, "tall table: <tr>");
    let doc = to_json(&parse_document(&src, &GFM_OPTS));
    assert_eq!(
        doc["children"][0]["children"]
            .as_array()
            .expect("rows")
            .len(),
        ROWS + 1,
        "tall table: rows"
    );

    // A pipe repeated 50000 times. On its own it is one paragraph and
    // nothing more; as a header row over a matching delimiter row it is a
    // very wide table: n pipes with the leading and trailing one stripped
    // is n-1 empty cells.
    const N: usize = 50000;
    let _ = to_html(&format!("{}\n", "|".repeat(N)), &GFM_OPTS);
    let _ = to_html(&format!("{}\n", "|".repeat(N)), &CM_OPTS);
    let out = to_html(
        &format!("{}\n|{}\n", "|".repeat(N), " - |".repeat(N - 1)),
        &GFM_OPTS,
    );
    assert_eq!(
        out.matches("<th></th>").count(),
        N - 1,
        "pipe storm: empty cells"
    );
}

/// Pins the autocomplete budget.
///
/// Padding short rows is the one shape whose node count is not bounded by
/// the input: 10000 columns over 10000 one-cell rows asks for 10^8 cells.
/// The budget caps it, so quadrupling the rows must not grow the work.
#[test]
fn gfm_table_padding_is_bounded() {
    let header = format!("|{}\n|{}\n", " h |".repeat(10000), " --- |".repeat(10000));
    let measure = |rows: usize| -> (Duration, usize) {
        let src = format!("{header}{}", "| x |\n".repeat(rows));
        let start = Instant::now();
        let out = to_html(&src, &GFM_OPTS);
        (start.elapsed(), out.len())
    };

    let (base_time, base_len) = measure(5000);
    let (large_time, large_len) = measure(20000);

    // The output is capped, not proportional to the row count.
    let ratio = large_len as f64 / base_len as f64;
    assert!(ratio < 3.0, "output grew {ratio:.1}x for 4x the rows");
    if base_time < Duration::from_millis(5) {
        return; // too fast to measure reliably
    }
    let ratio = large_time.as_secs_f64() / base_time.as_secs_f64();
    assert!(
        ratio < 6.0,
        "4x rows took {ratio:.1}x time ({base_time:?} -> {large_time:?})"
    );
}

/// Pins the block parser's `last_added_line`.
///
/// Every second line is a syntactically valid delimiter row that fails
/// the cell-count test, so the paragraph grows without bound and each
/// line asks for its last line again. Reading that back out of the
/// accumulated content is O(n) per line, and this is the input that shows
/// it.
#[test]
fn gfm_table_delimiter_row_is_linear() {
    let measure = |n: usize| -> Duration {
        let src = "| a | b |\n| --- |\n".repeat(n);
        let start = Instant::now();
        let _ = to_html(&src, &GFM_OPTS);
        start.elapsed()
    };

    measure(2000);
    let small = best_of(5, || measure(20000));
    let large = best_of(5, || measure(80000));

    if small < Duration::from_millis(2) {
        return; // too fast to measure reliably; assert nothing rather than flake
    }
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= 12.0,
        "4x input took {ratio:.1}x time ({small:?} -> {large:?}); linear is ~4x, quadratic ~16x"
    );
}

#[test]
fn gfm_table_degenerate_fragments() {
    let long_dashes = format!("a\n{}", "-".repeat(10000));
    let pipe_storm = format!("{}\n{}", "|".repeat(200), "-|".repeat(200));
    let fragments = [
        "|",
        "||",
        "|||",
        "\\|",
        "|\\",
        "| - ",
        " - |",
        "|-|",
        ":",
        "::",
        ":-:",
        "-:",
        "a\n|",
        "a\n:",
        "a\n-:",
        "a\n|-",
        "a\n||",
        "a\n:-:|",
        "a|b\n-|-|-",
        "a\n\\|",
        "|a|\n|\\|",
        "a\n|\t-\t|",
        "|\n|",
        "| |\n| |",
        long_dashes.as_str(),
        pipe_storm.as_str(),
        "> a\n> |-|",
        "- a\n  |-|",
        "a\n---\n",
        "a\n- | -",
        "|a\n|-\n    x",
        "\\\\|\n-",
        // Byte-vs-character: a delimiter row whose cells hold multi-byte
        // characters, and a header row that ends mid-character's worth
        // of pipes.
        "é|é\n-|-",
        "| 😀 |\n| - |\n| 😀 |",
        "—|—\n-|-",
    ];
    for src in fragments {
        for opts in [GFM_OPTS, CM_OPTS] {
            let src = format!("{src}\n");
            let _ = to_html(&src, &opts);
            let _ = parse_document(&src, &opts);
        }
    }
}

// --- task list items --------------------------------------------------------

#[test]
fn gfm_task_list_items() {
    // A marker becomes a checkbox and is consumed. Note the exact
    // attribute order and the space after the tag.
    check(
        "- [ ] foo\n",
        "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> foo</li>\n</ul>\n",
        "unchecked",
    );
    check(
        "- [x] bar\n",
        "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> bar</li>\n</ul>\n",
        "checked",
    );
    // "either a whitespace character or the letter x in either lowercase
    // or uppercase", and a tab between the brackets is whitespace, so
    // unchecked.
    check(
        "- [X] up\n",
        "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> up</li>\n</ul>\n",
        "uppercase X",
    );
    check(
        "- [\t] tab\n",
        "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> tab</li>\n</ul>\n",
        "tab is whitespace",
    );

    // Every list flavour, and arbitrarily nestable.
    check(
        "* [ ] a\n",
        "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> a</li>\n</ul>\n",
        "star bullet",
    );
    check(
        "1. [x] a\n",
        "<ol>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> a</li>\n</ol>\n",
        "ordered",
    );
    check(
        "> - [x] quoted\n",
        "<blockquote>\n<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> quoted</li>\n</ul>\n</blockquote>\n",
        "inside a block quote",
    );
    check(
        "- - [x] deep\n",
        "<ul>\n<li>\n<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> deep</li>\n</ul>\n</li>\n</ul>\n",
        "nested list",
    );

    // A loose item puts the checkbox inside the paragraph.
    check(
        "- [x] a\n\n- [ ] b\n",
        "<ul>\n<li>\n<p><input checked=\"\" disabled=\"\" type=\"checkbox\"> a</p>\n</li>\n<li>\n<p><input disabled=\"\" type=\"checkbox\"> b</p>\n</li>\n</ul>\n",
        "loose list",
    );

    // What is not a marker. The extension requires "at least one
    // whitespace character before any other content", and only a space,
    // an `x` or an `X` between the brackets.
    check(
        "- [x]no space\n",
        "<ul>\n<li>[x]no space</li>\n</ul>\n",
        "no trailing whitespace",
    );
    check(
        "- [x]\n",
        "<ul>\n<li>[x]</li>\n</ul>\n",
        "nothing after the marker",
    );
    check(
        "- [y] no\n",
        "<ul>\n<li>[y] no</li>\n</ul>\n",
        "not x or whitespace",
    );
    check(
        "- [xx] no\n",
        "<ul>\n<li>[xx] no</li>\n</ul>\n",
        "two characters",
    );
    // Only the *first* block of the item, and only at its start.
    check(
        "- a\n\n  [x] b\n",
        "<ul>\n<li>\n<p>a</p>\n<p>[x] b</p>\n</li>\n</ul>\n",
        "second block",
    );
    check(
        "[x] loose text\n",
        "<p>[x] loose text</p>\n",
        "not a list item at all",
    );

    // The marker beats a link reference definition of the same label: it
    // is decided over the paragraph's raw text, before the inline phase
    // runs.
    check(
        "[x]: /url\n\n- [x] still a task\n",
        "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> still a task</li>\n</ul>\n",
        "beats a reference definition",
    );
}

#[test]
fn gfm_task_list_items_ast() {
    let doc = to_json(&parse_document("- [x] a\n- [ ] b\n- c\n", &GFM_OPTS));
    let items = doc["children"][0]["children"]
        .as_array()
        .expect("items")
        .clone();
    assert_eq!(items.len(), 3, "expected 3 items");
    for (i, want) in [json!(true), json!(false), json!(null)].iter().enumerate() {
        let item = items[i].as_object().expect("an item");
        // mdast keeps the field on every item, null included.
        assert!(
            item.contains_key("checked"),
            "item {i} has no `checked` field"
        );
        assert_eq!(&item["checked"], want, "item {i} checked");
    }

    // The marker is gone from the text.
    assert_eq!(
        items[0]["children"],
        json!([{"type": "paragraph", "children": [{"type": "text", "value": "a"}]}])
    );

    // Nested items carry their own state.
    let nested = to_json(&parse_document("- [x] a\n  - [ ] b\n", &GFM_OPTS));
    let outer = &nested["children"][0]["children"][0];
    assert_eq!(outer["checked"], true, "outer item checked");
    let inner = &outer["children"][1]["children"][0];
    assert_eq!(inner["checked"], false, "inner item checked");
}

// --- disallowed raw HTML ----------------------------------------------------

#[test]
fn gfm_disallowed_raw_html() {
    // The nine tags lose their leading angle bracket, opening and
    // closing.
    for tag in DISALLOWED_TAGS {
        check(
            &format!("a <{tag}> b\n"),
            &format!("<p>a &lt;{tag}> b</p>\n"),
            tag,
        );
        check(
            &format!("a </{tag}> b\n"),
            &format!("<p>a &lt;/{tag}> b</p>\n"),
            &format!("/{tag}"),
        );
    }

    // Case-insensitive, block and inline.
    check("a <XMP> b\n", "<p>a &lt;XMP> b</p>\n", "uppercase");
    check(
        "a </ScRiPt> b\n",
        "<p>a &lt;/ScRiPt> b</p>\n",
        "mixed case, closing",
    );
    check(
        "<script>\nx\n</script>\n",
        "&lt;script>\nx\n&lt;/script>\n",
        "html block",
    );
    check(
        "<blockquote>\n  <xmp> no\n</blockquote>\n",
        "<blockquote>\n  &lt;xmp> no\n</blockquote>\n",
        "inside an html block",
    );

    // Everything else passes through verbatim, as CommonMark requires.
    check("a <em> b\n", "<p>a <em> b</p>\n", "not in the set");
    check(
        "a <scriptlet> b\n",
        "<p>a <scriptlet> b</p>\n",
        "longer tag name",
    );
    check("a <titles> b\n", "<p>a <titles> b</p>\n", "longer tag name");
    check(
        "a <script-ish> b\n",
        "<p>a <script-ish> b</p>\n",
        "hyphenated tag name",
    );
    // ASCII case folding only: a Unicode-aware fold would map U+017F onto
    // `s`, and the canonical runtime's `i` flag does not.
    check(
        "a <\u{17f}cript> b\n",
        "<p>a &lt;\u{17f}cript&gt; b</p>\n",
        "long s is not a tag at all",
    );
    // The filter is a rendering step; a code span is text, not raw HTML.
    check(
        "`<script>`\n",
        "<p><code>&lt;script&gt;</code></p>\n",
        "code span",
    );

    // The tree and the AST keep the original text.
    let doc = to_json(&parse_document("<script>x</script>\n", &GFM_OPTS));
    assert_eq!(
        doc["children"][0],
        json!({"type": "html", "value": "<script>x</script>"})
    );
}

// --- autolink literals ------------------------------------------------------

fn anchor(href: &str, text: &str) -> String {
    format!("<a href=\"{href}\">{text}</a>")
}

#[test]
fn gfm_autolink_literals() {
    let links = |src: &str| to_html(src, &GFM_OPTS).contains("<a href");

    // www., the three schemes, and email.
    check(
        "www.commonmark.org\n",
        &format!(
            "<p>{}</p>\n",
            anchor("http://www.commonmark.org", "www.commonmark.org")
        ),
        "www.",
    );
    check(
        "http://commonmark.org\n",
        &format!(
            "<p>{}</p>\n",
            anchor("http://commonmark.org", "http://commonmark.org")
        ),
        "http",
    );
    check(
        "https://commonmark.org\n",
        &format!(
            "<p>{}</p>\n",
            anchor("https://commonmark.org", "https://commonmark.org")
        ),
        "https",
    );
    check(
        "ftp://foo.bar.baz\n",
        &format!(
            "<p>{}</p>\n",
            anchor("ftp://foo.bar.baz", "ftp://foo.bar.baz")
        ),
        "ftp",
    );
    check(
        "foo@bar.baz\n",
        &format!("<p>{}</p>\n", anchor("mailto:foo@bar.baz", "foo@bar.baz")),
        "email",
    );

    // Only at a line start, after whitespace, or after * _ ~ (.
    for before in ["", " ", "*", "_", "~", "("] {
        assert!(
            links(&format!("{before}www.a.com\n")),
            "{before:?} should open an autolink"
        );
    }
    for before in ["x", "-", "/", "=", "\"", "&", ":"] {
        assert!(
            !links(&format!("{before}www.a.com\n")),
            "{before:?} should not open an autolink"
        );
    }

    // A valid domain needs a period and no underscore in its last two
    // segments.
    for (src, want, why) in [
        ("x www.a.b.com y\n", true, "plain"),
        ("x www.foo_bar.com y\n", false, "second-to-last segment"),
        ("x www.a.foo_bar y\n", false, "last segment"),
        (
            "x www.foo_bar.a.com y\n",
            true,
            "earlier segments may hold _",
        ),
        ("x http://foo_bar.com y\n", false, "scheme form, same rule"),
        ("x http://localhost/y z\n", false, "no period, no domain"),
    ] {
        assert_eq!(links(src), want, "{why}: {src:?} linked");
    }

    // Extended autolink path validation: trailing punctuation is not part
    // of the link.
    check(
        "Visit www.commonmark.org.\n",
        &format!(
            "<p>Visit {}.</p>\n",
            anchor("http://www.commonmark.org", "www.commonmark.org")
        ),
        "trailing period",
    );
    check(
        "Visit www.commonmark.org/a.b.\n",
        &format!(
            "<p>Visit {}.</p>\n",
            anchor("http://www.commonmark.org/a.b", "www.commonmark.org/a.b")
        ),
        "interior periods survive",
    );
    for p in ["?", "!", ".", ",", ":", "*", "~"] {
        assert!(
            to_html(&format!("x www.a.com{p}\n"), &GFM_OPTS).contains(">www.a.com</a>"),
            "trailing {p:?} is not part of the link"
        );
    }
    // `_` is the one that is also a domain character, so it reaches the
    // domain check before the trim can drop it and invalidates the last
    // segment: there is no link here at all, which is cmark-gfm's
    // behaviour too.
    check(
        "x www.a.com_\n",
        "<p>x www.a.com_</p>\n",
        "trailing underscore, in the domain",
    );
    // Past the domain it is ordinary trailing punctuation again.
    check(
        "x www.a.com/y_\n",
        &format!(
            "<p>x {}_</p>\n",
            anchor("http://www.a.com/y", "www.a.com/y")
        ),
        "trailing underscore, in the path",
    );

    // Parentheses balance across the whole match, and only when it ends
    // in `)`.
    let link = anchor(
        "http://www.google.com/search?q=Markup+(business)",
        "www.google.com/search?q=Markup+(business)",
    );
    check(
        "www.google.com/search?q=Markup+(business)\n",
        &format!("<p>{link}</p>\n"),
        "balanced",
    );
    check(
        "www.google.com/search?q=Markup+(business)))\n",
        &format!("<p>{link}))</p>\n"),
        "two extra",
    );
    check(
        "(www.google.com/search?q=Markup+(business))\n",
        &format!("<p>({link})</p>\n"),
        "wrapped",
    );
    check(
        "(www.google.com/search?q=Markup+(business)\n",
        &format!("<p>({link}</p>\n"),
        "open wrapper",
    );
    // Interior parentheses alone trigger nothing.
    check(
        "www.google.com/search?q=(business))+ok\n",
        &format!(
            "<p>{}</p>\n",
            anchor(
                "http://www.google.com/search?q=(business))+ok",
                "www.google.com/search?q=(business))+ok"
            )
        ),
        "does not end in )",
    );

    // A trailing entity-like `;`, and `<` ends a link.
    check(
        "www.google.com/search?q=commonmark&hl=en\n",
        &format!(
            "<p>{}</p>\n",
            anchor(
                "http://www.google.com/search?q=commonmark&amp;hl=en",
                "www.google.com/search?q=commonmark&amp;hl=en"
            )
        ),
        "& that is not an entity",
    );
    check(
        "www.google.com/search?q=commonmark&hl;\n",
        &format!(
            "<p>{}&amp;hl;</p>\n",
            anchor(
                "http://www.google.com/search?q=commonmark",
                "www.google.com/search?q=commonmark"
            )
        ),
        "entity-like trailing ;",
    );
    check(
        "www.commonmark.org/he<lp\n",
        &format!(
            "<p>{}&lt;lp</p>\n",
            anchor("http://www.commonmark.org/he", "www.commonmark.org/he")
        ),
        "< ends the link",
    );

    // Email: `+` before the `@` only, and a trailing `-` or `_`
    // invalidates it.
    check(
        "hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.\n",
        &format!(
            "<p>hello@mail+xyz.example isn't valid, but {} is.</p>\n",
            anchor("mailto:hello+xyz@mail.example", "hello+xyz@mail.example")
        ),
        "+ before the @ only",
    );
    check(
        "a.b-c_d@a.b\n",
        &format!("<p>{}</p>\n", anchor("mailto:a.b-c_d@a.b", "a.b-c_d@a.b")),
        "local part",
    );
    check(
        "a.b-c_d@a.b.\n",
        &format!("<p>{}.</p>\n", anchor("mailto:a.b-c_d@a.b", "a.b-c_d@a.b")),
        "trailing period",
    );
    check(
        "a.b-c_d@a.b-\n",
        "<p>a.b-c_d@a.b-</p>\n",
        "trailing hyphen invalidates",
    );
    check(
        "a.b-c_d@a.b_\n",
        "<p>a.b-c_d@a.b_</p>\n",
        "trailing underscore invalidates",
    );
    check("foo@bar\n", "<p>foo@bar</p>\n", "the domain needs a period");

    // Never inside a link, a code span, or raw HTML.
    check(
        "[label www.a.com](/dest)\n",
        &format!("<p>{}</p>\n", anchor("/dest", "label www.a.com")),
        "inline link label",
    );
    check(
        "[label www.a.com][ref]\n\n[ref]: /dest\n",
        &format!("<p>{}</p>\n", anchor("/dest", "label www.a.com")),
        "reference link label",
    );
    check(
        "<https://a.com/x>\n",
        &format!("<p>{}</p>\n", anchor("https://a.com/x", "https://a.com/x")),
        "a CommonMark autolink is already a link",
    );
    check(
        "`www.a.com`\n",
        "<p><code>www.a.com</code></p>\n",
        "code span",
    );
    check(
        "<b title=\"www.a.com\">x</b>\n",
        "<p><b title=\"www.a.com\">x</b></p>\n",
        "raw html",
    );
    check(
        "![alt www.a.com](/i)\n",
        "<p><img src=\"/i\" alt=\"alt www.a.com\" /></p>\n",
        "image alt text",
    );

    // Inside emphasis, strikethrough, headings and list items.
    check(
        "*www.a.com*\n",
        &format!(
            "<p><em>{}</em></p>\n",
            anchor("http://www.a.com", "www.a.com")
        ),
        "emphasis",
    );
    check(
        "~~www.a.com~~\n",
        &format!(
            "<p><del>{}</del></p>\n",
            anchor("http://www.a.com", "www.a.com")
        ),
        "strikethrough",
    );
    check(
        "# www.a.com\n",
        &format!("<h1>{}</h1>\n", anchor("http://www.a.com", "www.a.com")),
        "heading",
    );
    check(
        "- www.a.com\n",
        &format!(
            "<ul>\n<li>{}</li>\n</ul>\n",
            anchor("http://www.a.com", "www.a.com")
        ),
        "list item",
    );
}

/// Offset 0 of a text node.
///
/// The rule is about the SOURCE character immediately before the match,
/// and the post-pass runs over a tree that no longer carries source
/// offsets. At offset 0 of a text node that character lives in the
/// previous sibling, and the sibling's *type* names it exactly, so each
/// row below states the type, the character it ends on, and whether an
/// autolink may start after it.
///
/// Rejecting offset 0 whenever there is any previous sibling is the
/// obvious wrong fix: it breaks the four `true` rows in the middle of
/// this table.
#[test]
fn gfm_autolink_boundary_at_inline_node_edges() {
    for (prev_type, prefix, last_char, links) in [
        ("(none)", "", "start of line", true),
        ("softbreak", "a\n", "start of line", true),
        ("linebreak", "a\\\n", "start of line", true),
        ("emph", "*a*", "*", true),
        ("strong", "**a**", "*", true),
        ("del", "~~a~~", "~", true),
        ("code", "`a`", "`", false),
        ("link", "[a](/u)", ")", false),
        ("image", "![a](/u)", ")", false),
        ("html_inline", "<b>", ">", false),
    ] {
        // The row's first column, asserted rather than assumed: with an
        // inert word in place of the URL, the paragraph ends in a text
        // node whose previous sibling is exactly the node this row names.
        let tree = parse_tree(&format!("{prefix}X\n"), &GFM_OPTS);
        let para = tree.get(tree.root()).first_child.expect("a paragraph");
        let tail = tree.get(para).last_child.expect("a tail");
        assert_eq!(tree.get(tail).node_type, NodeType::Text, "tail type");
        assert_eq!(tree.get(tail).literal, "X", "tail literal");
        let got_prev = tree
            .get(tail)
            .prev
            .map_or("(none)", |p| tree.get(p).node_type.as_str());
        assert_eq!(got_prev, prev_type, "previous sibling");

        let out = to_html(&format!("{prefix}www.a.com\n"), &GFM_OPTS);
        assert_eq!(
            out.contains("href=\"http://www.a.com\""),
            links,
            "after {prev_type} (source ends {last_char}): {out:?}"
        );
    }
}

/// The emphasis case on its own, because it is what the obvious wrong
/// fix breaks, with its counterpart beside it.
#[test]
fn gfm_autolink_after_emphasis() {
    check(
        "*a*www.b.com\n",
        &format!(
            "<p><em>a</em>{}</p>\n",
            anchor("http://www.b.com", "www.b.com")
        ),
        "emphasis is still a delimiter an autolink may follow",
    );
    check(
        "`x`www.a.com\n",
        "<p><code>x</code>www.a.com</p>\n",
        "a code span is not",
    );

    // The boundary applies to email addresses too.
    check(
        "*a*b@c.de\n",
        &format!("<p><em>a</em>{}</p>\n", anchor("mailto:b@c.de", "b@c.de")),
        "email after emphasis",
    );
    check(
        "`x`b@c.de\n",
        "<p><code>x</code>b@c.de</p>\n",
        "email after a code span",
    );
    check(
        "[l](/u)b@c.de\n",
        &format!("<p>{}b@c.de</p>\n", anchor("/u", "l")),
        "email after a link",
    );
}

/// A local part of 65 characters or more is not recognised at all.
///
/// The rewind that finds the start of a local part stops after 64
/// characters, which is what keeps this pass linear and is RFC 5321's
/// own limit. Stopping there is a cap, not a boundary: when the character
/// just outside it still belongs to the local part, the address is
/// over-long and there is no address; linking the 64-character tail
/// would invent one.
#[test]
fn gfm_autolink_email_local_part_cap() {
    for (local, linked) in [
        ("a".repeat(63), true),
        ("a".repeat(64), true),
        ("a".repeat(65), false),
        ("a".repeat(100), false),
        // A leading `_` is itself a local-part character, so these are
        // the same four lengths shifted by one: 64 links, 65 does not.
        (format!("_{}", "a".repeat(63)), true),
        (format!("_{}", "a".repeat(64)), false),
        (format!("_{}", "a".repeat(65)), false),
        (format!("_{}", "a".repeat(100)), false),
    ] {
        let addr = format!("{local}@b.co");
        let got = to_html(&format!("{addr}\n"), &GFM_OPTS);
        let want = if linked {
            format!("<p>{}</p>\n", anchor(&format!("mailto:{addr}"), &addr))
        } else {
            format!("<p>{addr}</p>\n")
        };
        assert_eq!(got, want, "{}-character local part", local.len());
    }
}

#[test]
fn gfm_autolink_ast() {
    // `_` ends a text run in the inline scanner, so `a.b-c_d@a.b` reaches
    // the post-pass as three text siblings. Consolidating them is what
    // makes the whole address one link rather than `d@a.b`.
    let doc = to_json(&parse_document("a.b-c_d@a.b\n", &GFM_OPTS));
    assert_eq!(
        doc["children"][0]["children"],
        json!([{
            "type": "link",
            "url": "mailto:a.b-c_d@a.b",
            "title": null,
            "children": [{"type": "text", "value": "a.b-c_d@a.b"}]
        }]),
        "spanning match"
    );

    // The AST carries the decoded destination; the renderer
    // percent-encodes.
    let doc = to_json(&parse_document("see www.a.com/ä now\n", &GFM_OPTS));
    assert_eq!(
        doc["children"][0]["children"][1],
        json!({
            "type": "link",
            "url": "http://www.a.com/ä",
            "title": null,
            "children": [{"type": "text", "value": "www.a.com/ä"}]
        }),
        "non-ASCII path"
    );
    assert!(
        to_html("see www.a.com/ä now\n", &GFM_OPTS).contains("href=\"http://www.a.com/%C3%A4\""),
        "non-ASCII path is percent-encoded in the HTML"
    );
}

// --- gfm:false --------------------------------------------------------------

/// The regression that matters most: the whole 652-example CommonMark
/// suite runs with GFM off, so every extension has to vanish.
#[test]
fn gfm_disabled() {
    for (src, want) in [
        ("- [ ] foo\n", "<ul>\n<li>[ ] foo</li>\n</ul>\n"),
        ("- [x] foo\n", "<ul>\n<li>[x] foo</li>\n</ul>\n"),
        ("<script>alert(1)</script>\n", "<script>alert(1)</script>\n"),
        ("a <title> b\n", "<p>a <title> b</p>\n"),
        ("www.commonmark.org\n", "<p>www.commonmark.org</p>\n"),
        ("http://commonmark.org\n", "<p>http://commonmark.org</p>\n"),
        ("foo@bar.baz\n", "<p>foo@bar.baz</p>\n"),
        ("~~x~~\n", "<p>~~x~~</p>\n"),
    ] {
        assert_eq!(to_html(src, &CM_OPTS), want, "to_html {src:?}");
        // The tree records the parse flavour, so a caller rendering a
        // tree can ask for exactly what it was parsed as.
        let tree = parse_tree(src, &CM_OPTS);
        assert!(!tree.get(tree.root()).gfm, "parse_tree {src:?}: gfm = true");
        assert_eq!(render_html(&tree, None), want, "render_html(tree) {src:?}");
    }

    // ...and an explicit option still wins over what the tree records.
    let tree = parse_tree("<script>x</script>\n", &CM_OPTS);
    assert_eq!(
        render_html(&tree, Some(&GFM_OPTS)),
        "&lt;script>x&lt;/script>\n",
        "explicit gfm over a CommonMark tree"
    );
    let tree = parse_tree("x\n", &GFM_OPTS);
    assert!(
        tree.get(tree.root()).gfm,
        "parse_tree with GFM on: gfm = false"
    );

    // The AST carries no GFM shapes either.
    let doc = to_json(&parse_document("- [x] www.a.com and a@b.co\n", &CM_OPTS));
    let item = &doc["children"][0]["children"][0];
    assert_eq!(item["checked"], json!(null));
    assert_eq!(
        item["children"],
        json!([{
            "type": "paragraph",
            "children": [{"type": "text", "value": "[x] www.a.com and a@b.co"}]
        }])
    );
}

// --- robustness -------------------------------------------------------------

/// Nothing here may panic, and nothing may become super-linear. The
/// inputs are adversarial, and untrusted input picks them.
#[test]
fn gfm_autolink_robustness_long_run_of_www_prefixes() {
    let out = to_html(&format!("{}\n", "www.".repeat(50000)), &GFM_OPTS);
    // One link over the whole run; the final period is trailing
    // punctuation.
    assert!(
        out.starts_with("<p><a href=\"http://www.www."),
        "prefix: {:?}",
        &out[..out.len().min(60)]
    );
    assert!(
        out.ends_with("www</a>.</p>\n"),
        "suffix: {:?}",
        &out[out.len().saturating_sub(60)..]
    );
    assert_eq!(out.matches("<a href").count(), 1, "expected one link");
}

#[test]
fn gfm_autolink_robustness_deeply_parenthesised_urls() {
    let (open, closed) = ("(".repeat(20000), ")".repeat(20000));

    let bal = to_html(&format!("www.a.com/{open}{closed}\n"), &GFM_OPTS);
    assert!(
        bal.contains(&format!("href=\"http://www.a.com/{open}{closed}\"")),
        "balanced parens are not all part of the link"
    );

    let unbal = to_html(&format!("www.a.com/x{closed}\n"), &GFM_OPTS);
    assert!(
        unbal.starts_with("<p><a href=\"http://www.a.com/x\">www.a.com/x</a>)"),
        "unbalanced prefix: {:?}",
        &unbal[..unbal.len().min(60)]
    );
    assert!(
        unbal.ends_with(&format!("{closed}</p>\n")),
        "unbalanced parens did not stay in the text"
    );
}

#[test]
fn gfm_autolink_robustness_candidates_that_fail_late_stay_linear() {
    // `_` both continues a domain and opens an autolink, so every
    // underscore here starts a candidate whose domain runs to the end of
    // the paragraph: the shape that goes quadratic without a bound on
    // the domain scan.
    let measure = |n: usize| -> Duration {
        let src = format!("{}\n", "_www.a_b.c".repeat(n));
        let start = Instant::now();
        let _ = to_html(&src, &GFM_OPTS);
        start.elapsed()
    };

    measure(2000);
    let small = best_of(5, || measure(20000));
    let large = best_of(5, || measure(80000));

    if small < Duration::from_millis(2) {
        return; // too fast to measure reliably; assert nothing rather than flake
    }
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    println!("4x input took {ratio:.1}x time ({small:?} -> {large:?})");
    assert!(
        ratio <= 12.0,
        "4x input took {ratio:.1}x time ({small:?} -> {large:?}); linear is ~4x, quadratic ~16x"
    );
}

#[test]
fn gfm_autolink_robustness_degenerate_fragments() {
    let dots = format!("a@{}", ".".repeat(500));
    let parens = format!("www.a.com/{}", ")".repeat(500));
    let amps = format!("{};", "&".repeat(500));
    let fragments = [
        "www.",
        "www..",
        "www...",
        "w",
        "@",
        "@@@",
        "www.@",
        "a@",
        "@b.c",
        "http://",
        "://",
        "www. x",
        dots.as_str(),
        parens.as_str(),
        amps.as_str(),
        "- [",
        "- []",
        "- [ ",
        "<script",
        "</",
        // Byte-vs-character: a multi-byte character next to every
        // boundary the pass computes.
        "www.é.com",
        "www.a.com/é.",
        "é@a.com",
        "a@é.com",
        "(www.a.com/é)",
        "www.a.com/\u{fffd}\u{fffd}",
        "\u{fffd}www.a.com",
    ];
    for src in fragments {
        for opts in [GFM_OPTS, CM_OPTS] {
            let src = format!("{src}\n");
            let _ = to_html(&src, &opts);
            let _ = parse_document(&src, &opts);
        }
    }
}

/// The five extensions and the one option: `Options::default()` is GFM
/// on, and `Options::COMMONMARK` is every extension off.
#[test]
fn option_defaults() {
    assert_eq!(
        Options::default(),
        Options {
            gfm: true,
            breaks: false
        }
    );
    assert_eq!(
        Options::COMMONMARK,
        Options {
            gfm: false,
            breaks: false
        }
    );
}
