// Every repetition is a replace loop, never a push chain (alchemy's
// AGENTS.md, "The reader repeats by replacement").
//
// A container's children are the `children` rule, which goes on by
// replacement (`r`) from one child to the next, so a document of ten
// thousand blocks, or a list of ten thousand items, or a paragraph of ten
// thousand inline nodes, runs at the rule depth one needs. Real nesting
// still nests, as it should: a block quote inside a block quote is a level
// deeper. The rule depth is the engine's `d`, read by a rule subscriber on
// every step of the parse. ts/test/repeat.test.ts and go/repeat_test.go
// hold the other runtimes to the same.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The deepest rule a parse of `src` reaches.
fn max_depth(src: &str) -> usize {
    let mut parser = tabnas_markdown::make();
    let max = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&max);
    parser.subscribe_rules(move |rule, _context| {
        seen.fetch_max(rule.d, Ordering::Relaxed);
    });
    if let Err(error) = parser.parse(src) {
        panic!("{:?} does not parse: {error}", &src[..src.len().min(40)]);
    }
    max.load(Ordering::Relaxed)
}

const N: usize = 10_000;

type Many = fn(usize) -> String;

/// Each construct, one of it, and many of it: the same depth.
const REPEATED: [(&str, &str, Many); 21] = [
    ("paragraphs", "a\n", |n| "a\n\n".repeat(n)),
    ("ATX headings", "# h\n", |n| "# h\n".repeat(n)),
    ("setext headings", "h\n=\n", |n| "h\n=\n\n".repeat(n)),
    ("thematic breaks", "***\n", |n| "***\n".repeat(n)),
    ("fenced code blocks", "```\nx\n```\n", |n| {
        "```\nx\n```\n".repeat(n)
    }),
    ("indented code blocks", "    x\n\na\n", |n| {
        "    x\n\na\n\n".repeat(n)
    }),
    ("HTML blocks", "<div>\n</div>\n", |n| {
        "<div>\n</div>\n\n".repeat(n)
    }),
    ("definitions", "[a]: /u\n", |n| "[a]: /u\n".repeat(n)),
    ("list items", "- a\n", |n| "- a\n".repeat(n)),
    ("loose list items", "- a\n", |n| "- a\n\n".repeat(n)),
    ("ordered list items", "1. a\n", |n| "1. a\n".repeat(n)),
    ("paragraphs in a block quote", "> a\n", |n| {
        "> a\n>\n".repeat(n)
    }),
    ("lines of a paragraph", "a\n", |n| "a\n".repeat(n)),
    ("lines of a code block", "```\nx\n```\n", |n| {
        format!("```\n{}```\n", "x\n".repeat(n))
    }),
    ("table rows", "| a |\n| - |\n| b |\n", |n| {
        format!("| a |\n| - |\n{}", "| b |\n".repeat(n))
    }),
    ("table cells", "| a |\n| - |\n", |n| {
        format!("{}|\n{}|\n", "| a ".repeat(n), "| - ".repeat(n))
    }),
    ("emphasis in a paragraph", "*a*\n", |n| {
        format!("{}\n", "*a* ".repeat(n))
    }),
    ("links in a paragraph", "[a](/u)\n", |n| {
        format!("{}\n", "[a](/u) ".repeat(n))
    }),
    ("references in a paragraph", "[a]\n", |n| {
        format!("{}\n", "[a] ".repeat(n))
    }),
    ("code spans in a paragraph", "`a`\n", |n| {
        format!("{}\n", "`a` ".repeat(n))
    }),
    ("hard breaks in a paragraph", "a  \nb\n", |n| {
        format!("{}b\n", "a  \n".repeat(n))
    }),
];

#[test]
fn ten_thousand_of_each_construct_reach_the_depth_one_does() {
    for (what, one, many) in REPEATED {
        let d1 = max_depth(one);
        let dn = max_depth(&many(N));
        assert!(d1 > 0, "{what}: no rule ran");
        assert_eq!(dn, d1, "{what}: one reaches {d1}, {N} reach {dn}");
    }
}

#[test]
fn nesting_nests_each_block_quote_a_level_deeper() {
    // A block quote is a rule and its children's loop, two levels a quote.
    let d = |n: usize| max_depth(&format!("{}a\n", "> ".repeat(n)));
    assert_eq!(d(2) - d(1), 2);
    assert_eq!(d(100) - d(1), 2 * 99);
}

#[test]
fn nesting_nests_each_emphasis_a_level_deeper() {
    let d = |n: usize| max_depth(&format!("{}a{}\n", "*".repeat(n), "*".repeat(n)));
    // `***a***` is emphasis in strong: three delimiters, two nodes.
    assert!(d(3) > d(1));
}
