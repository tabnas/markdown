// Engine-path conformance: the full CommonMark 0.31.2 suite (652
// examples, gfm:false) and the GFM extension corpus (24 examples,
// gfm:true) run through `make_with(..)` and `parse_keep_tree` (the engine
// lexing lines, the rules dispatching them) with byte-for-byte HTML
// comparison and an AST comparison against the engine-free path.
//
// commonmark_test.rs asserts the same corpus over the engine-free
// modules; this suite is the other leg of the dual-path contract
// (dx-report §42): the conformance claim holds on the code path the
// plugin actually runs, not just on the reference implementation. The
// native tree comes back via the `keepTree` handshake and is rendered
// with the same `render_html` the engine-free path uses, so the
// comparison isolates the parse: a difference here is a parsing
// difference, never a rendering one. Mirrors
// ts/test/engine-conformance.test.ts and go/engine_conformance_test.go.

mod common;

use common::{load_gfm_cases, load_spec_cases, to_json, SpecCase, CM_OPTS, GFM_OPTS};
use tabnas::Tabnas;
use tabnas_markdown::{make_with, parse_document, parse_keep_tree, render_html, Options};

fn check_engine_example(tn: &Tabnas, c: &SpecCase, opts: &Options, failures: &mut Vec<String>) {
    let (ast, tree) = match parse_keep_tree(tn, &c.markdown) {
        Ok(result) => result,
        Err(error) => {
            failures.push(format!(
                "example {}: engine parse error: {error}",
                c.example
            ));
            return;
        }
    };
    let Some(tree) = tree else {
        failures.push(format!(
            "example {}: keepTree returned no native tree",
            c.example
        ));
        return;
    };

    let got = render_html(&tree, None);
    if got != c.html {
        failures.push(format!(
            "example {}: engine-path HTML\nmarkdown: {:?}\n     got: {:?}\n    want: {:?}",
            c.example, c.markdown, got, c.html
        ));
    }

    let direct = parse_document(&c.markdown, opts);
    if to_json(&ast) != to_json(&direct) {
        failures.push(format!(
            "example {}: engine-path AST diverges from engine-free path\nmarkdown: {:?}",
            c.example, c.markdown
        ));
    }
}

#[test]
fn engine_commonmark_spec() {
    let cases = load_spec_cases();
    let tn = make_with(&CM_OPTS);
    let mut failures = Vec::new();
    for c in &cases {
        check_engine_example(&tn, c, &CM_OPTS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn engine_gfm_spec() {
    let cases = load_gfm_cases();
    let tn = make_with(&GFM_OPTS);
    let mut failures = Vec::new();
    for c in &cases {
        check_engine_example(&tn, c, &GFM_OPTS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The handshake itself: a tree comes back only when asked for, never
/// for an empty source, and never stale.
#[test]
fn keep_tree_handshake() {
    let tn = make_with(&GFM_OPTS);

    let (ast, tree) = parse_keep_tree(&tn, "# hi\n").expect("parses");
    assert_eq!(to_json(&ast)["children"][0]["type"], "heading");
    let tree = tree.expect("a tree for a non-empty source");
    assert_eq!(render_html(&tree, None), "<h1>hi</h1>\n");

    let (ast, tree) = parse_keep_tree(&tn, "").expect("parses");
    assert_eq!(
        to_json(&ast),
        serde_json::json!({"type": "document", "children": []})
    );
    assert!(tree.is_none(), "the engine short-circuits an empty source");

    // A plain parse parks nothing.
    tn.parse("x\n").expect("parses");
    let (_, tree) = parse_keep_tree(&tn, "").expect("parses");
    assert!(tree.is_none());
}
