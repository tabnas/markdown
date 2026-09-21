// Token COLUMNS after a non-ASCII character, in both engine phases.
//
// This plugin brings its own matchers, and a plugin that does owns the
// arithmetic the engine's matchers do for it: the byte offset advances in
// BYTES and the column in CHARACTERS. Both phases here hand that
// arithmetic to the lexer's own `advance_chars`, which counts characters
// and resets the column on a consumed line ending; this pins the
// positions that yields, on the same tables ts/test/engine-columns.test.ts
// and go/enginecol_test.go assert.
//
// The astral rows are the only ones where the answers differ, and that is
// the recorded engine divergence: TypeScript counts UTF-16 units (an
// astral character is 2), Go and Rust count characters (1). See
// parser/DIVERGENCE.md, "Column positions for astral characters".
//
// Neither phase puts these positions in the AST, so a pin that went
// through `parse` would assert nothing about them. The token stream is
// the observable surface: `subscribe_tokens` sees every token the parser
// consumes, `#ZZ` included.

use std::sync::{Arc, Mutex};

use tabnas::Tabnas;
use tabnas_markdown::block::parse_blocks;
use tabnas_markdown::engine_inline::{make_inline_tn, parse_inlines_engine};
use tabnas_markdown::{markdown, Options};

fn subscribe(tn: &mut Tabnas) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    tn.subscribe_tokens(move |token| {
        sink.lock().expect("the sink is not poisoned").push(format!(
            "{}@{}:{}",
            token.name.as_str(),
            token.site.ri,
            token.site.ci
        ));
    });
    seen
}

/// The stream up to and including the first `#ZZ`, which is where the
/// other runtimes' lexer-driving loops stop too: the rule loop may look
/// at the end token from more than one rule, and each look is reported.
fn drain(seen: &Arc<Mutex<Vec<String>>>) -> String {
    let mut seen = seen.lock().expect("the sink is not poisoned");
    let end = seen
        .iter()
        .position(|t| t.starts_with("#ZZ@"))
        .map_or(seen.len(), |i| i + 1);
    let out = seen[..end].join(" ");
    seen.clear();
    out
}

#[test]
fn block_columns_count_characters_not_bytes() {
    // Every case omits the trailing newline on purpose. A source that
    // ends in one takes the terminated branch, which resets the column to
    // 1 and hides the arithmetic.
    let cases: [(&str, &str, &str); 12] = [
        // Controls. Pure ASCII, and a source that DOES end in a newline:
        // without them, "columns count characters" is also satisfied by
        // never counting.
        ("ascii-nl", "# xx\n", "#LB@1:1 #ZZ@2:1"),
        ("ascii-nonl", "# xx", "#LB@1:1 #ZZ@1:5"),
        // 2 and 3 bytes, 1 character, 1 UTF-16 unit: every port agrees.
        ("latin1", "# é", "#LB@1:1 #ZZ@1:4"),
        ("bmp", "# €", "#LB@1:1 #ZZ@1:4"),
        ("para-latin1", "é text", "#LB@1:1 #ZZ@1:7"),
        ("para-bmp", "€€ text", "#LB@1:1 #ZZ@1:8"),
        ("emph-latin1", "*é* x", "#LB@1:1 #ZZ@1:6"),
        ("link-bmp", "[€](u) x", "#LB@1:1 #ZZ@1:9"),
        ("code-latin1", "`é` x", "#LB@1:1 #ZZ@1:6"),
        ("html-latin1", "<b>é</b> x", "#LB@1:1 #ZZ@1:11"),
        // A terminated line followed by an unterminated one: the row
        // advances on the first and the column counts on the second.
        ("multiline-latin1", "é a\nb c", "#LB@1:1 #LB@2:1 #ZZ@2:4"),
        // 4 bytes, 1 character, TWO UTF-16 units: the recorded
        // divergence (TypeScript asserts #ZZ@1:5), and the only block row
        // where the halves differ.
        ("astral", "# \u{1F600}", "#LB@1:1 #ZZ@1:4"),
    ];
    for (label, src, want) in cases {
        let mut tn = Tabnas::new();
        markdown(&mut tn, &Options::default()).expect("installs");
        let seen = subscribe(&mut tn);
        tn.parse(src).expect("parses");
        assert_eq!(drain(&seen), want, "{label}: {src:?}");
    }
}

#[test]
fn inline_columns_count_characters_not_bytes() {
    // The inline phase is a second engine instance over one block's
    // content, and its positions never reach the AST. Driving that
    // instance over a single paragraph is the level at which the
    // arithmetic is observable at all, so it is the level the pin asserts.
    let opts = Options::default();
    let cases: [(&str, &str, &str); 9] = [
        // Control: pure ASCII.
        ("ascii", "a b c", "#ITX@1:1 #ZZ@1:6"),
        // 2 and 3 bytes, 1 character, 1 UTF-16 unit: every port agrees.
        ("latin1", "é text", "#ITX@1:1 #ZZ@1:7"),
        ("bmp", "€€ text", "#ITX@1:1 #ZZ@1:8"),
        (
            "emph-latin1",
            "*é* x",
            "#IDL@1:1 #ITX@1:2 #IDL@1:3 #ITX@1:4 #ZZ@1:6",
        ),
        ("code-latin1", "`é` x", "#ICS@1:1 #ITX@1:4 #ZZ@1:6"),
        (
            "link-bmp",
            "[€](u) x",
            "#IOB@1:1 #ITX@1:2 #ICB@1:3 #ITX@1:7 #ZZ@1:9",
        ),
        // A consumed line ending resets the column against the last line
        // ending rather than accumulating. A multiline code span is the
        // shortest input that reaches it.
        (
            "codespan-multiline-latin1",
            "`é\né` tail é",
            "#ICS@1:1 #ITX@2:3 #ZZ@2:10",
        ),
        (
            "codespan-multiline-bmp",
            "`€€\nx` tail €",
            "#ICS@1:1 #ITX@2:3 #ZZ@2:10",
        ),
        // 4 bytes, 1 character, TWO UTF-16 units: the recorded divergence
        // (TypeScript asserts #ZZ@1:8).
        ("astral", "\u{1F600} text", "#ITX@1:1 #ZZ@1:7"),
    ];
    for (label, subject, want) in cases {
        let mut tn = make_inline_tn(&opts);
        let seen = subscribe(&mut tn);
        // One paragraph whose raw content is exactly the subject: the
        // block phase alone, so the content is still there to scan.
        let (tree, refmap) = parse_blocks(subject, opts);
        let _ = parse_inlines_engine(&tn, tree, refmap, &opts);
        assert_eq!(drain(&seen), want, "{label}: {subject:?}");
    }

    // The soft break row, on both branches: TypeScript asserts
    // "#ITX@1:1 #IBK@1:5 #ITX@2:1 #ZZ@2:5".
    let mut tn = make_inline_tn(&opts);
    let seen = subscribe(&mut tn);
    let (tree, refmap) = parse_blocks("\u{1F600} a\n\u{1F600} b", opts);
    let _ = parse_inlines_engine(&tn, tree, refmap, &opts);
    assert_eq!(drain(&seen), "#ITX@1:1 #IBK@1:4 #ITX@2:1 #ZZ@2:4");
}
