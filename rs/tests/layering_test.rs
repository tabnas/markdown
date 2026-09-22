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

/// Every `.rs` file directly under `src/`, by file name.
fn src_files() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(src_dir())
        .expect("src/ is readable")
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            (name, path)
        })
        .collect();
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
/// This is deliberately not a Rust lexer. It does not know string
/// literals, so a `"tabnas::Value"` inside one would be read as code.
/// That errs towards reporting, which is the right direction for a gate,
/// and no file here has such a literal.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        if depth > 0 {
            if bytes[i..].starts_with(b"*/") {
                depth -= 1;
                i += 2;
            } else if bytes[i..].starts_with(b"/*") {
                depth += 1;
                i += 2;
            } else {
                // Keep newlines so line structure survives.
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
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

/// Nothing reachable from `commonmark.rs` names an engine item. The
/// phase modules, the renderer, the node tree, the character helpers and
/// the generated entity table are all engine-free code.
#[test]
fn only_the_drivers_use_the_engine() {
    let mut violations = Vec::new();
    for (name, path) in src_files() {
        let code = strip_comments(&fs::read_to_string(&path).expect("a readable source file"));
        let items = engine_items(&code);

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
        let code = strip_comments(&fs::read_to_string(path).expect("a readable source file"));
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
        let code = strip_comments(&fs::read_to_string(path).expect("a readable source file"));
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
