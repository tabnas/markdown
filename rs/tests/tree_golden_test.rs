// Golden native-tree snapshots, shared with the TypeScript and Go
// runtimes.
//
// The `test/spec/*.tsv` fixtures compare public ASTs, and the projection
// drops sourcepos, so a positional divergence between the runtimes is
// invisible to them. This suite pins the NATIVE tree instead: every
// fixture input is parsed with `parse_tree` and serialized to the
// canonical JSON shape all three runtimes implement (see `serialize_tree`
// here, in ts/test/tree-golden.test.ts and in go/tree_golden_test.go),
// then compared against `test/spec/tree/<name>.json`. The goldens are
// generated from the canonical TypeScript:
//
//	cd ts && npm run build && MD_TREE_GOLDEN=write npm test
//
// so this suite failing means the Rust native tree, sourcepos included,
// has drifted from the TypeScript one.

mod common;

use std::path::Path;

use common::normalize_numbers;
use serde_json::{json, Map, Value};
use tabnas_markdown::{parse_tree, NodeId, NodeType, Options, Tree};
use tabnas_support::{find_spec_dir, load_spec_dir, unescape, SpecOptions};

/// Node types whose literal is meaningful. Kept as an explicit list
/// because the tree cannot distinguish an absent literal from an empty
/// one; the serializers include the field for exactly these types.
fn has_literal(t: NodeType) -> bool {
    matches!(
        t,
        NodeType::Text
            | NodeType::Code
            | NodeType::HtmlInline
            | NodeType::CodeBlock
            | NodeType::HtmlBlock
    )
}

/// The canonical cross-runtime serialization. It must stay
/// field-for-field identical to `serializeTree` in the other two
/// runtimes' suites.
fn serialize_tree(tree: &Tree, id: NodeId) -> Value {
    let n = tree.get(id);
    let mut out = Map::new();
    out.insert("type".into(), json!(n.node_type.as_str()));
    out.insert("sourcepos".into(), json!(n.sourcepos));

    if has_literal(n.node_type) {
        out.insert("literal".into(), json!(n.literal));
    }
    if n.node_type == NodeType::Heading {
        out.insert("level".into(), json!(n.level));
    }
    if n.node_type == NodeType::Link || n.node_type == NodeType::Image {
        out.insert("destination".into(), json!(n.destination));
        if let Some(title) = &n.title {
            out.insert("title".into(), json!(title));
        }
    }
    if n.node_type == NodeType::CodeBlock {
        out.insert("isFenced".into(), json!(n.is_fenced));
        if n.is_fenced {
            out.insert("info".into(), json!(n.info.clone().unwrap_or_default()));
            out.insert(
                "fenceChar".into(),
                json!((n.fence_char as char).to_string()),
            );
            out.insert("fenceLength".into(), json!(n.fence_length));
            out.insert("fenceOffset".into(), json!(n.fence_offset));
        }
    }
    if let Some(data) = &n.list_data {
        out.insert(
            "listData".into(),
            json!({
                "type": data.list_type.as_str(),
                "tight": data.tight,
                "start": data.start,
                "delimiter": data.delimiter,
                "bulletChar": data.bullet_char,
                "padding": data.padding,
                "markerOffset": data.marker_offset,
            }),
        );
    }
    if let Some(align) = &n.table_align {
        let arr: Vec<Value> = align
            .iter()
            .map(|a| match a {
                Some(a) => json!(a.as_str()),
                None => Value::Null,
            })
            .collect();
        out.insert("tableAlign".into(), Value::Array(arr));
    }
    if n.is_header_row {
        out.insert("isHeaderRow".into(), json!(true));
    }
    if let Some(checked) = n.checked {
        out.insert("checked".into(), json!(checked));
    }
    if n.node_type == NodeType::Document {
        out.insert("gfm".into(), json!(n.gfm));
    }

    let children: Vec<Value> = tree
        .children(id)
        .into_iter()
        .map(|c| serialize_tree(tree, c))
        .collect();
    if !children.is_empty() {
        out.insert("children".into(), Value::Array(children));
    }

    Value::Object(out)
}

#[test]
fn tree_goldens() {
    let dir = find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("test/spec is found above rs/");
    let files = load_spec_dir(&dir, &SpecOptions::default()).expect("the fixtures load");
    assert!(!files.is_empty(), "no spec fixtures found");

    let mut failures = Vec::new();

    for spec in &files {
        let stem = spec.file.strip_suffix(".tsv").unwrap_or(&spec.file);
        let golden_path = dir.join("tree").join(format!("{stem}.json"));
        let raw = std::fs::read_to_string(&golden_path).unwrap_or_else(|error| {
            panic!(
                "tree golden missing for {}: {error} (regenerate: cd ts && npm run build && MD_TREE_GOLDEN=write npm test)",
                spec.file
            )
        });
        let golden: Vec<Value> = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("{}: {error}", golden_path.display()));
        assert_eq!(
            golden.len(),
            spec.rows.len(),
            "{}: {} golden cases for {} fixture rows; regenerate the goldens",
            spec.file,
            golden.len(),
            spec.rows.len()
        );

        for (i, g) in golden.iter().enumerate() {
            let opts_raw = g["opts"].as_str().unwrap_or("").trim();
            let opts = if opts_raw.is_empty() {
                Options::default()
            } else {
                let bag: Value = serde_json::from_str(opts_raw)
                    .unwrap_or_else(|error| panic!("{} case {i} opts: {error}", spec.file));
                Options::resolve_json(&bag)
            };

            let input = unescape(g["input"].as_str().expect("input"));
            let tree = parse_tree(&input, &opts);
            let got = normalize_numbers(serialize_tree(&tree, tree.root()));
            let want = normalize_numbers(g["tree"].clone());
            if got != want {
                failures.push(format!(
                    "{} {}: native tree diverges from golden\n got: {got}\nwant: {want}",
                    spec.file,
                    spec.rows[i].location()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} divergences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The goldens cover every fixture file: a fixture without a golden is a
/// gap in the positional contract, not a file to skip.
#[test]
fn every_fixture_has_a_golden() {
    let dir = find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("test/spec is found above rs/");
    let files = load_spec_dir(&dir, &SpecOptions::default()).expect("the fixtures load");
    assert_eq!(files.len(), 10, "the corpus holds ten fixture files");
    let rows: usize = files.iter().map(|f| f.rows.len()).sum();
    assert_eq!(rows, 83, "the corpus holds 83 rows");
    for spec in &files {
        let stem = spec.file.strip_suffix(".tsv").unwrap_or(&spec.file);
        assert!(
            dir.join("tree").join(format!("{stem}.json")).is_file(),
            "{}: no tree golden",
            spec.file
        );
    }
}

/// A loose list loosens the `listData` of the item that OPENED it, and
/// only that one. The canonical runtime hands one `listData` object to
/// both the new list and its first item, and Go shares the pointer, so
/// `tight = false` on the list shows through on that item; later items
/// keep their own `tight: true`. No fixture holds a loose list, so the
/// goldens above never see this; it is pinned here on the same
/// serialization, with the values the TypeScript produces.
#[test]
fn loose_list_loosens_the_opening_item_only() {
    let tight = |v: &Value| v["listData"]["tight"].as_bool().expect("tight");
    let items = |v: &Value| -> Vec<bool> {
        v["children"]
            .as_array()
            .expect("children")
            .iter()
            .map(tight)
            .collect()
    };

    for (input, list_tight, item_tights) in [
        ("- a\n- b\n\n- c", false, vec![false, true, true]),
        ("- a\n\n  b", false, vec![false]),
        ("1. a\n\n   b\n2. c", false, vec![false, true]),
        ("- a\n- b\n- c", true, vec![true, true, true]),
    ] {
        let tree = parse_tree(input, &Options::default());
        let root = serialize_tree(&tree, tree.root());
        let list = &root["children"][0];
        assert_eq!(tight(list), list_tight, "{input:?}: list tight");
        assert_eq!(items(list), item_tights, "{input:?}: item tight values");
    }

    // A loose SUBLIST loosens its own opening item and nothing outside it.
    let tree = parse_tree("- a\n  - b\n\n\n  - c", &Options::default());
    let root = serialize_tree(&tree, tree.root());
    let outer = &root["children"][0];
    assert!(tight(outer), "outer list stays tight");
    assert_eq!(items(outer), vec![true]);
    let sub = &outer["children"][0]["children"][1];
    assert_eq!(sub["type"], "list");
    assert!(!tight(sub), "sublist is loose");
    assert_eq!(items(sub), vec![false, true]);
}
