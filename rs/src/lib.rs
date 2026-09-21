/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// The engine's error carries a code, position, hint and a formatted
// report, so it is large by design and `Result<_, TabnasError>` trips
// clippy's `result_large_err`. The engine allows the lint at its own
// crate root for the same reason; boxing here instead would make `parse`
// return a different shape from `Tabnas::parse` and from the TypeScript
// and Go ports, which is a worse trade than the lint.
#![allow(clippy::result_large_err)]

//! `tabnas-markdown`: a CommonMark 0.31.2 parser with the GFM extensions
//! (tables, strikethrough, task list items, autolink literals and the
//! disallowed-raw-HTML filter), built as a plugin on the bare `tabnas`
//! engine.
//!
//! ```
//! let doc = tabnas_markdown::parse("# Hello *world*")?;
//! assert_eq!(doc.to_json()["children"][0]["type"], "heading");
//! # Ok::<(), tabnas_markdown::MarkdownError>(())
//! ```
//!
//! This file is only the plugin wiring and the public surface. The parser
//! itself is engine-free and lives in [`commonmark`] and the modules it
//! pulls in ([`block`], [`inline`], [`html`], [`common`], [`node`]), so it
//! can be exercised, and the conformance suite run, without the engine.
//! Nothing under `commonmark.rs` may import it.
//!
//! The parse result is a native CommonMark node tree ([`Tree`]); [`ast`]
//! projects it to the map-based JSON AST this package has always returned
//! (as a [`tabnas::Value`]), and [`html`] renders it for [`to_html`].
//!
//! This is a port of `ts/src/*.ts`, which is canonical, and a sibling of
//! the Go port in `go/`. See AGENTS.md.

use std::sync::{Arc, OnceLock};

use tabnas::{AltSpec, Plugin, PluginError, Value, TIN_ZZ};

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml` and `bash` fences
/// are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

pub mod ast;
pub mod block;
pub mod common;
pub mod commonmark;
pub mod engine_block;
pub mod engine_inline;
mod entities;
pub mod html;
pub mod inline;
pub mod node;
pub mod options;

pub use ast::to_ast;
pub use html::{render_html, render_html_with, DISALLOWED_TAGS};
pub use node::{
    Align, ListData, ListType, MdNode, NodeId, NodeType, SourcePos, TableAlign, Tree, WalkEvent,
    Walker,
};
pub use options::{Options, RefDef, RefMap, DEFAULT_OPTIONS};
pub use tabnas::Tabnas;

/// This crate's version. It MUST equal `ts/package.json` "version": the
/// release orchestrator rewrites both, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/markdown.ts` and
/// `const VERSION` in `go/markdown.go`.
pub const VERSION: &str = "0.7.4";

/// The error a failed engine parse produces, re-exported so callers need
/// not depend on the engine crate directly. The parser itself defines an
/// output for every input; this is the engine's own failure shape (a
/// budget exceeded, a panic in another plugin's callback).
pub use tabnas::TabnasError as MarkdownError;

/// The plugin's name, as `Tabnas::use_plugin` records it.
pub const PLUGIN_NAME: &str = "Markdown";

// ---------------------------------------------------------------------------
// Public parse API (engine-free)

/// Parse Markdown to the mdast-adjacent JSON AST.
pub fn parse_document(src: &str, options: &Options) -> Value {
    to_ast(&commonmark::parse(src, *options), options)
}

/// Parse a single run of inline Markdown, returning the inline children of
/// the resulting paragraph. Empty when the text does not start with a
/// paragraph.
pub fn parse_inline(text: &str, options: &Options) -> Vec<Value> {
    let doc = parse_document(text, options);
    let Some(Value::Array(children)) = engine_block::value_get(&doc, "children") else {
        return Vec::new();
    };
    let Some(first) = children.first() else {
        return Vec::new();
    };
    if !matches!(engine_block::value_get(first, "type"), Some(Value::String(t)) if t == "paragraph")
    {
        return Vec::new();
    }
    match engine_block::value_get(first, "children") {
        Some(Value::Array(inlines)) => inlines.as_ref().clone(),
        _ => Vec::new(),
    }
}

/// Parse Markdown and render it to CommonMark-conformant HTML.
pub fn to_html(src: &str, options: &Options) -> String {
    render_html_with(&commonmark::parse(src, *options), *options)
}

/// Parse Markdown to the native CommonMark node tree.
pub fn parse_tree(src: &str, options: &Options) -> Tree {
    commonmark::parse(src, *options)
}

// ---------------------------------------------------------------------------
// Plugin wiring

/// Install the parser on a tabnas engine instance.
///
/// The engine parse IS the parse: the `mdLine` custom matcher
/// (`engine_block.rs`) lexes one `#LB` token per physical line,
/// `parse.prepare` seeds a fresh block parser for the parse, the `line`
/// rule feeds each matched line to the shared `incorporate_line`, and the
/// `markdown` rule's close action on `#ZZ` finalizes, runs the shared
/// inline phase, and projects the public AST into the rule's node. No
/// recognition or block decision happens here: the engine owns
/// tokenization, dispatch and state carriage; the algorithm stays in the
/// engine-free core (the anti-drift rule, dx-report §42), which is what
/// keeps this path byte-identical to [`parse_document`] (asserted by the
/// differential gate and the engine-path conformance harness).
///
/// Use this to add Markdown to an engine you are configuring yourself;
/// [`make`] and [`make_with`] are the short way to a ready one.
pub fn markdown(tn: &mut Tabnas, options: &Options) -> Result<(), PluginError> {
    let opts = *options;

    let lb = tn.token(engine_block::LINE_TOKEN);

    // Every built-in matcher is off: deliberate configuration, not
    // defense. Markdown's block alphabet is *lines*, so the one registered
    // matcher is the complete lexical description of this phase: mdLine,
    // at an order ahead of every built-in, consuming each physical line
    // whole. (The built-ins would misread the syntax anyway: backtick code
    // spans lex as unterminated strings, `# heading` as a comment,
    // `1. list` as a number.)
    tn.set_options(|o| {
        o.fixed.lex = false;
        o.space.lex = false;
        o.line.lex = false;
        o.text.lex = false;
        o.string.lex = false;
        o.comment.lex = false;
        o.number.lex = false;
        o.value.lex = false;
        o.lex.empty_result = empty_document();
        o.rule.start = "markdown".to_string();
        // The GFM dialect is subtracted, not branched around: without the
        // option, the tagged alts below simply do not exist in this
        // instance's grammar.
        o.rule.exclude = if opts.gfm {
            String::new()
        } else {
            "gfm".to_string()
        };
    })?;

    tn.options.lex.matchers.insert(
        engine_block::MD_LINE_MATCHER.to_string(),
        tabnas::LexMatcher {
            name: engine_block::MD_LINE_MATCHER.to_string(),
            order: engine_block::MD_LINE_ORDER,
            matcher: None,
            imperative: Some(Arc::new(engine_block::make_md_line_matcher(lb))),
            factory: None,
        },
    );
    tn.options.parse.named_prepare.insert(
        engine_block::MD_PREPARE.to_string(),
        tabnas::ParsePrepare::Context(Arc::new(engine_block::make_md_prepare(opts))),
    );

    // The inline phase's own engine instance, built once per install and
    // shared by every parse (see `engine_inline.rs` for why that is sound
    // here and not in Go).
    let inline_tn = Arc::new(engine_inline::make_inline_tn(&opts));
    let run_inlines = move |tree: Tree, refmap: RefMap| {
        engine_inline::parse_inlines_engine(&inline_tn, tree, refmap, &opts)
    };

    tn.define_rule("markdown", |rs| {
        rs.clear();
        rs.add_open(AltSpec {
            p: Some("line".to_string()),
            ..Default::default()
        });
        let mut finish = AltSpec {
            s: vec![vec![TIN_ZZ]],
            ..Default::default()
        };
        finish.add_action(engine_block::make_md_finish_action(opts, run_inlines));
        rs.add_close(finish);
        rs.add_close(AltSpec::default());
    });

    // One `#LB` consumed per iteration, tail-recursing via `r` until only
    // `#ZZ` remains; the empty alts are the fall-through that hands
    // control back to markdown's close.
    //
    // The first alt is the GFM extension seam: tagged `g: "gfm"`, gated on
    // the token's tblArm bit, arming the shared table probe for this line.
    // It is injected exactly the way a downstream dialect would extend
    // this grammar (an alt ahead of the base one, subtractable by its
    // group tag) and gfm:false removes it via `rule.exclude` above, so the
    // base dialect is the grammar minus the tagged alts, visibly.
    tn.define_rule("line", |rs| {
        rs.clear();
        let mut gfm = AltSpec {
            s: vec![vec![lb]],
            c_fn: Some(Arc::new(engine_block::md_line_gfm_condition)),
            r: Some("line".to_string()),
            g: "gfm".to_string(),
            ..Default::default()
        };
        gfm.add_action(engine_block::md_line_gfm_action);
        rs.add_open(gfm);
        let mut base = AltSpec {
            s: vec![vec![lb]],
            r: Some("line".to_string()),
            ..Default::default()
        };
        base.add_action(engine_block::md_line_action);
        rs.add_open(base);
        rs.add_open(AltSpec::default());
        rs.add_close(AltSpec::default());
    });

    Ok(())
}

/// The AST of an empty document, which is what the engine answers for an
/// empty source before any rule runs.
fn empty_document() -> Value {
    let mut map = indexmap::IndexMap::with_capacity(2);
    map.insert("type".to_string(), Value::String("document".to_string()));
    map.insert("children".to_string(), Value::array(Vec::new()));
    Value::object(map)
}

/// The plugin descriptor, for [`Tabnas::use_plugin`]. Options are read
/// from the plugin option bag (`gfm`, `breaks`), so a caller who has one
/// already can hand it over; [`markdown`] is the typed way in.
pub fn plugin() -> Plugin {
    Plugin::new(PLUGIN_NAME, |tn, options| {
        markdown(tn, &Options::resolve(options))
    })
    .with_defaults(DEFAULT_OPTIONS.to_value())
}

/// Build a tabnas engine with the Markdown plugin installed under the
/// given options. The Rust counterpart of `new Tabnas().use(Markdown,
/// opts)`.
///
/// Reuse the result: building the instance dominates a parse of a small
/// document. This goes through [`plugin`] rather than calling
/// [`markdown`] directly, so the instance records the plugin and its
/// resolved options.
///
/// ```
/// let parser = tabnas_markdown::make_with(&tabnas_markdown::Options::COMMONMARK);
/// let doc = parser.parse("~~not struck~~")?;
/// assert_eq!(
///     doc.to_json()["children"][0]["children"][0]["value"],
///     "~~not struck~~"
/// );
/// # Ok::<(), tabnas_markdown::MarkdownError>(())
/// ```
pub fn make_with(options: &Options) -> Tabnas {
    let mut tn = Tabnas::new();
    tn.use_plugin(plugin(), Some(options.to_value()))
        .expect("the Markdown plugin installs on a bare engine");
    tn
}

/// [`make_with`] under the package defaults (`gfm` on, `breaks` off).
///
/// ```
/// let parser = tabnas_markdown::make();
/// let doc = parser.parse("- [x] done")?;
/// assert_eq!(doc.to_json()["children"][0]["children"][0]["checked"], true);
/// # Ok::<(), tabnas_markdown::MarkdownError>(())
/// ```
pub fn make() -> Tabnas {
    make_with(&DEFAULT_OPTIONS)
}

/// Parse a Markdown source with the shared default engine instance.
///
/// The instance is built once, on first use, and reused after that:
/// [`Tabnas::parse`] takes `&self` and builds a fresh parse context per
/// call, and `Tabnas` is `Send + Sync`, so concurrent callers share one
/// installed grammar instead of each rebuilding it. Use [`make`] or
/// [`make_with`] when the parser needs configuring: that returns a fresh
/// instance and leaves this one alone.
///
/// ```
/// let doc = tabnas_markdown::parse("Hello *world*")?;
/// let para = &doc.to_json()["children"][0];
/// assert_eq!(para["type"], "paragraph");
/// assert_eq!(para["children"][1]["type"], "emphasis");
/// # Ok::<(), tabnas_markdown::MarkdownError>(())
/// ```
pub fn parse(src: &str) -> Result<Value, MarkdownError> {
    static DEFAULT: OnceLock<Tabnas> = OnceLock::new();
    DEFAULT.get_or_init(make).parse(src)
}

/// Parse through the engine and also hand back the native tree the parse
/// built: the `keepTree` handshake the engine-path conformance harness
/// uses, so it can render with the same [`render_html`] the engine-free
/// path uses and isolate the parse from the rendering.
///
/// The tree is `None` for an empty source, which the engine answers with
/// `lex.empty_result` before any rule runs. The instance must have the
/// plugin installed; one without it parses, but hands back no tree.
pub fn parse_keep_tree(tn: &Tabnas, src: &str) -> Result<(Value, Option<Tree>), MarkdownError> {
    // Clear anything an aborted earlier parse on this thread may have
    // left, so a stale tree is never mistaken for this parse's.
    let _ = engine_block::take_kept_tree();

    let mut md = indexmap::IndexMap::with_capacity(1);
    md.insert("keepTree".to_string(), Value::Bool(true));
    let mut meta = indexmap::IndexMap::with_capacity(1);
    meta.insert("md".to_string(), Value::object(md));

    let ast = tn.parse_with_meta(src, Value::object(meta))?;
    Ok((ast, engine_block::take_kept_tree()))
}
