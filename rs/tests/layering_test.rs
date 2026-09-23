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
            // A SPACE in its place. Rust treats a comment as a token
            // separator, so `use tabnas/**/as/**/engine;` compiles --
            // and deleting the comment outright produced
            // `use tabnasasengine;`, which no scan below can see. A line
            // comment needs no separator: its terminating newline is
            // left in the stream and pushed as an ordinary character.
            out.push(' ');
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

/// The first offset at or after `from` where `word` begins at an
/// identifier boundary in `code`: the character before it, if there is
/// one, is not one an identifier continues with.
///
/// A plain substring search read `local_tabnas::Context` -- the path to
/// an engine-free module's own `mod local_tabnas` -- as the engine, and
/// reported a file that never touched it. That is the unactionable noise
/// this gate was written to replace. The `#` of a raw identifier is not an
/// identifier character, so `r#tabnas::` is still the engine.
fn find_word(code: &str, from: usize, word: &str) -> Option<usize> {
    let mut from = from;
    while let Some(rel) = code[from..].find(word) {
        let at = from + rel;
        let joined = code[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !joined {
            return Some(at);
        }
        from = at + word.len();
    }
    None
}

/// The identifier `s` starts with, and how many bytes it spans. A raw
/// identifier's `r#` is spanned but is not part of the name, because Rust
/// accepts the raw form of any ordinary name: `crate::r#engine_inline` IS
/// `crate::engine_inline`. Reading up to the first non-identifier
/// character saw only `r` there, and the driver behind it went unseen.
fn leading_ident(s: &str) -> (&str, usize) {
    let body = s.strip_prefix("r#").unwrap_or(s);
    let skip = s.len() - body.len();
    let end = body
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(body.len());
    (&body[..end], skip + end)
}

/// The engine items a file's CODE names: every `tabnas::Ident`, with a
/// `use tabnas::{A, B}` brace list expanded to its members.
fn engine_items(code: &str) -> BTreeSet<String> {
    let mut items = BTreeSet::new();
    let mut rest = code;
    // `rest` is always a suffix of `code`, so the search can look at the
    // character BEFORE a match, which a search of `rest` alone cannot.
    while let Some(at) = find_word(code, code.len() - rest.len(), "tabnas::") {
        let tail = &code[at + "tabnas::".len()..];
        if let Some(inner) = tail.strip_prefix('{') {
            let end = group_end(inner);
            for part in inner[..end].split(',') {
                // `X as Y` records X: the engine item, not the local name.
                let name = part.split_whitespace().next().unwrap_or("");
                let name = name.strip_prefix("r#").unwrap_or(name);
                if !name.is_empty() {
                    items.insert(name.to_string());
                }
            }
            rest = &tail[end.min(tail.len())..];
        } else if let Some(after) = tail.strip_prefix('*') {
            // `use tabnas::*;` imports EVERY engine item and names none,
            // so the identifier scan below found nothing after the path
            // and recorded nothing. A glob is the broadest engine
            // reference there is, and every later unqualified use of what
            // it brought in is invisible to this file.
            items.insert("*".to_string());
            rest = after;
        } else {
            let (name, span) = leading_ident(tail);
            if !name.is_empty() {
                items.insert(name.to_string());
            }
            rest = &tail[span..];
        }
    }
    items
}

/// Everything a sibling module can write after `crate::` that reaches
/// the engine, by the LOCAL name it writes. Read from `lib.rs` rather
/// than listed, so adding one widens the check instead of quietly
/// narrowing it.
///
/// TWO KINDS, and the second is easy to miss. The re-exported TYPES --
/// today `Tabnas` (from `pub use tabnas::Tabnas`) and `MarkdownError`
/// (from `pub use tabnas::TabnasError as MarkdownError`). And the
/// engine-facing FUNCTIONS: `crate::make()` hands back a built parser
/// without naming a single engine type at the call site, which is engine
/// behaviour reached from a module the rule calls engine-free.
///
/// A `pub fn` counts when its SIGNATURE names an engine type imported by
/// `lib.rs`. `Value` is excluded, for the same reason `VALUE_ONLY_FILES`
/// exists: the engine's value type is a type, not the engine, so
/// `to_html` and `parse_document` are ordinary crate API.
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

    // The engine types lib.rs imports, which is what makes a signature
    // engine-facing. Derived from its own `use tabnas::…`, minus the
    // value type.
    let mut markers: BTreeSet<String> = engine_items(&code);
    markers.remove("Value");
    markers.extend(names.iter().cloned());

    let mut entries = BTreeSet::new();
    // `match_indices`, not a byte range: `0..code.len()` visits offsets
    // INSIDE a multibyte character, and slicing there panics. A single
    // non-ASCII identifier in lib.rs -- `pub fn caf\u{e9}()` is a legal one --
    // would have crashed this gate before it could classify anything.
    for (at, _) in code.match_indices("pub fn ") {
        let tail = &code[at + "pub fn ".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // The signature is everything up to the body.
        let sig = &tail[..tail.find('{').unwrap_or(tail.len())];
        if markers.iter().any(|m| {
            sig.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == m)
        }) {
            entries.insert(name);
        }
    }
    assert!(
        entries.contains("make") && entries.contains("plugin"),
        "lib.rs no longer exposes make/plugin as engine-facing; \
         the entry-point half of this gate would check less than it says: {entries:?}"
    );
    names.extend(entries);
    names
}

/// The offset of the `}` closing the group `inner` opens, counting
/// nested groups. `find('}')` stopped at the FIRST one, so
/// `use crate::{engine_inline::{make_inline_tn}, node::Tree}` was read as
/// the single member `engine_inline::{make_inline_tn` -- which is not a
/// name any scan recognises, so a nested group imported the driver in
/// silence.
fn group_end(inner: &str) -> usize {
    let mut depth = 0usize;
    for (at, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if 0 == depth => return at,
            '}' => depth -= 1,
            _ => {}
        }
    }
    inner.len()
}

/// A brace group's members, split at TOP-LEVEL commas only, so
/// `engine_inline::{a, b}` stays one member.
fn top_level_parts(inner: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (at, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if 0 == depth => {
                out.push(&inner[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&inner[start..]);
    out
}

/// The members of a brace group, each cut back to its LEADING path
/// segment. `engine_inline::{a, b}` is one member and its leading segment
/// is `engine_inline`, which is the name the classification below is
/// about.
///
/// A member that is itself a bare group is read through: rustc accepts
/// `use crate::{{engine_inline}};`, and its leading segment is `{`, which
/// named nothing, so the driver inside was imported in silence.
fn group_members(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in top_level_parts(inner) {
        let part = part.trim();
        if let Some(nested) = part.strip_prefix('{') {
            out.extend(group_members(&nested[..group_end(nested)]));
            continue;
        }
        let head = part.split("::").next().unwrap_or("").trim();
        let (head, _) = leading_ident(head);
        if !head.is_empty() {
            out.push(head.to_string());
        }
    }
    out
}

/// A module pulled in by FILENAME, which is module inclusion the scans
/// cannot follow: `#[path = "engine_inline.rs"] mod borrowed_engine;`
/// puts a driver's whole body under a name in an engine-free module, and
/// every later `borrowed_engine::…` writes none of the roots this file
/// watches. The driver is still exempt under its own filename, so
/// nothing reports it.
///
/// Rejected rather than followed, like the aliases: an engine-free module
/// has no reason to include a file by path, and resolving one means
/// re-implementing module resolution. Read from the RAW source, because
/// the comment stripper drops string contents and the filename is a
/// string.
fn path_attribute_modules(raw: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (start, _) in raw.match_indices("#[") {
        found.extend(path_values(attribute_body(raw, start)));
    }
    found
}

/// One attribute's body: what sits between `#[` and its matching `]`,
/// counting nesting and ignoring brackets inside string literals, so a
/// `cfg_attr` predicate carrying either does not end the span early.
fn attribute_body(raw: &str, start: usize) -> &str {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (at, c) in raw[start + 1..].char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if 0 == depth {
                    return &raw[start + 2..start + 1 + at];
                }
            }
            _ => {}
        }
    }
    &raw[start + 2..]
}

/// The `path = "<file>"` items of one attribute body.
///
/// `#[path = "engine_inline.rs"]` and
/// `#[cfg_attr(all(), path = "engine_inline.rs")]` are the SAME include
/// with a predicate in front of the second, and rustfmt leaves both
/// alone, so a scan for the literal `#[path` sees only one of them.
///
/// An item named `path`, not the word: what precedes it is the start of
/// the attribute or an item separator, and what follows is `= "<file>"`.
/// That is what keeps `#[doc = "path = \"x\""]` out, where the word is
/// inside a string and is preceded by a quote.
fn path_values(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = body;
    while let Some(at) = rest.find("path") {
        let before = rest[..at].chars().rev().find(|c| !c.is_whitespace());
        let after = &rest[at + "path".len()..];
        rest = after;
        if !matches!(before, None | Some('(') | Some(',')) {
            continue;
        }
        let Some(tail) = after.trim_start().strip_prefix('=') else {
            continue;
        };
        let Some(tail) = tail.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(close) = tail.find('"') else {
            continue;
        };
        found.insert(format!("#[path = {:?}]", &tail[..close]));
    }
    found
}

/// The ways a file reaches the engine WITHOUT writing `tabnas::`, which
/// the literal scan above cannot see:
///
/// * `use tabnas as engine;` -- every later `engine::Context` is invisible.
/// * `use crate::Tabnas;` -- `lib.rs` re-exports engine types, so the
///   crate root is a second door into the same items.
/// * `crate::engine_inline::make_inline_tn(..)` -- the DRIVER MODULES are
///   allowed to name engine items, so calling into one reaches engine
///   behaviour without naming an engine item at all. Their whole purpose
///   is to be the boundary, which is exactly why crossing it from the
///   other side has to be a finding.
/// * `use crate as markdown;` -- an alias of the CRATE ROOT. Every later
///   `markdown::engine_inline::…` then contains none of `tabnas::`,
///   `crate::` or `super::`, so it reopens both of the doors above at
///   once. `self` and `super` alias the same way, and so does a
///   prefix-less group: `use {crate as markdown};`.
///
/// Both are REJECTED rather than resolved. Following an alias means
/// tracking the local name through the file, and following a re-export
/// means resolving paths; a gate that says "do not do this here" is
/// smaller than either and just as sound, because both forms are
/// unnecessary in an engine-free module by definition.
fn indirect_engine_refs(code: &str, reexports: &BTreeSet<String>) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    // `use tabnas as X;` / `use ::tabnas as X;`
    //
    // Every WHOLE-WORD `tabnas` in the statement, not the first substring:
    // `use local_tabnas as lt;` renames an engine-free module, and reading
    // only the first match would also let one such name hide a real alias
    // later in the same statement.
    for stmt in code.split(';') {
        let mut from = 0;
        while let Some(at) = find_word(stmt, from, "tabnas") {
            from = at + "tabnas".len();
            let tail = stmt[from..].trim_start();
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
    }

    // `use crate as X;` -- the crate root under another name, which is a
    // door to lib.rs's re-exports AND to every driver module, and which
    // no scan below can see. `self` and `super` are the same root.
    //
    // A token scan rather than a substring one: `use`, the root and `as`
    // may be separated by any whitespace including a newline, and a
    // preceding `pub` or `pub(crate)` must not hide the statement.
    let words: Vec<&str> = code.split_whitespace().collect();
    for w in words.windows(4) {
        if "use" != w[0] || "as" != w[2] || !["crate", "self", "super"].contains(&w[1]) {
            continue;
        }
        let alias: String = w[3]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !alias.is_empty() {
            refs.insert(format!("{} as {}", w[1], alias));
        }
    }

    // `extern crate self as markdown;` -- the same crate-root alias by a
    // spelling the `use` scan cannot see, and rustfmt-clean. After it,
    // `markdown::engine_inline::make_inline_tn(..)` reaches a driver
    // without containing any watched prefix.
    for w in words.windows(5) {
        if "extern" != w[0] || "crate" != w[1] || "self" != w[2] || "as" != w[3] {
            continue;
        }
        let alias: String = w[4]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !alias.is_empty() {
            refs.insert(format!("extern crate self as {alias}"));
        }
    }

    // `use {crate as markdown};` -- the same root alias from inside a
    // group with no prefix. rustc accepts it alone, nested as
    // `use {{crate as md}};`, or beside other members as
    // `use {node::Tree, crate as md};`. The word scan above reads
    // `{crate`, not `crate`, and the `crate::` scan below finds no
    // `crate::` at all. rustc refuses a bare `self` or `super` in that
    // position (E0431, E0432); they are matched with `crate` anyway, as
    // in the word scan, so the two scans stay one rule about the root.
    for stmt in code.split(';') {
        let mut from = 0;
        while let Some(at) = find_word(stmt, from, "use") {
            from = at + "use".len();
            let after = &stmt[from..];
            if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            if let Some(inner) = after.trim_start().strip_prefix('{') {
                grouped_root_aliases(inner, &mut refs);
            }
        }
    }

    // `crate::Name` / `super::Name`, for a name lib.rs re-exports from the
    // engine, OR for one of the engine-facing driver modules. A brace
    // list is expanded, so `use crate::{Tabnas, Tree}` reports only the
    // engine half.
    //
    // The driver modules are named without their `.rs`, since that is how
    // a path spells them: `crate::engine_inline::make_inline_tn(..)`.
    let drivers: BTreeSet<String> = ENGINE_FILES
        .iter()
        .filter(|f| **f != "lib.rs")
        .map(|f| f.trim_end_matches(".rs").to_string())
        .collect();
    let reachable = |name: &str| -> bool { reexports.contains(name) || drivers.contains(name) };
    for root in ["crate::", "super::"] {
        let mut rest = code;
        while let Some(at) = rest.find(root) {
            let tail = &rest[at + root.len()..];
            let consumed;
            if let Some(inner) = tail.strip_prefix('{') {
                let end = group_end(inner);
                for name in group_members(&inner[..end]) {
                    let name = name.as_str();
                    // `use crate::{self as markdown}` aliases the ROOT from
                    // inside a group. The word scan above cannot see it --
                    // it reads `crate::{self`, not `crate` -- and
                    // `reachable` takes `self` for an ordinary module name.
                    if "self" == name {
                        refs.insert(format!("{root}{{self}}"));
                        continue;
                    }
                    if reachable(name) {
                        refs.insert(format!("{root}{name}"));
                    }
                }
                consumed = end.min(tail.len());
            } else {
                // `crate::r#engine_inline` is `crate::engine_inline`, so the
                // name is read past a raw identifier's `r#`.
                let (name, span) = leading_ident(tail);
                // `super::super::engine_inline::…` is valid Rust. Consuming
                // the first `super` left the scan past the second `super::`,
                // so the driver behind it was never read. A root keyword
                // consumes nothing, and the next turn of this loop finds the
                // inner path. It still terminates: `rest` shrinks by the
                // root prefix every time.
                if matches!(name, "super" | "crate" | "self") {
                    consumed = 0;
                } else {
                    if !name.is_empty() && reachable(name) {
                        refs.insert(format!("{root}{name}"));
                    }
                    consumed = span;
                }
            }
            rest = &tail[consumed..];
        }
    }
    refs
}

/// The crate-root aliases in a prefix-less `use` group: every top-level
/// member of the form `crate as X` (or `self` / `super`, as above), and
/// the same inside a bare nested group.
fn grouped_root_aliases(inner: &str, refs: &mut BTreeSet<String>) {
    for part in top_level_parts(&inner[..group_end(inner)]) {
        let part = part.trim();
        if let Some(nested) = part.strip_prefix('{') {
            grouped_root_aliases(nested, refs);
            continue;
        }
        if let [root, "as", alias] = part.split_whitespace().collect::<Vec<_>>().as_slice() {
            if ["crate", "self", "super"].contains(root) {
                refs.insert(format!("use {{{root} as {alias}}}"));
            }
        }
    }
}

/// Nothing reachable from `commonmark.rs` names an engine item. The
/// phase modules, the renderer, the node tree, the character helpers and
/// the generated entity table are all engine-free code.
#[test]
fn only_the_drivers_use_the_engine() {
    let reexports = reexported_engine_names();
    let mut violations = Vec::new();
    for (name, path) in src_files() {
        let raw = fs::read_to_string(&path).expect("a readable source file");
        let code = normalise_paths(&strip_comments(&raw));
        let mut items = engine_items(&code);
        // An alias or a crate-relative re-export is an engine reference
        // too, and neither writes `tabnas::`. lib.rs is where both are
        // declared, so it is exempt from this half as it is from the rest.
        // lib.rs declares the re-exports and the drivers are the engine
        // boundary, so neither is measured against the indirect rule:
        // engine_block.rs calling engine_inline.rs is the layering
        // working, not a breach of it. Both are already exempt from the
        // direct scan below for the same reason.
        if !ENGINE_FILES.contains(&name.as_str()) {
            items.extend(indirect_engine_refs(&code, &reexports));
            // From the RAW source: the comment stripper drops string
            // contents, and the filename a `#[path]` attribute names is a
            // string. A driver included this way carries its whole body
            // into an engine-free module under a new name, and every
            // later call through that name writes none of the roots the
            // scans watch.
            items.extend(path_attribute_modules(&raw));
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
        // At least one item that is NOT `Value`. This file classifies
        // `Value` as a type rather than as engine behaviour -- that is
        // what VALUE_ONLY_FILES is for -- so a file that has lost its
        // engine calls while still naming `tabnas::Value` is stale here
        // and belongs there. Asking only that `items` is non-empty let
        // such a file keep an UNRESTRICTED ENGINE_FILES entry, so the
        // layering gate stopped checking it and said nothing.
        let behaviour: Vec<&String> = items
            .iter()
            .filter(|item| item.as_str() != VALUE_ITEM)
            .collect();
        assert!(
            !behaviour.is_empty(),
            "src/{want} is declared engine-facing but names {}; drop it from ENGINE_FILES \
             (a file naming only tabnas::{VALUE_ITEM} belongs in VALUE_ONLY_FILES)",
            if items.is_empty() {
                "no engine item".to_string()
            } else {
                format!("only tabnas::{VALUE_ITEM}")
            }
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

    // Reaching a DRIVER module is reaching the engine. Those modules are
    // allowed to name engine items, so calling into one from an
    // engine-free module gets engine behaviour without naming an engine
    // item at all -- their whole purpose is to be the boundary.
    for call in [
        "crate::engine_inline::make_inline_tn(opts)",
        "use crate::engine_block::something;",
        "super::engine_inline::other()",
        "use crate::{node, engine_inline};",
        // A raw identifier is the same name: the scan used to stop at the
        // `#` and read the module as `r`.
        "crate::r#engine_inline::make_inline_tn(opts)",
        "use crate::{node, r#engine_block::x};",
        // A bare group inside a group, which rustc accepts.
        "use crate::{{engine_inline}};",
    ] {
        let refs = indirect_engine_refs(call, &reexports);
        assert!(
            !refs.is_empty(),
            "{call:?} reached a driver module unnoticed: {refs:?}"
        );
    }

    // An alias of the CRATE ROOT reopens both doors at once, and writes
    // none of the three roots the scans look for.
    for aliased in [
        "use crate as markdown;",
        "pub use crate as md;",
        "use self as here;",
        "use\n  super\n  as\n  up;",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(aliased), &reexports);
        assert!(
            !refs.is_empty(),
            "{aliased:?} aliased the crate root unnoticed: {refs:?}"
        );
    }

    // But `use crate::thing as name;` is NOT a root alias -- it renames one
    // item, and whether it is a finding is the re-export question above.
    let item = indirect_engine_refs(&normalise_paths("use crate::node as n;"), &reexports);
    assert!(
        item.is_empty(),
        "an ordinary rename was read as a root alias: {item:?}"
    );

    // A grouped root alias: `use crate::{self as markdown};` is valid and
    // the word scan reads `crate::{self`, not `crate`.
    for grouped in [
        "use crate::{self as markdown};",
        "use super::{self as up};",
        "use crate::{node, self as md};",
        // And from a group with NO prefix, which the word scan reads as
        // `{crate` and the `crate::` scan never reaches.
        "use {crate as markdown};",
        "pub use {node::Tree, crate as md};",
        "use {{crate as md}};",
        "use{crate  as\n md};",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(grouped), &reexports);
        assert!(
            !refs.is_empty(),
            "{grouped:?} aliased the root from inside a group unnoticed: {refs:?}"
        );
    }

    // The lookalikes stay silent: a raw identifier naming an engine-free
    // module, a module whose name merely STARTS like a driver's, and a
    // prefix-less group that renames ordinary paths, one of them through
    // the crate root.
    for quiet in [
        "use crate::r#node::Tree;",
        "crate::engine_inline_notes::x()",
        "use {std::fmt as f, core as c};",
        "use {crate::node as crate_node};",
        "use crate::{{node::Tree}, html};",
        "fn reuse() {} let used = {1};",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(quiet), &reexports);
        assert!(refs.is_empty(), "{quiet:?} was flagged: {refs:?}");
    }

    // A REPEATED root: consuming the first `super` used to leave the scan
    // past the second, so the driver behind it was never read.
    for repeated in [
        "super::super::engine_inline::make_inline_tn(o)",
        "use super::super::super::engine_block::x;",
        "crate::super_helper(o)",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(repeated), &reexports);
        let want = !repeated.contains("super_helper");
        assert_eq!(
            !refs.is_empty(),
            want,
            "{repeated:?} gave {refs:?}, want {}",
            if want { "a finding" } else { "nothing" }
        );
    }

    // The crate root's engine-facing ENTRY POINTS. `crate::make()` hands
    // back a built parser while naming no engine type at the call site.
    for entry in [
        "let mut tn = crate::make();",
        "crate::plugin()",
        "crate::markdown(&mut tn, &opts)",
        "super::make_with(&opts)",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(entry), &reexports);
        assert!(
            !refs.is_empty(),
            "{entry:?} reached an engine entry point unnoticed: {refs:?}"
        );
    }

    // And the ones that are NOT engine-facing stay silent: their
    // signatures name only the engine's VALUE type, which VALUE_ONLY_FILES
    // already rules is a type rather than the engine.
    for ordinary in [
        "crate::to_html(src, &opts)",
        "crate::parse_document(src, &opts)",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(ordinary), &reexports);
        assert!(
            refs.is_empty(),
            "{ordinary:?} was read as an engine entry point: {refs:?}"
        );
    }

    // A NESTED group. `find('}')` stopped at the inner brace, so the
    // member read as `engine_inline::{make_inline_tn` -- not a name any
    // classification recognises -- and the driver was imported in
    // silence.
    for nested in [
        "use crate::{engine_inline::{make_inline_tn, parse_inlines_engine}, node::Tree};",
        "use crate::{node::Tree, engine_block::{x}};",
        "use super::{a::{b}, engine_inline::c};",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(nested), &reexports);
        assert!(
            !refs.is_empty(),
            "{nested:?} hid a driver inside a nested group: {refs:?}"
        );
    }

    // And a nested group of innocent modules stays silent, so this is not
    // a new source of false findings.
    let innocent_nested = indirect_engine_refs(
        &normalise_paths("use crate::{node::{Tree, Kind}, html::render};"),
        &reexports,
    );
    assert!(
        innocent_nested.is_empty(),
        "an engine-free nested group was flagged: {innocent_nested:?}"
    );

    // lib.rs is NOT in that set: it is the crate root, and reaching a
    // re-export through it is already covered by name above.
    let root_only = indirect_engine_refs("use crate::lib::nothing;", &reexports);
    assert!(
        root_only.is_empty(),
        "crate::lib was flagged: {root_only:?}"
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

    // A BLOCK COMMENT is a token separator too. Deleting one outright
    // welded the tokens either side together, so `use tabnas/**/as/**/
    // engine;` -- which compiles -- became `use tabnasasengine;` and no
    // scan could see it.
    for commented in [
        "use tabnas/**/as/**/engine;",
        "use tabnas/* why */as/* not */engine;",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(&strip_comments(commented)), &reexports);
        assert!(
            refs.contains("tabnas as engine"),
            "{commented:?} hid an alias: {refs:?}"
        );
    }
    let commented = "use tabnas/**/::/**/Context;";
    let items = engine_items(&normalise_paths(&strip_comments(commented)));
    assert!(
        items.contains("Context"),
        "{commented:?} hid an engine import: {items:?}"
    );

    // A `#[path]` attribute, read from RAW source: the stripper drops
    // string contents, so the filename is invisible to every other scan.
    let pathed = path_attribute_modules(
        "#[path = \"engine_inline.rs\"]\nmod borrowed_engine;\nfn f() { borrowed_engine::x(); }",
    );
    assert!(
        pathed.contains("#[path = \"engine_inline.rs\"]"),
        "a path-attributed module was not reported: {pathed:?}"
    );
    assert!(
        path_attribute_modules("mod node;\nmod html;").is_empty(),
        "an ordinary mod declaration was read as a path attribute"
    );

    // A GLOB names no item, so the identifier scan found nothing after the
    // path and reported nothing -- while importing every engine item there
    // is, and making every later unqualified use of one invisible.
    let glob = engine_items(&normalise_paths("use tabnas::*;"));
    assert!(
        glob.contains("*"),
        "a glob import was not read as one: {glob:?}"
    );
    let both = engine_items(&normalise_paths("use tabnas::*; use tabnas::Context;"));
    assert!(
        both.contains("*") && both.contains("Context"),
        "a glob swallowed the import after it: {both:?}"
    );

    // Normalising must not invent a path where none was written: an
    // ordinary identifier containing the crate name is left alone.
    let innocent = normalise_paths("let has_tabnas = 1; fn tabnas_name() {}");
    assert_eq!(innocent, "let has_tabnas = 1; fn tabnas_name() {}");
    assert!(
        engine_items(&innocent).is_empty(),
        "invented an engine item"
    );

    // A path whose first segment only ENDS in the crate name is not the
    // engine. `local_tabnas::Context` names an engine-free module's own
    // `mod local_tabnas`, and a substring search reported it.
    for quiet in [
        "mod local_tabnas { pub struct Context; } fn f() -> local_tabnas::Context { todo!() }",
        "use my_tabnas::{Context, Lexer};",
        "use local_tabnas as lt;",
    ] {
        let code = normalise_paths(&strip_comments(quiet));
        let items = engine_items(&code);
        let refs = indirect_engine_refs(&code, &reexports);
        assert!(
            items.is_empty() && refs.is_empty(),
            "{quiet:?} was read as the engine: {items:?} {refs:?}"
        );
    }

    // While the crate name after any other punctuation is still the
    // engine, and a lookalike earlier in a statement does not hide a real
    // alias later in it.
    for real in [
        "fn f() -> ::tabnas::Context { todo!() }",
        "let v: Vec<tabnas::Context> = vec![];",
        "fn f(_: &tabnas::Context) {}",
    ] {
        let items = engine_items(&normalise_paths(&strip_comments(real)));
        assert!(
            items.contains("Context"),
            "{real:?} hid an engine item: {items:?}"
        );
    }
    let both = indirect_engine_refs(
        &normalise_paths("use {local_tabnas as lt, tabnas as engine};"),
        &reexports,
    );
    assert_eq!(
        both.iter().collect::<Vec<_>>(),
        vec!["tabnas as engine"],
        "the lookalike and the real alias were not told apart"
    );
}

/// A `#[path]` attribute does not have to be spelled `#[path]`.
///
/// `#[cfg_attr(all(), path = "engine_inline.rs")]` is the same include
/// with a predicate in front of it, rustfmt leaves it alone, and a scan
/// for the literal `#[path` records nothing. Every later call through
/// the local module name is then an ordinary path with no watched prefix
/// in it, which is the shape this gate cannot recover from once the file
/// is in.
#[test]
fn a_path_attribute_is_seen_through_cfg_attr() {
    for spelling in [
        r#"#[path = "engine_inline.rs"] mod borrowed;"#,
        r#"#[cfg_attr(all(), path = "engine_inline.rs")] mod borrowed;"#,
        r#"#[cfg_attr(unix, path = "engine_inline.rs")] mod borrowed;"#,
        r#"#[cfg_attr(not(feature = "x"), path="engine_inline.rs")] mod borrowed;"#,
        "#[cfg_attr(\n    all(),\n    path = \"engine_inline.rs\"\n)]\nmod borrowed;",
    ] {
        let found = path_attribute_modules(spelling);
        assert!(
            found.contains(r#"#[path = "engine_inline.rs"]"#),
            "{spelling:?} hid a module pulled in by filename: {found:?}"
        );
    }

    // And the negatives, so this does not become a source of false
    // findings. Declaring a module is not naming a file, and the word
    // `path` inside a string is not an attribute item.
    for quiet in [
        "mod node;",
        "pub mod html;",
        r#"#[doc = "path = \"engine_inline.rs\""] pub fn f() {}"#,
        r#"let path = "engine_inline.rs";"#,
        r#"#[cfg(feature = "path")] mod node;"#,
    ] {
        let found = path_attribute_modules(quiet);
        assert!(found.is_empty(), "{quiet:?} was flagged: {found:?}");
    }
}

/// The crate root can be aliased without a `use` statement at all.
///
/// `extern crate self as markdown;` is accepted in an edition-2018 crate
/// and is rustfmt-clean, and after it `markdown::engine_inline::…`
/// reaches a driver module by a name the `use … as …` token scan never
/// sees.
#[test]
fn extern_crate_self_is_a_crate_root_alias() {
    let reexports = reexported_engine_names();

    for spelling in [
        "extern crate self as markdown;",
        "pub extern crate self as markdown;",
        "extern  crate\n    self  as  markdown ;",
    ] {
        let refs = indirect_engine_refs(&normalise_paths(&strip_comments(spelling)), &reexports);
        assert!(
            refs.contains("extern crate self as markdown"),
            "{spelling:?} hid a crate-root alias: {refs:?}"
        );
    }

    // An ordinary extern crate of ANOTHER crate is not a root alias.
    let other = indirect_engine_refs(
        &normalise_paths(&strip_comments("extern crate serde_json as sj;")),
        &reexports,
    );
    assert!(
        !other.iter().any(|r| r.starts_with("extern crate self")),
        "an unrelated extern crate was read as a root alias: {other:?}"
    );
}
