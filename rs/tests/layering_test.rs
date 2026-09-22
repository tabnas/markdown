// The layering rule, made executable.
//
// `../AGENTS.md` and `../rs/AGENTS.md` state it in prose: only `lib.rs`,
// `engine_block.rs` and `engine_inline.rs` use engine BEHAVIOUR, and
// `ast.rs` plus `options.rs` name `tabnas::Value` alone, because the AST
// and the plugin option bag ARE engine values. Nothing reachable from
// `commonmark.rs` may touch the engine at all, which is what keeps the
// two HTML corpora gradable by the engine-free functions and the
// conformance suite runnable with no engine installed.
//
// It was checked by eye, with a grep that rs/AGENTS.md printed as the
// command to run. That grep reads comments as well as code, so it
// already named four files rather than the three the rule allows:
// `options.rs` matched on a doc comment referring to
// `Tabnas::use_plugin`, and `block.rs` and `inline.rs` match too, on doc
// comments explaining why the nesting caps exist. A check that reports a
// violation for prose is a check nobody can act on, and the real
// regression it exists to catch (an engine import landing in the block
// or inline phase) looks exactly like the noise.
//
// So the rule is asserted here against CODE, with comments stripped, and
// the file classification below is the whole of it. A new file in
// `src/` needs no entry: unclassified means engine-free, which is the
// safe default and the answer for every phase module.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The engine-facing drivers and the plugin surface. These may name any
/// engine item.
const ENGINE_FILES: [&str; 3] = ["lib.rs", "engine_block.rs", "engine_inline.rs"];

/// The two modules that name the engine's VALUE TYPE and nothing else.
/// `ast.rs` projects into a `Value`, and `options.rs` resolves a plugin
/// option bag, which is one. That is a type, not the engine.
const VALUE_ONLY_FILES: [&str; 2] = ["ast.rs", "options.rs"];

/// The only engine item the value-only modules may name.
const VALUE_ITEM: &str = "Value";

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `src/`, RECURSIVELY, keyed by its path relative
/// to `src/` with `/` separators.
///
/// The walk is recursive and the key is the relative path, because both
/// classifications below name root files. A module in the standard nested
/// layout -- `src/foo/mod.rs`, `src/foo/bar.rs` -- is as reachable as any
/// other, and a flat `read_dir` never saw it: an engine import there left
/// this gate green. There is no such directory today, which is exactly
/// when to fix it, rather than when someone adds one.
fn src_files() -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
        let entries = fs::read_dir(dir).expect("a readable directory under src/");
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            let key = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                walk(&path, &key, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push((key, path));
            }
        }
    }
    let mut out = Vec::new();
    walk(&src_dir(), "", &mut out);
    out.sort();
    assert!(
        !out.is_empty(),
        "src/ holds no .rs files; the layering gate would pass having checked nothing"
    );
    out
}

/// Strip `/* */` blocks and `//` line comments, so the scan below reads
/// code alone. Rust doc comments (`///`, `//!`) start with `//` and go
/// with them, which is the point: the prose that explains why a cap
/// exists is not an engine reference.
///
/// String, byte-string, raw-string and character literals are recognised
/// and their CONTENTS dropped, for two reasons that pull the same way. A
/// literal cannot import anything, so its text is data and not code. And
/// a stripper that does not know them produces false NEGATIVES, not only
/// the false positives an earlier comment here claimed were the whole
/// risk: a `"/*"` inside a literal opens a block comment that never
/// closes, and every `use tabnas::…` after it disappears from the scan
/// while this gate stays green.
///
/// It walks `char`s rather than bytes. The byte walk it replaces mapped
/// each byte to its own `char`, so a multi-byte character -- six files
/// under `src/` carry them -- could put a later slice off a character
/// boundary and panic.
///
/// A `'` is a lifetime unless a closing `'` follows within one character
/// or one escape, which is what distinguishes `&'a str` from `'\n'`.
fn strip_comments(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let mut depth = 0usize;
    let at = |i: usize| -> Option<char> { c.get(i).copied() };
    let starts = |i: usize, a: char, b: char| at(i) == Some(a) && at(i + 1) == Some(b);

    while i < c.len() {
        if depth > 0 {
            if starts(i, '*', '/') {
                depth -= 1;
                i += 2;
            } else if starts(i, '/', '*') {
                depth += 1;
                i += 2;
            } else {
                // Keep newlines so line structure survives.
                if c[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if starts(i, '/', '*') {
            depth += 1;
            i += 2;
            continue;
        }
        if starts(i, '/', '/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // A raw string: r"…", r#"…"#, br##"…"##. The hash count closes it.
        if let Some(open) = raw_string_open(&c, i) {
            i = skip_raw_string(&c, open.0, open.1);
            continue;
        }
        // An ordinary string or byte string.
        if c[i] == '"' || (c[i] == 'b' && at(i + 1) == Some('"')) {
            i = skip_quoted(&c, if c[i] == '"' { i } else { i + 1 }, '"');
            continue;
        }
        // A character literal, but not a lifetime.
        if c[i] == '\'' && is_char_literal(&c, i) {
            i = skip_quoted(&c, i, '\'');
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// `Some((quote_index, hash_count))` when a raw string starts at `i`.
fn raw_string_open(c: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if c.get(j) == Some(&'b') {
        j += 1;
    }
    if c.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = {
        let start = j;
        while c.get(j) == Some(&'#') {
            j += 1;
        }
        j - start
    };
    if c.get(j) == Some(&'"') {
        Some((j, hashes))
    } else {
        None
    }
}

/// Past the closing `"` + `hashes` of a raw string opened at `quote`.
fn skip_raw_string(c: &[char], quote: usize, hashes: usize) -> usize {
    let mut i = quote + 1;
    while i < c.len() {
        if c[i] == '"' {
            let closed = (1..=hashes).all(|k| c.get(i + k) == Some(&'#'));
            if closed {
                return i + 1 + hashes;
            }
        }
        i += 1;
    }
    c.len()
}

/// Past the closing `end` of a `\`-escaped literal opened at `i`.
fn skip_quoted(c: &[char], i: usize, end: char) -> usize {
    let mut j = i + 1;
    while j < c.len() {
        if c[j] == '\\' {
            j += 2;
            continue;
        }
        if c[j] == end {
            return j + 1;
        }
        j += 1;
    }
    c.len()
}

/// A `'` at `i` opens a character literal rather than a lifetime.
fn is_char_literal(c: &[char], i: usize) -> bool {
    match c.get(i + 1) {
        Some('\\') => true,                     // '\n', '\'', '\u{1F600}'
        Some(_) => c.get(i + 2) == Some(&'\''), // 'x'
        None => false,
    }
}

/// Collapse the whitespace Rust allows around `::` and around the `as`
/// of a rename, so the searches below can stay exact-string.
///
/// `use tabnas :: Context;` compiles, and so does a path broken across
/// lines. Every scan here looks for `tabnas::`, `crate::` or `super::`,
/// and all three missed those spellings: an engine import written that
/// way sat in an engine-free module with this gate green.
///
/// Normalising is the smaller answer than tokenising, because the thing
/// being read is a path prefix rather than a program. It runs on code
/// with the comments and literals already gone, so it cannot reach into
/// either.
fn normalise_paths(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let c: Vec<char> = code.chars().collect();
    let mut i = 0usize;
    while i < c.len() {
        // `  ::  `  ->  `::`
        if c[i].is_whitespace() {
            let mut j = i;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if c.get(j) == Some(&':') && c.get(j + 1) == Some(&':') {
                out.push_str("::");
                i = j + 2;
                while i < c.len() && c[i].is_whitespace() {
                    i += 1;
                }
                continue;
            }
            // `  as  ` -> ` as ` (any run of whitespace, newlines included)
            if c.get(j) == Some(&'a')
                && c.get(j + 1) == Some(&'s')
                && c.get(j + 2).is_some_and(|ch| ch.is_whitespace())
            {
                out.push_str(" as ");
                i = j + 3;
                while i < c.len() && c[i].is_whitespace() {
                    i += 1;
                }
                continue;
            }
            out.push(' ');
            i = j;
            continue;
        }
        // `::  ` -> `::`
        if c[i] == ':' && c.get(i + 1) == Some(&':') {
            out.push_str("::");
            i += 2;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// The engine items a file's CODE names: every `tabnas::Ident`, with a
/// `use tabnas::{A, B}` brace list expanded to its members.
fn engine_items(code: &str) -> BTreeSet<String> {
    let mut items = BTreeSet::new();
    let mut rest = code;
    while let Some(at) = rest.find("tabnas::") {
        let tail = &rest[at + "tabnas::".len()..];
        if let Some(inner) = tail.strip_prefix('{') {
            let end = inner.find('}').unwrap_or(inner.len());
            for part in inner[..end].split(',') {
                // `X as Y` records X: the engine item, not the local name.
                let name = part.split_whitespace().next().unwrap_or("");
                if !name.is_empty() {
                    items.insert(name.to_string());
                }
            }
            rest = &tail[end.min(tail.len())..];
        } else {
            let end = tail
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(tail.len());
            if end > 0 {
                items.insert(tail[..end].to_string());
            }
            rest = &tail[end..];
        }
    }
    items
}

/// The engine names `lib.rs` re-exports, by the LOCAL name a sibling
/// module writes after `crate::`. Read from `lib.rs` rather than listed,
/// so adding a re-export widens the check instead of quietly narrowing it.
///
/// Today that is `Tabnas` (from `pub use tabnas::Tabnas`) and
/// `MarkdownError` (from `pub use tabnas::TabnasError as MarkdownError`).
fn reexported_engine_names() -> BTreeSet<String> {
    let lib = src_dir().join("lib.rs");
    let code = normalise_paths(&strip_comments(
        &fs::read_to_string(&lib).expect("src/lib.rs is readable"),
    ));
    let mut names = BTreeSet::new();
    for stmt in code.split(';') {
        let Some(at) = stmt.find("pub use tabnas::") else {
            continue;
        };
        let tail = &stmt[at + "pub use tabnas::".len()..];
        let parts: Vec<&str> = if let Some(inner) = tail.trim_start().strip_prefix('{') {
            let end = inner.find('}').unwrap_or(inner.len());
            inner[..end].split(',').collect()
        } else {
            vec![tail]
        };
        for part in parts {
            // `X as Y` re-exports as Y: the LOCAL name is what a sibling
            // writes, which is the opposite of engine_items above.
            let words: Vec<&str> = part.split_whitespace().collect();
            let local = match words.as_slice() {
                [_, "as", y, ..] => *y,
                [x, ..] => *x,
                [] => continue,
            };
            let local: String = local
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !local.is_empty() {
                names.insert(local);
            }
        }
    }
    assert!(
        !names.is_empty(),
        "src/lib.rs re-exports no engine name; the crate-relative half of this gate would check nothing"
    );
    names
}

/// The ways a file reaches the engine WITHOUT writing `tabnas::`, which
/// the literal scan above cannot see:
///
/// * `use tabnas as engine;` -- every later `engine::Context` is invisible.
/// * `use crate::Tabnas;` -- `lib.rs` re-exports engine types, so the
///   crate root is a second door into the same items.
///
/// Both are REJECTED rather than resolved. Following an alias means
/// tracking the local name through the file, and following a re-export
/// means resolving paths; a gate that says "do not do this here" is
/// smaller than either and just as sound, because both forms are
/// unnecessary in an engine-free module by definition.
fn indirect_engine_refs(code: &str, reexports: &BTreeSet<String>) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    // `use tabnas as X;` / `use ::tabnas as X;`
    for stmt in code.split(';') {
        let Some(at) = stmt.find("tabnas") else {
            continue;
        };
        let tail = stmt[at + "tabnas".len()..].trim_start();
        if let Some(rest) = tail
            .strip_prefix("as ")
            .or_else(|| tail.strip_prefix("as\t"))
        {
            let alias: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !alias.is_empty() {
                refs.insert(format!("tabnas as {alias}"));
            }
        }
    }

    // `crate::Name` / `super::Name`, for a name lib.rs re-exports from the
    // engine. A brace list is expanded, so `use crate::{Tabnas, Tree}`
    // reports only the engine half.
    for root in ["crate::", "super::"] {
        let mut rest = code;
        while let Some(at) = rest.find(root) {
            let tail = &rest[at + root.len()..];
            let consumed;
            if let Some(inner) = tail.strip_prefix('{') {
                let end = inner.find('}').unwrap_or(inner.len());
                for part in inner[..end].split(',') {
                    let name = part.split_whitespace().next().unwrap_or("");
                    if reexports.contains(name) {
                        refs.insert(format!("{root}{name}"));
                    }
                }
                consumed = end.min(tail.len());
            } else {
                let end = tail
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(tail.len());
                if end > 0 && reexports.contains(&tail[..end]) {
                    refs.insert(format!("{root}{}", &tail[..end]));
                }
                consumed = end;
            }
            rest = &tail[consumed..];
        }
    }
    refs
}

/// Nothing reachable from `commonmark.rs` names an engine item. The
/// phase modules, the renderer, the node tree, the character helpers and
/// the generated entity table are all engine-free code.
#[test]
fn only_the_drivers_use_the_engine() {
    let reexports = reexported_engine_names();
    let mut violations = Vec::new();
    for (name, path) in src_files() {
        let code = normalise_paths(&strip_comments(
            &fs::read_to_string(&path).expect("a readable source file"),
        ));
        let mut items = engine_items(&code);
        // An alias or a crate-relative re-export is an engine reference
        // too, and neither writes `tabnas::`. lib.rs is where both are
        // declared, so it is exempt from this half as it is from the rest.
        if name != "lib.rs" {
            items.extend(indirect_engine_refs(&code, &reexports));
        }

        if ENGINE_FILES.contains(&name.as_str()) {
            continue;
        }
        if VALUE_ONLY_FILES.contains(&name.as_str()) {
            let extra: Vec<&String> = items.iter().filter(|i| i.as_str() != VALUE_ITEM).collect();
            if !extra.is_empty() {
                violations.push(format!(
                    "src/{name} may name only tabnas::{VALUE_ITEM}, but also names {extra:?}"
                ));
            }
            continue;
        }
        if !items.is_empty() {
            violations.push(format!(
                "src/{name} is engine-free, but names {:?}",
                items.iter().collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "the layering rule is broken:\n  {}",
        violations.join("\n  ")
    );
}

/// The classification stays honest: a file listed as engine-facing that
/// no longer touches the engine is a stale entry, and the list is what
/// the test above trusts.
#[test]
fn the_declared_engine_files_really_use_the_engine() {
    let files = src_files();
    for want in ENGINE_FILES {
        let (_, path) = files
            .iter()
            .find(|(name, _)| name == want)
            .unwrap_or_else(|| panic!("src/{want} is declared engine-facing but is not on disk"));
        let code = normalise_paths(&strip_comments(
            &fs::read_to_string(path).expect("a readable source file"),
        ));
        let items = engine_items(&code);
        assert!(
            !items.is_empty(),
            "src/{want} is declared engine-facing but names no engine item; drop it from ENGINE_FILES"
        );
    }
    for want in VALUE_ONLY_FILES {
        let (_, path) = files
            .iter()
            .find(|(name, _)| name == want)
            .unwrap_or_else(|| panic!("src/{want} is declared value-only but is not on disk"));
        let code = normalise_paths(&strip_comments(
            &fs::read_to_string(path).expect("a readable source file"),
        ));
        assert!(
            engine_items(&code).contains(VALUE_ITEM),
            "src/{want} is declared value-only but does not name tabnas::{VALUE_ITEM}; drop it from VALUE_ONLY_FILES"
        );
    }
}

/// The stripper is the part that could silently turn this gate off: if
/// it ate code as well as comments, every file would look engine-free
/// and the suite would pass having checked nothing.
#[test]
fn the_comment_stripper_keeps_code_and_drops_prose() {
    let src = "\
/* tabnas::Header */
use tabnas::{Context, Value};
/// tabnas::DocComment
//! tabnas::InnerDoc
// tabnas::LineComment
let v: tabnas::Tin = tabnas::Value::Bool(true);
";
    let code = strip_comments(src);
    assert!(!code.contains("Header"), "block comment survived");
    assert!(!code.contains("DocComment"), "doc comment survived");
    assert!(!code.contains("InnerDoc"), "inner doc comment survived");
    assert!(!code.contains("LineComment"), "line comment survived");

    let items = engine_items(&code);
    let want: BTreeSet<String> = ["Context", "Value", "Tin"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(items, want, "the scan lost or invented an engine item");
}

/// A literal is data, and the failure it used to cause was a FALSE
/// NEGATIVE: an unclosed `/*` inside a string swallowed the rest of the
/// file, so a real import after it left the gate green.
#[test]
fn the_comment_stripper_knows_literals() {
    // The first line is the trap: a block-comment opener inside a string.
    let src = r###"
let trap = "/* not a comment";
let name = "tabnas::NotCode";
let raw = r#"/* also not */ tabnas::AlsoNotCode"#;
let byte = b"/* nor this */";
let quote = '"';
let tick = '''; let esc = '
';
let lt: &'a str = "x";
use tabnas::Context;
"###;
    let code = strip_comments(src);
    assert!(
        code.contains("use tabnas::Context"),
        "a literal swallowed the code after it: {code:?}"
    );
    let items = engine_items(&code);
    assert!(
        !items.contains("NotCode") && !items.contains("AlsoNotCode"),
        "a string literal was read as code: {items:?}"
    );
    assert!(items.contains("Context"), "the real import was lost");
    // `&'a str` is a lifetime, not an unterminated character literal.
    assert!(code.contains("&'a str"), "a lifetime was eaten: {code:?}");
}

/// The two indirect doors, each rejected by name.
#[test]
fn an_alias_or_a_crate_reexport_counts_as_an_engine_reference() {
    let reexports = reexported_engine_names();
    assert!(
        reexports.contains("Tabnas"),
        "lib.rs no longer re-exports Tabnas; the fixtures below need rewriting: {reexports:?}"
    );

    let aliased =
        indirect_engine_refs("use tabnas as engine;\nengine::Context::new();", &reexports);
    assert!(
        aliased.contains("tabnas as engine"),
        "an aliased engine import went unseen: {aliased:?}"
    );

    let relative = indirect_engine_refs("use crate::Tabnas;", &reexports);
    assert!(
        relative.contains("crate::Tabnas"),
        "a crate-relative engine re-export went unseen: {relative:?}"
    );

    let braced = indirect_engine_refs("use crate::{Tree, Tabnas};", &reexports);
    assert_eq!(
        braced.iter().collect::<Vec<_>>(),
        vec!["crate::Tabnas"],
        "the brace list reported the wrong half"
    );

    // An engine-free crate-relative import is not a finding.
    let innocent = indirect_engine_refs("use crate::node::Tree;", &reexports);
    assert!(
        innocent.is_empty(),
        "a non-engine import was flagged: {innocent:?}"
    );
}

/// Rust allows whitespace around `::` and around the `as` of a rename,
/// and every scan in this file is an exact-string search for a path
/// prefix. `use tabnas :: Context;` compiles and named no engine item;
/// `use tabnas  as  engine;` was not read as an alias either. Both sat in
/// an engine-free module with this gate green.
#[test]
fn whitespace_in_a_path_is_not_a_way_past_the_gate() {
    let reexports = reexported_engine_names();

    for spaced in [
        "use tabnas :: Context;",
        "use tabnas::  Context;",
        "use  tabnas  ::  Context ;",
        "use tabnas\n    ::Context;",
    ] {
        let items = engine_items(&normalise_paths(&strip_comments(spaced)));
        assert!(
            items.contains("Context"),
            "{spaced:?} hid an engine import: {items:?}"
        );
    }

    for spaced in ["use tabnas  as  engine;", "use tabnas\n    as engine;"] {
        let refs = indirect_engine_refs(&normalise_paths(&strip_comments(spaced)), &reexports);
        assert!(
            refs.contains("tabnas as engine"),
            "{spaced:?} hid an alias: {refs:?}"
        );
    }

    let spaced = "use crate :: Tabnas;";
    let refs = indirect_engine_refs(&normalise_paths(&strip_comments(spaced)), &reexports);
    assert!(
        refs.contains("crate::Tabnas"),
        "{spaced:?} hid a crate-relative re-export: {refs:?}"
    );

    // Normalising must not invent a path where none was written: an
    // ordinary identifier containing the crate name is left alone.
    let innocent = normalise_paths("let has_tabnas = 1; fn tabnas_name() {}");
    assert_eq!(innocent, "let has_tabnas = 1; fn tabnas_name() {}");
    assert!(
        engine_items(&innocent).is_empty(),
        "invented an engine item"
    );
}
