// The translation parts: what the manifest says and what the crate
// embeds are the same files.
//
// A packaged crate holds nothing outside `rs/`, so the crate embeds its
// own copies, `rs/translate/manifest.json` of `tabnas.plugin.json`,
// `rs/translate/lift.alc` of the lift the manifest names and
// `rs/translate/render.alc` of the render it names, as `manifest_text()`,
// `lift_text()` and `render_text()`. The copies are the only texts a host
// sees, so they must be the files: this holds the embedded manifest to
// the repository's, and each part the manifest names, read from the
// repository, to the embedded one, as it would an embed the manifest
// named. Change the file at the root and run `npm run embed` in `ts/`,
// which copies it into `rs/translate/`; this fails until both are the
// same.

mod common;

use std::collections::HashSet;
use std::fs;

use serde_json::{json, Value};

fn translate() -> Value {
    let manifest: Value =
        serde_json::from_str(tabnas_markdown::manifest_text()).expect("the manifest is JSON");
    manifest
        .get("translate")
        .cloned()
        .expect("the manifest carries a translate object")
}

/// The part `translate.<key>` names, read from the repository.
fn part_on_disk(key: &str) -> String {
    let translate = translate();
    let path = translate[key]
        .as_str()
        .unwrap_or_else(|| panic!("translate.{key} names a file"));
    fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.{key} names {path}, which cannot be read: {e}"))
}

/// The names a part defines, in order.
fn names(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("def "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect()
}

#[test]
fn the_manifest_the_crate_embeds_is_the_repositorys() {
    let on_disk = fs::read_to_string(common::repo_root().join("tabnas.plugin.json"))
        .expect("the repository has its manifest");
    assert_eq!(
        on_disk,
        tabnas_markdown::manifest_text(),
        "rs/translate/manifest.json is not tabnas.plugin.json: run npm run embed in ts"
    );
}

#[test]
fn the_lift_the_manifest_names_is_the_one_the_crate_embeds() {
    assert_eq!(
        part_on_disk("lift"),
        tabnas_markdown::lift_text(),
        "translate.lift names a file, and rs/translate/lift.alc, which lift_text() embeds, \
         is another text: run npm run embed in ts"
    );
}

#[test]
fn the_render_the_manifest_names_is_the_one_the_crate_embeds() {
    assert_eq!(
        part_on_disk("render"),
        tabnas_markdown::render_text(),
        "translate.render names a file, and rs/translate/render.alc, which render_text() \
         embeds, is another text: run npm run embed in ts"
    );
}

/// An embed takes a plain tree into a format's own schema. Markdown's
/// events carry an mdast tree, but its render writes from records, which
/// any tree's rows give, so its manifest names no embed and the crate
/// carries none; a manifest that named one would be held to its file
/// here, as the lift and the render are above.
#[test]
fn the_embed_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let parts = tabnas_markdown::translate().expect("Markdown carries translation parts");
    let Some(path) = translate.get("embed").and_then(Value::as_str) else {
        assert_eq!(
            parts.embed, None,
            "the manifest names no embed, and the crate carries one"
        );
        return;
    };
    let on_disk = fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.embed names {path}, which cannot be read: {e}"));
    let embed = parts
        .embed
        .unwrap_or_else(|| panic!("translate.embed names {path}, and the crate carries no embed"));
    assert_eq!(embed.entry, "markdown-embed");
    assert_eq!(
        embed.source,
        Some(on_disk.as_str()),
        "translate.embed names {path}, and the crate embeds another text: run npm run embed in ts"
    );
}

#[test]
fn the_structural_interface_names_both_entries() {
    let parts = tabnas_markdown::translate().expect("Markdown carries translation parts");
    assert_eq!(parts.manifest, tabnas_markdown::manifest_text());
    let lift = parts.lift.expect("Markdown carries a lift");
    assert_eq!(lift.entry, "markdown-lift");
    assert_eq!(lift.source, Some(tabnas_markdown::lift_text()));
    let render = parts.render.expect("Markdown carries a render");
    assert_eq!(render.entry, "markdown-render");
    assert_eq!(render.source, Some(tabnas_markdown::render_text()));
}

/// A Markdown table is read as records first, through the lift, and as
/// the document's tree second, an mdast tree rather than a plain one; it
/// is written from records, as a table, the records being the elements of
/// the root array. The manifest carries the languageId the host keys its
/// registry by.
#[test]
fn markdown_reads_records_through_a_lift_and_writes_records() {
    let manifest: Value =
        serde_json::from_str(tabnas_markdown::manifest_text()).expect("the manifest is JSON");
    assert_eq!(manifest["languageId"], "markdown");
    assert_eq!(manifest["pluginKind"], "grammar");
    let translate = translate();
    assert_eq!(translate["reads"], json!(["records", "tree"]));
    assert_eq!(translate["writes"], "records");
    assert_eq!(translate["root"], "array");
    assert_eq!(translate["schema"], "mdast");
    assert_eq!(translate["lift"], "alchemy/lift.alc");
    assert_eq!(translate["render"], "alchemy/render.alc");
}

/// The host prints the loss lines verbatim, so each is a sentence.
#[test]
fn the_loss_is_a_list_of_sentences() {
    let translate = translate();
    let loss = translate["loss"]
        .as_array()
        .expect("translate.loss is a list");
    assert!(!loss.is_empty());
    for line in loss {
        let line = line.as_str().expect("each loss line is a string");
        assert!(
            line.starts_with(char::is_uppercase) && line.ends_with('.'),
            "{line:?} is not a sentence"
        );
    }
}

/// A host links both parts, with its own program and other formats'
/// parts, into one namespace, where a name defined twice is an error.
/// So every definition is named for Markdown, the entry points are
/// `markdown-lift` and `markdown-render`, no name is defined in both
/// files, and neither file defines an `export` of its own.
#[test]
fn the_parts_are_libraries_named_for_markdown() {
    let lift = names(tabnas_markdown::lift_text());
    let render = names(tabnas_markdown::render_text());
    assert!(lift.contains(&"markdown-lift"), "{lift:?}");
    assert!(render.contains(&"markdown-render"), "{render:?}");
    let mut seen = HashSet::new();
    for name in lift.iter().chain(render.iter()) {
        assert!(
            name.starts_with("markdown-"),
            "{name} is not named for Markdown"
        );
        assert!(
            seen.insert(*name),
            "{name} is defined twice across the parts"
        );
    }
}
