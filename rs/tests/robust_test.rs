// Adversarial input. Markdown is frequently parsed from untrusted
// sources, so nothing here may panic, hang, or take super-linear time.
// Mirrors go/robust_test.go.

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::{best_of, to_json};
use tabnas_markdown::{
    parse_document, parse_tree, render_html_with, to_html, NodeType, Options,
    MAX_CONTAINER_NESTING, MAX_INLINE_NESTING,
};

#[test]
fn no_panic_on_adversarial_input() {
    let cases: Vec<(&str, String)> = vec![
        ("long backtick fence", format!("{}\nx\n", "`".repeat(2000))),
        ("long tilde fence", format!("{}\nx\n", "~".repeat(2000))),
        (
            "fence at char boundary",
            format!("```{}\nx\n", "é".repeat(2000)),
        ),
        (
            "deep brackets",
            format!("{}a{}", "[".repeat(20000), "]".repeat(20000)),
        ),
        (
            "delimiter run",
            format!("{}x{}", "*".repeat(5000), "*".repeat(5000)),
        ),
        ("unclosed backticks", format!("{}x", "`".repeat(5000))),
        ("deep quotes", format!("{} x", ">".repeat(2000))),
        ("unclosed autolink", format!("<{}", "a".repeat(50000))),
        ("entity storm", "&amp;".repeat(20000)),
        ("setext storm", "a\n=\n".repeat(5000)),
        ("unbalanced link parens", "[a](b".repeat(10000)),
        ("nul bytes", "a\0b\0c".to_string()),
        // Rust strings are UTF-8 by construction, so the invalid-byte
        // rows of the other runtimes are spelled here as the replacement
        // characters a lossy decode of them yields.
        ("replacement characters", "a\u{fffd}\u{fffd}b".to_string()),
        ("lone replacement", "a\u{fffd}b".to_string()),
        ("crlf mix", "a\r\nb\rc\nd".to_string()),
        ("empty", String::new()),
        ("only newlines", "\n\n\n\n".to_string()),
        ("tabs everywhere", "\ta\tb\n".repeat(5000)),
    ];

    for (name, input) in cases {
        let (tx, rx) = mpsc::channel();
        let src = input.clone();
        let worker = std::thread::Builder::new()
            .name(name.to_string())
            .stack_size(64 << 20)
            .spawn(move || {
                for gfm in [true, false] {
                    let opts = Options { gfm, breaks: false };
                    let _ = to_html(&src, &opts);
                    let _ = parse_document(&src, &opts);
                }
                let _ = tx.send(());
            })
            .expect("spawns");
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("{name}: timed out, likely super-linear")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("{name}: panicked"),
        }
        worker.join().unwrap_or_else(|_| panic!("{name}: panicked"));
    }
}

fn measure(src: &str) -> Duration {
    let start = Instant::now();
    let _ = parse_tree(src, &Options::default());
    start.elapsed()
}

/// Pins the paren-nesting bound in the link destination scanner. Without
/// it, an unbalanced `[a](b` run makes every `]` scan to end of input:
/// 4x the input cost ~16x the time.
#[test]
fn link_destination_is_not_quadratic() {
    // Warm up, then compare a 4x size step against a generous linear
    // bound. Best-of-5 and a meaningful-baseline floor, because a single
    // sample on a shared CI runner is noise. Linear is ~4x here,
    // quadratic ~16x.
    measure(&"[a](b".repeat(1000));
    let small = best_of(5, || measure(&"[a](b".repeat(5000)));
    let large = best_of(5, || measure(&"[a](b".repeat(20000)));

    if small < Duration::from_millis(2) {
        return; // too fast to measure reliably; assert nothing rather than flake
    }
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= 12.0,
        "4x input took {ratio:.1}x time ({small:?} -> {large:?}); linear is ~4x, quadratic ~16x"
    );
}

/// Pins the sourcepos column anchor in block.rs. A sourcepos column
/// counts characters, but this port's offsets are byte indices, so
/// `add_child` has to convert; counting from the start of the line each
/// time made a line of n nested containers cost O(n²). `> - > - > - …` is
/// cheap to write and untrusted input decides n, so this is a
/// denial-of-service bound, not a micro-benchmark.
#[test]
fn nested_containers_are_not_quadratic() {
    let nested = |n: usize| format!("{}x", "> - ".repeat(n));
    measure(&nested(1000));
    let small = best_of(5, || measure(&nested(5000)));
    let large = best_of(5, || measure(&nested(20000)));

    if small < Duration::from_millis(2) {
        return;
    }
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= 12.0,
        "4x input took {ratio:.1}x time ({small:?} -> {large:?}); linear is ~4x, quadratic ~16x"
    );
}

/// Pins the one thing a Unicode-aware case-insensitive regex does that
/// JavaScript's `i` flag does not: fold case over all of Unicode.
///
/// `(?i)script` would also match `ſcript`, and `(?i)[A-Za-z]` would match
/// U+017F LATIN SMALL LETTER LONG S and U+212A KELVIN SIGN, where
/// JavaScript's non-unicode `i` deliberately never folds a non-ASCII
/// code point onto an ASCII one. Both spec suites are pure ASCII, so
/// neither catches it; a cross-runtime comparison over non-ASCII input
/// did.
#[test]
fn case_folding_is_ascii_only() {
    // Written as escapes, not as literals: U+017F and U+212A are
    // homoglyphs of `s` and `K` in most editors, and this test is exactly
    // about telling them apart.
    const LONG_S: &str = "\u{17f}";
    const KELVIN: &str = "\u{212a}";

    let cases = [
        // Type 1: <script|pre|textarea|style>.
        (
            format!("<{LONG_S}cript>\n"),
            format!("<p>&lt;{LONG_S}cript&gt;</p>\n"),
            "long s is not `s`",
        ),
        (
            format!("<{LONG_S}tyle>\n"),
            format!("<p>&lt;{LONG_S}tyle&gt;</p>\n"),
            "long s is not `s`",
        ),
        // Type 6: the §4.6 tag list.
        (
            format!("<lin{KELVIN}>\n"),
            format!("<p>&lt;lin{KELVIN}&gt;</p>\n"),
            "kelvin sign is not `k`",
        ),
        (
            format!("<bloc{KELVIN}quote>\n"),
            format!("<p>&lt;bloc{KELVIN}quote&gt;</p>\n"),
            "kelvin sign is not `k`",
        ),
        // Type 7: any tag name at all, on a line of its own.
        (
            format!("<{LONG_S}script>\n"),
            format!("<p>&lt;{LONG_S}script&gt;</p>\n"),
            "tag names are ASCII",
        ),
        (
            format!("<script{LONG_S}>\n"),
            format!("<p>&lt;script{LONG_S}&gt;</p>\n"),
            "tag names are ASCII",
        ),
        (
            format!("<{KELVIN}script>\n"),
            format!("<p>&lt;{KELVIN}script&gt;</p>\n"),
            "tag names are ASCII",
        ),
        // The ASCII case-insensitivity the folding is actually there for.
        (
            "<SCRIPT>\nx\n</SCRIPT>\n".to_string(),
            "<SCRIPT>\nx\n</SCRIPT>\n".to_string(),
            "uppercase type 1",
        ),
        (
            "<ScRiPt>\nx\n</ScRiPt>\n".to_string(),
            "<ScRiPt>\nx\n</ScRiPt>\n".to_string(),
            "mixed case type 1",
        ),
        (
            "<LINK>\n".to_string(),
            "<LINK>\n".to_string(),
            "uppercase type 6",
        ),
        (
            "<H1>\n".to_string(),
            "<H1>\n".to_string(),
            "uppercase type 6, with a digit class in the pattern",
        ),
        (
            "<TEXTAREA>\n".to_string(),
            "<TEXTAREA>\n".to_string(),
            "uppercase type 1",
        ),
        (
            "<Anything>\n".to_string(),
            "<Anything>\n".to_string(),
            "type 7",
        ),
    ];
    for (input, want, why) in cases {
        assert_eq!(
            to_html(&input, &Options::COMMONMARK),
            want,
            "{why}: {input:?}"
        );
    }
}

#[test]
fn non_ascii() {
    for (input, want) in [
        ("café naïve *ém*\n", "<p>café naïve <em>ém</em></p>\n"),
        ("**日本語**\n", "<p><strong>日本語</strong></p>\n"),
        ("[link](/ü)\n", "<p><a href=\"/%C3%BC\">link</a></p>\n"),
        ("*héllo* wörld 🎉\n", "<p><em>héllo</em> wörld 🎉</p>\n"),
        // Emphasis flanking is decided on Unicode character classes, so
        // it must see characters and not bytes.
        ("*«a»*\n", "<p><em>«a»</em></p>\n"),
    ] {
        assert_eq!(to_html(input, &Options::default()), want, "in {input:?}");
    }
}

/// Pins the unit of a sourcepos column.
///
/// Columns count characters. Rust offsets are byte indices and the
/// canonical runtime's are UTF-16 indices, so both have to convert. The
/// AST carries no sourcepos, so the AST-level parity check between the
/// runtimes cannot catch a regression in this.
#[test]
fn sourcepos_columns_count_characters() {
    let end_column = |src: &str| -> Option<usize> {
        let tree = parse_tree(src, &Options::default());
        let mut walker = tree.walker(tree.root());
        while let Some(event) = walker.next(&tree) {
            if event.entering && tree.get(event.node).node_type == NodeType::Paragraph {
                return Some(tree.get(event.node).sourcepos[1][1]);
            }
        }
        None
    };

    for (input, want, why) in [
        ("abc *x*\n", 7, "ascii"),
        ("é *x*\n", 5, "BMP: two bytes, one character"),
        ("😀 *x*\n", 5, "astral: four bytes, still one character"),
        ("𝔘𝔘 *x*\n", 6, "two astral characters"),
    ] {
        assert_eq!(end_column(input), Some(want), "{why}: {input:?} end column");
    }
}

/// Pins where percent-encoding happens.
///
/// `normalize_uri` belongs to rendering. Applying it at parse time was
/// invisible in the HTML (it is idempotent over its own output, so
/// conformance stayed at 652/652) but it put "/%C3%A4" in the public AST
/// where mdast, and every previous release, promises "/ä".
#[test]
fn destinations_decoded_in_ast() {
    let first = |src: &str| -> serde_json::Value {
        let doc = to_json(&parse_document(src, &Options::default()));
        doc["children"][0]["children"][0].clone()
    };

    for (input, want_url, want_html) in [
        ("[l](/ä)", "/ä", "<p><a href=\"/%C3%A4\">l</a></p>\n"),
        (
            "![i](/ö)",
            "/ö",
            "<p><img src=\"/%C3%B6\" alt=\"i\" /></p>\n",
        ),
        (
            "<https://x.com/ä>",
            "https://x.com/ä",
            "<p><a href=\"https://x.com/%C3%A4\">https://x.com/ä</a></p>\n",
        ),
        // Already encoded: left alone, not double-encoded.
        ("[l](/a%20b)", "/a%20b", "<p><a href=\"/a%20b\">l</a></p>\n"),
    ] {
        assert_eq!(first(input)["url"], want_url, "{input:?}: AST url");
        assert_eq!(
            to_html(input, &Options::default()),
            want_html,
            "{input:?}: HTML"
        );
    }
}

/// Nesting depth is chosen by the input, and the whole pipeline (block
/// phase, inline phase, projection, rendering) stays off the call stack
/// for it: 8000 levels of input on the 2 MB stack a test thread has.
///
/// The one recursive step left is outside this crate: dropping the
/// projected `tabnas::Value`, and converting it with `to_json`, both walk
/// its nesting on the call stack. That is what `MAX_CONTAINER_NESTING`
/// and `MAX_INLINE_NESTING` bound, so the value this returns is shallow
/// however deep the input runs, and an ordinary caller may drop it.
/// The large stack below is belt and braces for the intermediate work,
/// not a requirement on callers. `nesting_is_capped_at_the_constants`
/// pins the boundary itself; DIVERGENCE.md records the cap.
#[test]
fn deep_nesting_stays_off_the_stack() {
    let opts = Options::default();
    for src in [
        format!("{}x", "> ".repeat(8000)),
        format!("{}x", "- ".repeat(8000)),
        format!("{}x{}", "*".repeat(8000), "*".repeat(8000)),
        format!("{}x{}", "[".repeat(8000), "]".repeat(8000)),
    ] {
        let html = to_html(&src, &opts);
        assert!(!html.is_empty());
        let tree = parse_tree(&src, &opts);
        assert_eq!(render_html_with(&tree, opts), html);
        let ast = parse_document(&src, &opts);
        assert!(!ast.is_undefined());
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || drop(ast))
            .expect("spawns")
            .join()
            .expect("the drop completes");
    }

    // What the 2 MB test thread absorbs on its own, drop included.
    for src in [
        format!("{}x", "> ".repeat(1000)),
        format!("{}x", "- ".repeat(500)),
        format!("{}x{}", "*".repeat(1000), "*".repeat(1000)),
        format!("{}x{}", "[".repeat(2000), "]".repeat(2000)),
    ] {
        let ast = parse_document(&src, &opts);
        assert!(!ast.is_undefined());
        drop(ast);
    }
}

/// Pins the two nesting caps at their boundary, which is what makes
/// them a divergence a reader can check rather than a claim in prose.
///
/// A document at the bound parses as the canonical TypeScript does. Past
/// it the extra markers stay literal text, so the tree stops growing and
/// the `tabnas::Value` projected from it stays inside the headroom
/// `deep_nesting_stays_off_the_stack` describes. DIVERGENCE.md records
/// both bounds and the measurements behind them.
#[test]
fn nesting_is_capped_at_the_constants() {
    let opts = Options::default();

    // The deepest run of matching nodes anywhere in the tree, counted
    // over every descendant: past the bound the wrappers that do form
    // sit beside the literal text, not above it.
    fn deepest(
        tree: &tabnas_markdown::Tree,
        at: tabnas_markdown::NodeId,
        want: fn(NodeType) -> bool,
    ) -> usize {
        let here = usize::from(want(tree.get(at).node_type));
        let below = tree
            .children(at)
            .into_iter()
            .map(|c| deepest(tree, c, want))
            .max()
            .unwrap_or(0);
        here + below
    }
    let depth = |src: String, want: fn(NodeType) -> bool| -> usize {
        let tree = parse_tree(&src, &opts);
        let root = tree.root();
        deepest(&tree, root, want)
    };

    let quotes = |n: usize| format!("{}x", "> ".repeat(n));
    let is_quote = |t: NodeType| NodeType::BlockQuote == t;
    assert_eq!(
        depth(quotes(MAX_CONTAINER_NESTING), is_quote),
        MAX_CONTAINER_NESTING
    );
    assert_eq!(
        depth(quotes(MAX_CONTAINER_NESTING + 1), is_quote),
        MAX_CONTAINER_NESTING
    );
    assert_eq!(
        depth(quotes(MAX_CONTAINER_NESTING + 50), is_quote),
        MAX_CONTAINER_NESTING
    );

    // A list marker is two containers, the list and its item, so the
    // bound is reached at half as many markers. Each container type is
    // counted on its own, never as a union: the table's cell reads "50
    // lists and items EACH", and a regression yielding 100 lists and no
    // items would satisfy a combined count of 100 while changing exactly
    // what the cell records.
    let items = |n: usize| format!("{}x", "- ".repeat(n));
    let is_list = |t: NodeType| NodeType::List == t;
    let is_item = |t: NodeType| NodeType::Item == t;
    // `half + 1` is the row DIVERGENCE.md actually records as the first
    // past the bound -- 51 markers, 50 each. Measuring only `half` and
    // twice it left that row pinned by the HTML check alone, which says
    // `- x` appears inside some item and not that the tree stopped at 50.
    let half = MAX_CONTAINER_NESTING / 2;
    for markers in [half, half + 1, MAX_CONTAINER_NESTING] {
        let src = items(markers);
        assert_eq!(
            depth(src.clone(), is_list),
            half,
            "{markers} markers, lists"
        );
        assert_eq!(depth(src, is_item), half, "{markers} markers, items");
    }

    // A run of stars pairs up: `**` a side is one `strong`, so 2n stars
    // a side nest n wrappers. The table names `strong`, so `strong` is
    // what is counted -- and `emph` is pinned at zero beside it, because
    // paired stars coming back as emphasis would be a different cell
    // than the one recorded, and a union of the two would not notice.
    let stars = |n: usize| format!("{}x{}", "*".repeat(n), "*".repeat(n));
    let is_strong = |t: NodeType| NodeType::Strong == t;
    let is_emph = |t: NodeType| NodeType::Emph == t;
    for side in [
        2 * MAX_INLINE_NESTING,
        2 * MAX_INLINE_NESTING + 2,
        4 * MAX_INLINE_NESTING,
    ] {
        assert_eq!(
            depth(stars(side), is_strong),
            MAX_INLINE_NESTING,
            "{side} stars"
        );
        assert_eq!(depth(stars(side), is_emph), 0, "{side} stars");
    }

    // Nothing the reader wrote disappears: the markers past the bound
    // stay in the output AS TEXT, which is the half of the DIVERGENCE.md
    // table that `contains('x')` never checked. A capped path that kept
    // the content and dropped the overflow markers passed that, while the
    // recorded output had changed.
    //
    // Every string below was read off this port before being pinned.

    // One marker past the bound: the `> ` reaches the innermost paragraph
    // as literal text, escaped as `&gt;` like any other `>`.
    assert!(
        to_html(&quotes(MAX_CONTAINER_NESTING + 1), &opts).contains("<p>&gt; x</p>"),
        "the overflow block-quote marker did not stay literal"
    );
    // Three past: all three, in order, in the one paragraph.
    assert!(
        to_html(&quotes(MAX_CONTAINER_NESTING + 3), &opts).contains("<p>&gt; &gt; &gt; x</p>"),
        "the three overflow block-quote markers did not stay literal"
    );

    // A list marker is not escaped, so the whole `- ` survives verbatim
    // inside the innermost item.
    assert!(
        to_html(&items(MAX_CONTAINER_NESTING / 2 + 1), &opts).contains("<li>- x</li>"),
        "the overflow list marker did not stay literal"
    );

    // The extra stars surface OUTSIDE the wrapper run rather than beside
    // the content, which is why a scan around the `x` would have missed
    // them: the opener that cannot pair is left where it was written.
    let over = to_html(&stars(2 * MAX_INLINE_NESTING + 2), &opts);
    assert!(
        over.starts_with("<p>**<strong>"),
        "the overflow emphasis stars did not stay literal: {:.40}",
        over
    );
    assert!(
        over.ends_with("</strong>**</p>\n"),
        "the closing overflow stars did not stay literal: {:.40}",
        &over[over.len().saturating_sub(40)..]
    );
    assert_eq!(
        over.matches("<strong>").count(),
        MAX_INLINE_NESTING,
        "the wrapper count moved"
    );
    assert_eq!(over.matches('*').count(), 4, "the literal star count moved");
}
