// Named character references (§6.2), checked against the canonical table.
//
// `src/entities.rs` is generated from ts/src/entities.ts, the canonical
// list (2125 semicolon-terminated entries, generated from the WHATWG
// entities.json). This reads the TypeScript table and asserts the two
// agree on every entry, and that no near-miss decodes at all: the exact
// rule §6.2 states. Mirrors go/entities_test.go, which pins the same
// thing against Go's standard-library table.

mod common;

use std::collections::BTreeMap;

use tabnas_markdown::common::decode_entity;
use tabnas_markdown::{to_html, Options};

/// Undo the JavaScript double-quoted string literal the table is embedded
/// in. The generator emits only `\"` and `\\`; anything else is kept as
/// it is, which the JSON parse below would then reject loudly.
fn unquote_js(literal: &str) -> String {
    let inner = literal
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .expect("a double-quoted literal");
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The generated TypeScript table: the one source of truth for which
/// names §6.2 admits and what they decode to.
fn canonical_entities() -> BTreeMap<String, String> {
    let path = common::repo_root()
        .join("ts")
        .join("src")
        .join("entities.ts");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    let open = src
        .find("JSON.parse(")
        .expect("entities.ts: no JSON.parse( ; has the generator changed shape?");
    let start = open
        + src[open..]
            .find('"')
            .expect("entities.ts: no string literal after JSON.parse(");
    let end = src
        .rfind('"')
        .expect("entities.ts: unterminated string literal");
    assert!(end > start, "entities.ts: unterminated string literal");

    let inner = unquote_js(&src[start..=end]);
    let table: BTreeMap<String, String> =
        serde_json::from_str(&inner).expect("entities.ts: the table is JSON");
    assert_eq!(table.len(), 2125, "entities.ts: expected 2125 names");
    table
}

/// Every canonical name decodes to the canonical value, and no near-miss
/// decodes at all.
#[test]
fn entity_table_matches_typescript() {
    let table = canonical_entities();
    let mut failures = Vec::new();

    for (name, want) in &table {
        let reference = format!("&{name};");
        let got = decode_entity(&reference);
        if &got != want {
            failures.push(format!(
                "decode_entity({reference:?}) = {got:?}, want {want:?}"
            ));
        }

        // One more name character makes it a different name. Some of
        // those are themselves real names (`sup` -> `sup1`, `le` ->
        // `leq`), so the expectation is "decodes iff the table has it":
        // the exact rule §6.2 states.
        for extra in ["q", "Z", "1"] {
            let longer = format!("{name}{extra}");
            if longer.len() > 32 {
                continue; // beyond the entity pattern's name limit
            }
            let near = format!("&{longer};");
            let want = table.get(&longer).cloned().unwrap_or_else(|| near.clone());
            let got = decode_entity(&near);
            if got != want {
                failures.push(format!("decode_entity({near:?}) = {got:?}, want {want:?}"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} disagreements:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The same contract one level up, through the public renderer, for the
/// shapes that regressed in another port: a legacy semicolon-less alias
/// must never match as a prefix of a longer name.
#[test]
fn entity_legacy_alias_not_matched_as_prefix() {
    for (input, want) in [
        ("&ampa;\n", "<p>&amp;ampa;</p>\n"),
        ("&ampA;\n", "<p>&amp;ampA;</p>\n"),
        ("&nbspa;\n", "<p>&amp;nbspa;</p>\n"),
        ("&ltx;\n", "<p>&amp;ltx;</p>\n"),
        ("&gtx;\n", "<p>&amp;gtx;</p>\n"),
        ("&quotx;\n", "<p>&amp;quotx;</p>\n"),
        ("&copyx;\n", "<p>&amp;copyx;</p>\n"),
        ("&AMPa;\n", "<p>&amp;AMPa;</p>\n"),
        // The genuine references still resolve.
        ("&amp;\n", "<p>&amp;</p>\n"),
        ("&nbsp;\n", "<p>\u{a0}</p>\n"),
        ("&semi;\n", "<p>;</p>\n"),
        ("&nGt;\n", "<p>≫⃒</p>\n"),
        ("&nLt;\n", "<p>≪⃒</p>\n"),
        // A name with no semicolon is not a reference either.
        ("&amp\n", "<p>&amp;amp</p>\n"),
    ] {
        assert_eq!(
            to_html(input, &Options::default()),
            want,
            "to_html({input:?})"
        );
    }
}

/// The generated table is sorted, which is what the binary search in
/// `entities.rs` relies on, and carries exactly the canonical count.
#[test]
fn generated_table_is_sorted_and_complete() {
    let table = canonical_entities();
    for (name, want) in &table {
        assert_eq!(&decode_entity(&format!("&{name};")), want);
    }
    // A handful of names at the edges of the sort order.
    assert_eq!(decode_entity("&AElig;"), "Æ");
    assert_eq!(decode_entity("&zwnj;"), "\u{200c}");
    assert_eq!(decode_entity("&zwj;"), "\u{200d}");
}
