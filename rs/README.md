# tabnas-markdown (Rust)

A CommonMark parser for the [`tabnas`](https://github.com/tabnas/parser)
parsing engine, crate `tabnas_markdown`.

**This parser is conformant to CommonMark 0.31.2**: all 652 examples,
across all 26 sections of the spec suite. The suite is vendored in this
repository, so the claim is checkable rather than asserted:

```bash
cd rs && cargo test --test commonmark_test -- --nocapture   # 652/652
```

That run has the GFM extensions off, which is what measuring CommonMark
conformance means. On top of CommonMark the crate implements **all five
GFM extensions** (tables, task list items, autolink literals,
strikethrough and disallowed raw HTML), gated together on the single
`gfm` option, which is on by default. They score 24/24 on the vendored
GFM corpus:

```bash
cd rs && cargo test --test gfm_test gfm_spec -- --nocapture   # 24/24
```

This is the Rust port of the canonical TypeScript implementation in
[`../ts`](../ts); the TypeScript version is authoritative and this crate
tracks it, as the Go port in [`../go`](../go) does. All three runtimes
run the same shared AST fixtures, the same two HTML corpora, and the same
native-tree goldens, so they are held to one standard rather than to
each other.

The parser itself is engine-free: the block phase, the inline phase and
the renderer never touch the engine, and the AST projection needs only
its value type. The plugin in `src/lib.rs` is wiring around them, and
the engine path and the direct path are asserted to agree on every
example.

## Use

Three outputs, and the AST is the primary one. `parse_document` returns
it as a `tabnas::Value` and runs no renderer:

```rust
use tabnas_markdown::{parse_document, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse_document("# Hello\n\nHello *world*", &Options::default());
    let json = doc.to_json();
    assert_eq!(json["type"], "document");
    assert_eq!(json["children"][0]["type"], "heading");
    assert_eq!(json["children"][1]["children"][1]["type"], "emphasis");
    Ok(())
}
```

`to_json()` hands back a `serde_json::Value`, the convenient shape for
reading the tree. Numbers in the AST are `f64`, so a heading's `depth`
reads as `1.0` there; the section on differences below has the detail.

HTML is opt-in, through `to_html`. The CommonMark suite scores HTML
output, so the renderer is what makes the 652/652 claim measurable; that
it is useful to callers is a consequence:

```rust
use tabnas_markdown::{to_html, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html = to_html("# Hello\n\nHello *world*", &Options::default());
    assert_eq!(html, "<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n");
    Ok(())
}
```

**The HTML is not sanitised.** Raw HTML blocks and inline tags pass
through verbatim, as CommonMark specifies. Put a sanitiser downstream of
any untrusted Markdown. GFM's disallowed-raw-HTML filter, on under the
default `gfm: true`, escapes the leading `<` of nine tag names (`title`,
`textarea`, `style`, `xmp`, `iframe`, `noembed`, `noframes`, `script`,
`plaintext`) and touches nothing else. It is not a sanitiser: it does
nothing about attributes or `javascript:` destinations.

```rust
use tabnas_markdown::{to_html, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Options::default();
    assert_eq!(
        to_html("<img onerror=\"alert(1)\">", &opts),
        "<img onerror=\"alert(1)\">\n"
    );
    assert_eq!(
        to_html("<script>alert(1)</script>", &opts),
        "&lt;script>alert(1)&lt;/script>\n"
    );
    Ok(())
}
```

The third output is the native CommonMark node tree, for callers who
want to walk or mutate before rendering. `parse_tree` returns it, with
`sourcepos` kept on block nodes, and `render_html` renders a tree back
to HTML, so parse, transform and render are three separate steps when
they need to be:

```rust
use tabnas_markdown::{parse_tree, render_html, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Options::default();
    let tree = parse_tree("# Hello\n\nHello *world*", &opts);
    assert_eq!(tree.children(tree.root()).len(), 2);
    assert_eq!(
        render_html(&tree, Some(&opts)),
        "<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n"
    );
    Ok(())
}
```

### On an engine instance

The plugin path is what `tabnas` callers use, and the engine parse is
the parse: a custom matcher lexes one token per physical line and the
rules feed the same block algorithm. `make` builds an instance with the
plugin installed under the defaults, and its `parse` returns the same
AST:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = tabnas_markdown::make();
    let doc = parser.parse("- [x] done")?;
    assert_eq!(doc.to_json()["children"][0]["children"][0]["checked"], true);
    Ok(())
}
```

Reuse the instance: building it dominates a parse of a small document.
`tabnas_markdown::parse` keeps one shared default instance for the
one-off case:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = tabnas_markdown::parse("Hello *world*")?;
    assert_eq!(doc.to_json()["children"][0]["children"][1]["type"], "emphasis");
    Ok(())
}
```

`make_with` takes options. The only two, in every runtime, are `gfm`
(default `true`) and `breaks` (default `false`, which promotes soft line
breaks to hard breaks when set). `Options::COMMONMARK` is the pair with
`gfm` off, and the output is then plain CommonMark, byte for byte:

```rust
use tabnas_markdown::{make_with, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = make_with(&Options::COMMONMARK);
    let doc = parser.parse("~~not struck~~")?;
    assert_eq!(
        doc.to_json()["children"][0]["children"][0]["value"],
        "~~not struck~~"
    );
    Ok(())
}
```

To install on an engine you are configuring yourself, hand the plugin
descriptor to `use_plugin`, or call `markdown` with typed options:

```rust
use tabnas_markdown::{markdown, plugin, Options, Tabnas};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tn = Tabnas::new();
    tn.use_plugin(plugin(), None)?;
    assert_eq!(tn.parse("# Hello")?.to_json()["children"][0]["type"], "heading");

    let mut bare = Tabnas::new();
    markdown(&mut bare, &Options { gfm: true, breaks: true })?;
    let doc = bare.parse("a\nb")?;
    assert_eq!(doc.to_json()["children"][0]["children"][1]["type"], "break");
    Ok(())
}
```

### What is not implemented

**Footnotes are not implemented.** They are a GitHub product feature,
not part of the GFM spec suite. Because `[^1]` is a valid CommonMark
link label, a GitHub-authored footnote does not error: it renders
silently as a broken link. Nothing outside CommonMark and GFM is
implemented either: no math, front matter, definition lists, heading
attributes, admonitions, wiki links, emoji shortcodes, highlight or
sub/superscript. One collision follows from that: GFM's single-tilde
strikethrough takes the syntax other dialects use for subscript, so
under the default `gfm: true` a subscript becomes a deletion.

```rust
use tabnas_markdown::{to_html, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Options::default();
    assert_eq!(
        to_html("Text[^1]\n\n[^1]: note", &opts),
        "<p>Text<a href=\"note\">^1</a></p>\n"
    );
    assert_eq!(to_html("H~2~O", &opts), "<p>H<del>2</del>O</p>\n");
    Ok(())
}
```

## Install

The `tabnas` engine crate is not published to a registry, so it and this
crate are consumed as **sibling checkouts**, the standard tabnas
development model. Clone `https://github.com/tabnas/parser` and
`https://github.com/tabnas/markdown` next to each other and point at
them:

```toml
[dependencies]
tabnas-markdown = { path = "../markdown/rs" }
tabnas = { path = "../parser/rs" }
```

The first entry is enough for the examples above: the crate re-exports
`Tabnas` and the error type (as `MarkdownError`), and the AST comes back
as a `tabnas::Value` whose methods need no import. Add the second entry
as soon as your code names any other engine type, because a crate's
dependencies are not passed on to its dependents. Running the tests
needs a third sibling, `https://github.com/tabnas/support`, which holds
the shared fixture runner and is a dev-dependency only.

The crate needs Rust 1.85 or later.

## Differences from the canonical TypeScript

Every example, fixture and golden agrees across the three runtimes, so
the differences are in the surface and the machinery, never in the
output:

- **Options travel by reference, and `Options` is a plain `Copy`
  struct.** `parse_document(src, &opts)`, `to_html(src, &opts)`,
  `parse_tree(src, &opts)`; `Options::default()` is the package defaults
  and `Options::COMMONMARK` is plain CommonMark. The plugin descriptor
  still reads a `gfm` / `breaks` option bag, so
  `use_plugin(plugin(), Some(bag))` works the way `use(Markdown, opts)`
  does.
- **Numbers in the AST are `f64`.** `tabnas::Value` holds every number
  as a float, so `to_json()` renders a heading's `depth` as `1.0` where
  the TypeScript prints `1`. The value is the same number, and the
  shared fixture runner compares numerically, so the fixtures pass
  unchanged; compare against `1.0`, or read it with `as_f64()`.
- **Errors are the engine's own.** CommonMark defines an output for
  every input, so the parser refuses nothing and the engine-free
  functions return plain values. `parse` and `Tabnas::parse` return
  `Result<Value, MarkdownError>` because the engine can still fail on
  its own account (a budget exceeded, a panic in another plugin's
  callback); `MarkdownError` is the engine's `TabnasError`, re-exported.
- **The entity table is generated, not the standard library's.**
  `src/entities.rs` holds the same 2125 semicolon-terminated names as
  `ts/src/entities.ts`, and a test reads the TypeScript table and
  asserts the two agree entry for entry. Unicode punctuation comes from
  the `regex` crate's `\p{P}` and `\p{S}` tables, the same property
  escapes the TypeScript uses, and label case folding uses the full
  Unicode mappings, so `ß` folds as JavaScript folds it.
- **Per-parse state lives in a thread-local slab.** The other runtimes
  hang the live block parser on the engine context as an opaque object;
  this engine's context holds only values, so the context carries a
  slot index instead. The inline phase's own engine instance is built
  once per install and shared by every parse, as in TypeScript (Go
  builds one per parse), which is sound because the engine parses
  through `&self` and `Tabnas` is `Send + Sync`.
- **Token columns count characters.** After an astral character (one
  outside the Basic Multilingual Plane) the engine's token columns are
  one character further left than the TypeScript engine's, which counts
  UTF-16 units. That is the engine's recorded divergence, inherited
  here; it is visible only on the token stream, never in the AST or the
  native tree, whose `sourcepos` matches the TypeScript goldens.
- **Nesting is capped.** A document may nest
  `MAX_CONTAINER_NESTING` (100) block quotes, lists and list items, and
  `MAX_INLINE_NESTING` (50) emphasis, link, and image wrappers. Deeper
  markers stay literal text. The canonical TypeScript has no such cap,
  so a document nested past either bound parses to a different tree
  here; `DIVERGENCE.md` records it and `tests/robust_test.rs` pins the
  boundary. The reason is the AST type rather than this crate: every
  phase here is iterative, but the AST is a `tabnas::Value`, whose
  `to_json()` and whose drop both recurse once per level, and an
  unbounded document aborted the process in the caller's own stack
  frame. The canonical runtime reaches the same wall as a `RangeError`
  from `JSON.stringify`, which the caller can catch.

## Build and test

The engine and the fixture runner are path dependencies on sibling
checkouts, so there is nothing to fetch:

```bash
cargo test --all-targets && cargo test --doc
```

Or, from the repository root, `make test-rs`, which adds clippy. For
what CI would say, including formatting and the lockfile check, run
`ci/rust/run.sh`.

The suite runs the shared `../test/spec/*.tsv` AST fixtures (83 rows in
10 files, through the same `tabnas-support` runner the TypeScript and Go
suites use), the native-tree goldens in `../test/spec/tree/`, the
652-example CommonMark corpus and the 24-example GFM corpus, each graded
twice (engine-free and through the plugin), the differential gate
between those two paths, and the in-language tests: the plugin surface,
the pure recognisers, the entity table, adversarial input, token
columns, the inline driver's contract, scaling, and the version sites.
The Rust examples on this page run as doctests.

## License

MIT.
