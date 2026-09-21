// The plugin path, end to end: an engine instance with the Markdown
// plugin installed, parsing through `Tabnas::parse`. Mirrors
// ts/test/markdown.test.ts and go/markdown_test.go.

mod common;

use common::to_json;
use serde_json::json;
use tabnas::Tabnas;
use tabnas_markdown::{make, make_with, markdown, parse, plugin, Options};

/// A fresh instance per call, exactly as the other runtimes' `mdParse`.
fn md_parse(src: &str, opts: Option<Options>) -> serde_json::Value {
    let tn = match opts {
        Some(opts) => make_with(&opts),
        None => make(),
    };
    to_json(&tn.parse(src).expect("markdown never fails to parse"))
}

#[test]
fn empty() {
    let want = json!({"type": "document", "children": []});
    assert_eq!(md_parse("", None), want, "empty");
    assert_eq!(md_parse("\n", None), want, "blank");
}

#[test]
fn atx_heading() {
    assert_eq!(
        md_parse("# Hello", None),
        json!({"type": "document", "children": [
            {"type": "heading", "depth": 1, "children": [{"type": "text", "value": "Hello"}]}
        ]})
    );
    assert_eq!(
        md_parse("## Heading 2", None),
        json!({"type": "document", "children": [
            {"type": "heading", "depth": 2, "children": [{"type": "text", "value": "Heading 2"}]}
        ]})
    );
}

#[test]
fn setext_heading() {
    assert_eq!(
        md_parse("Foo\n===", None),
        json!({"type": "document", "children": [
            {"type": "heading", "depth": 1, "children": [{"type": "text", "value": "Foo"}]}
        ]})
    );
    assert_eq!(
        md_parse("Bar\n---", None),
        json!({"type": "document", "children": [
            {"type": "heading", "depth": 2, "children": [{"type": "text", "value": "Bar"}]}
        ]})
    );
}

#[test]
fn thematic_break() {
    assert_eq!(
        md_parse("---", None),
        json!({"type": "document", "children": [{"type": "thematicBreak"}]})
    );
}

#[test]
fn paragraph() {
    assert_eq!(
        md_parse("Hello world", None),
        json!({"type": "document", "children": [
            {"type": "paragraph", "children": [{"type": "text", "value": "Hello world"}]}
        ]})
    );
    let two = md_parse("Hello\n\nWorld", None);
    assert_eq!(two["children"].as_array().expect("children").len(), 2);
}

#[test]
fn fenced_code() {
    assert_eq!(
        md_parse("```\ncode here\n```", None),
        json!({"type": "document", "children": [
            {"type": "code", "lang": null, "meta": null, "value": "code here"}
        ]})
    );
    assert_eq!(
        md_parse("```js\nconsole.log(\"hi\")\n```", None),
        json!({"type": "document", "children": [
            {"type": "code", "lang": "js", "meta": null, "value": "console.log(\"hi\")"}
        ]})
    );
}

#[test]
fn indented_code() {
    assert_eq!(
        md_parse("    indented\n    second", None),
        json!({"type": "document", "children": [
            {"type": "code", "lang": null, "meta": null, "value": "indented\nsecond"}
        ]})
    );
}

#[test]
fn blockquote() {
    let doc = md_parse("> hello\n> world", None);
    assert_eq!(doc["children"][0]["type"], "blockquote");
}

#[test]
fn lists() {
    let doc = md_parse("- a\n- b\n- c", None);
    let list = &doc["children"][0];
    assert_eq!(list["ordered"], false);
    assert_eq!(list["children"].as_array().expect("items").len(), 3);

    let doc = md_parse("1. a\n2. b\n3. c", None);
    let list = &doc["children"][0];
    assert_eq!(list["ordered"], true);
    assert_eq!(list["start"], 1);
}

#[test]
fn inline() {
    let doc = md_parse("Hello *world*", None);
    let children = doc["children"][0]["children"].as_array().expect("inlines");
    assert_eq!(children.len(), 2);
    assert_eq!(children[1]["type"], "emphasis");

    let doc = md_parse("Hello **world**", None);
    assert_eq!(doc["children"][0]["children"][1]["type"], "strong");

    let doc = md_parse("`code`", None);
    assert_eq!(doc["children"][0]["children"][0]["type"], "inlineCode");

    let doc = md_parse("foo_bar_baz", None);
    assert_eq!(doc["children"][0]["children"][0]["value"], "foo_bar_baz");
}

#[test]
fn links() {
    let doc = md_parse("[link](https://example.com)", None);
    let link = &doc["children"][0]["children"][0];
    assert_eq!(link["type"], "link");
    assert_eq!(link["url"], "https://example.com");

    let doc = md_parse("![alt](https://example.com/img.png)", None);
    assert_eq!(doc["children"][0]["children"][0]["type"], "image");

    let doc = md_parse("<https://example.com>", None);
    assert_eq!(doc["children"][0]["children"][0]["type"], "link");
}

#[test]
fn strikethrough() {
    let doc = md_parse("~~delete~~", None);
    assert_eq!(doc["children"][0]["children"][0]["type"], "delete");

    let doc = md_parse("~~delete~~", Some(Options::COMMONMARK));
    assert_eq!(doc["children"][0]["children"][0]["type"], "text");
}

/// Bare engine: the package depends on the engine crate only, and the
/// typed installer works on an instance the caller configured.
#[test]
fn bare_and_compat() {
    let mut tn = Tabnas::new();
    markdown(&mut tn, &Options::default()).expect("installs");
    let doc = to_json(&tn.parse("# hi").expect("parses"));
    assert_eq!(doc["type"], "document");
    assert_eq!(doc["children"][0]["type"], "heading");
}

/// The plugin descriptor path: options come from the bag, defaults fill
/// the rest, and the instance records the install.
#[test]
fn plugin_descriptor() {
    let mut tn = Tabnas::new();
    tn.use_plugin(plugin(), None).expect("installs");
    let doc = to_json(&tn.parse("~~x~~").expect("parses"));
    assert_eq!(doc["children"][0]["children"][0]["type"], "delete");

    let mut tn = Tabnas::new();
    let bag = tabnas::Value::from_json(&json!({"gfm": false}));
    tn.use_plugin(plugin(), Some(bag)).expect("installs");
    let doc = to_json(&tn.parse("~~x~~").expect("parses"));
    assert_eq!(doc["children"][0]["children"][0]["type"], "text");

    let names: Vec<String> = tn
        .installed_plugins()
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, vec!["Markdown".to_string()]);
}

/// The shared default instance, and that it is safe to share.
#[test]
fn default_instance_is_shared_across_threads() {
    let doc = to_json(&parse("Hello *world*").expect("parses"));
    assert_eq!(doc["children"][0]["children"][1]["type"], "emphasis");

    let handles: Vec<_> = (0..4)
        .map(|i| {
            std::thread::spawn(move || {
                for _ in 0..50 {
                    let src =
                        format!("# T{i}\n\n- [x] a\n- b\n\n| a | b |\n| - | - |\n| {i} | y |\n");
                    let doc = to_json(&parse(&src).expect("parses"));
                    assert_eq!(doc["children"][0]["type"], "heading");
                    assert_eq!(doc["children"][2]["type"], "table");
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("a worker thread panicked");
    }
}

/// The two option-bearing entry points agree with the engine-free ones.
#[test]
fn breaks_option_reaches_the_ast() {
    let doc = md_parse(
        "a\nb",
        Some(Options {
            gfm: true,
            breaks: true,
        }),
    );
    assert_eq!(
        doc["children"][0]["children"],
        json!([{"type": "text", "value": "a"}, {"type": "break"}, {"type": "text", "value": "b"}])
    );
    let doc = md_parse("a\nb", None);
    assert_eq!(
        doc["children"][0]["children"],
        json!([{"type": "text", "value": "a b"}])
    );
}
