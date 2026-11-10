// References stay unresolved in the AST, as mdast keeps them: a bracketed
// label is a `linkReference` or an `imageReference` naming it, whether or
// not the document defines it, and each definition is a `definition` node
// where it stands. `resolve_references` gives the tree back with the
// references resolved, as CommonMark reads a document, and this file holds
// it to that reading over both conformance corpora: it gives CommonMark's
// tree for every example but the five registered below, where a label with
// no definition decided how the text around it nests. The definitions and
// references themselves are pinned in every runtime by
// test/spec/reference.tsv; ts/test/references.test.ts and
// go/references_test.go hold the other runtimes to this file.

mod common;

use common::{load_gfm_cases, load_spec_cases, to_json, SpecCase};
use tabnas_markdown::{
    commonmark, parse_document, resolve_references, to_ast, Options, MAX_CONTAINER_NESTING,
};

/// The CommonMark examples where resolving the tree's references does not
/// give the tree CommonMark reads, each with why: the declared loss. An
/// example that comes to resolve as CommonMark reads it fails here, and is
/// then deleted.
const RESOLVED_OTHERWISE: [(u64, &str); 5] = [
    (
        512,
        "the inner [bar] reads as a reference, so the outer link text holds a link and is no link",
    ),
    (
        523,
        "the [bar* baz] reads as a reference, so the emphasis cannot cross into it",
    ),
    (
        528,
        "the inner [bar] reads as a reference, so the outer link text holds a link and is no link",
    ),
    (
        569,
        "[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]",
    ),
    (
        571,
        "[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]",
    ),
];

/// A value as JSON text, members in their order.
fn text(value: &tabnas::Value) -> String {
    serde_json::to_string(&to_json(value)).expect("serializes")
}

/// The tree with its references resolved, as JSON text.
fn resolved(src: &str, opts: &Options) -> String {
    text(&resolve_references(&parse_document(src, opts)))
}

/// CommonMark's reading of a document as an AST, without its definitions.
fn commonmark_reading(src: &str, opts: &Options) -> String {
    fn strip(node: serde_json::Value) -> serde_json::Value {
        let serde_json::Value::Object(map) = node else {
            return node;
        };
        serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| match (k.as_str(), v) {
                    ("children", serde_json::Value::Array(children)) => (
                        k,
                        serde_json::Value::Array(
                            children
                                .into_iter()
                                .filter(|c| c["type"] != "definition")
                                .map(strip)
                                .collect(),
                        ),
                    ),
                    (_, v) => (k, v),
                })
                .collect(),
        )
    }
    let reading = to_json(&to_ast(&commonmark::parse(src, *opts), opts));
    serde_json::to_string(&strip(reading)).expect("serializes")
}

fn json(text: &str) -> String {
    serde_json::to_string(&serde_json::from_str::<serde_json::Value>(text).expect("JSON"))
        .expect("serializes")
}

#[test]
fn a_defined_reference_resolves_to_its_link_the_first_definition_winning() {
    assert_eq!(
        resolved(
            "[a] ![b][a] [c][]\n\n[a]: /1 \"t\"\n[A]: /2\n[c]: /3\n",
            &Options::default()
        ),
        json(
            r#"{"type":"document","children":[{"type":"paragraph","children":[
                {"type":"link","url":"/1","title":"t","children":[{"type":"text","value":"a"}]},
                {"type":"text","value":" "},
                {"type":"image","url":"/1","title":"t","alt":"b"},
                {"type":"text","value":" "},
                {"type":"link","url":"/3","title":null,"children":[{"type":"text","value":"c"}]}]}]}"#
        )
    );
}

#[test]
fn a_reference_with_no_definition_reads_as_its_source_text() {
    assert_eq!(
        resolved("x [a] [b][] [c *d*][e] ![f][g] y\n", &Options::default()),
        json(
            r#"{"type":"document","children":[{"type":"paragraph","children":[
                {"type":"text","value":"x [a] [b][] [c "},
                {"type":"emphasis","children":[{"type":"text","value":"d"}]},
                {"type":"text","value":"][e] ![f][g] y"}]}]}"#
        )
    );
}

#[test]
fn resolving_leaves_the_tree_it_was_given_as_it_was() {
    let doc = parse_document("[a]\n\n[a]: /u\n", &Options::default());
    let before = text(&doc);
    resolve_references(&doc);
    assert_eq!(text(&doc), before);
}

/// The deepest tree the crate builds: block quotes nest to
/// `MAX_CONTAINER_NESTING` (DIVERGENCE.md records the cap).
#[test]
fn resolving_the_deepest_tree_reaches_its_last_level() {
    let doc = parse_document(
        &format!("{}[a]\n\n[a]: /u\n", "> ".repeat(MAX_CONTAINER_NESTING)),
        &Options::default(),
    );
    let resolved = resolve_references(&doc);
    let mut node = &resolved;
    let mut depth = 0;
    loop {
        let tabnas::Value::Object(map) = node else {
            panic!("not an object")
        };
        let Some(tabnas::Value::Array(children)) = map.get("children") else {
            panic!("no children")
        };
        let first = &children[0];
        let tabnas::Value::Object(child) = first else {
            panic!("not an object")
        };
        match child.get("type") {
            Some(tabnas::Value::String(t)) if t.as_str() == "blockquote" => {
                node = first;
                depth += 1;
            }
            _ => break,
        }
    }
    assert_eq!(depth, MAX_CONTAINER_NESTING);
    let tabnas::Value::Object(map) = node else {
        panic!("not an object")
    };
    let Some(tabnas::Value::Array(children)) = map.get("children") else {
        panic!("no children")
    };
    let link = to_json(&children[0])["children"][0]["type"].clone();
    assert_eq!(link, "link");
}

fn otherwise(cases: &[SpecCase], opts: &Options) -> Vec<u64> {
    cases
        .iter()
        .filter(|c| resolved(&c.markdown, opts) != commonmark_reading(&c.markdown, opts))
        .map(|c| c.example)
        .collect()
}

#[test]
fn every_commonmark_example_resolves_as_commonmark_reads_it_but_the_registered() {
    let registered: Vec<u64> = RESOLVED_OTHERWISE.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        otherwise(&load_spec_cases(), &Options::COMMONMARK),
        registered
    );
}

#[test]
fn every_gfm_example_resolves_as_commonmark_reads_it() {
    assert_eq!(
        otherwise(&load_gfm_cases(), &Options::default()),
        Vec::<u64>::new()
    );
}
