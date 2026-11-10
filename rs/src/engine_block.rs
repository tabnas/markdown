/* Copyright (c) 2026 Richard Rodger, MIT License */

//! The engine-facing block driver: a custom lexer matcher that cuts the
//! source one physical line at a time, runs the shared block algorithm over
//! each line, and hands the engine the blocks that line opened and closed
//! as tokens; and the rule actions that build the AST from those tokens.
//! Port of `ts/src/engine-block.ts` and `go/engineblock.go`.
//!
//! The grammar's rules open and close every block (`lib.rs`): a
//! container's rule opens on the token that opens it (`#BQ` for a block
//! quote, `#LS` for a list, ...) and closes on `#EN`, and a leaf's rule
//! opens and closes on its one token (`#HR`, `#CD`, `#DF`, ...). A block's
//! inline content is parsed when the block closes, since the block bounds
//! it, and its inline nodes are tokens and rules too, so the rule passes
//! carry the whole tree, a block at a time, and the value the rules build
//! is that tree.
//!
//! The block decisions themselves are the shared [`BlockParser`]'s, the
//! same code the engine-free path runs (the anti-drift rule, dx-report
//! §42). This driver turns what it decided into tokens in the order it
//! decided it ([`BlockParser::events`]), and nothing here recognizes any
//! Markdown of its own.
//!
//! Each line is one `#LB` token, cut through the lexer's window, so a
//! grammar that declares `lex.window` is read a window at a time. The
//! tokens of the blocks the line closed and opened come first, of no
//! width, where the line starts, one each time the lexer asks, and then the
//! line itself: the tokens are in source order, and the last line's tokens
//! also close every block still open, since no token follows the last line
//! but the end.
//!
//! # State carriage
//!
//! The other two runtimes hang the live block parser on `ctx.u.md` as an
//! opaque object. The Rust engine's `Context::u` holds only [`Value`]s,
//! so the parser itself lives in a thread-local slab and `ctx.u["md"]`
//! carries its slot index. The slot is claimed by the `parse.prepare`
//! hook and released by the `markdown` rule's finish action, which is the
//! last thing a parse runs. A parse the engine aborts between the two
//! (a budget error, a panic in some other plugin's callback) never runs
//! the finish action, so its slot is reclaimed by liveness instead: the
//! index travels inside a one-element array whose `Arc` the slot also
//! holds, the engine drops the context, and with it the array, when the
//! parse ends however it ends, and the next prepare on the thread frees
//! every slot whose `Arc` it is the last holder of. The slab therefore
//! never holds more than the parses live on the thread plus the aborted
//! ones since the last prepare, whatever a long-lived worker is fed.
//!
//! Thread-local rather than keyed by parse: a [`tabnas::Tabnas`] instance
//! is `Send + Sync` and parses through `&self`, so one installed plugin
//! may serve many threads at once. Each thread's slab is its own, and a
//! parse never leaves the thread it started on.
//!
//! This module may import engine types: it is reachable only from
//! `lib.rs`, never from `commonmark.rs`, so the engine-free layering rule
//! holds (see AGENTS.md).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use tabnas::{Context, Lexer, Rule, Tabnas, Tin, Token, Value};

use crate::ast::{block_head, block_tail, definition_node, inline_children, leaf_block, object};
use crate::block::{BlockEvent, BlockParser};
use crate::engine_inline::{parse_block_engine, parse_inlines_engine};
use crate::inline::InlineParser;
use crate::node::{NodeId, NodeType, Tree};
use crate::options::Options;

/// One physical line, read: the blocks it made precede it.
pub const LINE_TOKEN: &str = "#LB";

/// A container's end: its value is the fields that follow its children.
pub const END_TOKEN: &str = "#EN";

/// The `mdBlock` matcher's name in `options.lex.matchers`.
pub const MD_BLOCK_MATCHER: &str = "mdBlock";

/// The `mdBlock` matcher's order: ahead of every built-in, so a line is
/// consumed whole before any built-in matcher sees its first character.
pub const MD_BLOCK_ORDER: f64 = 100_000.0;

/// The `parse.prepare` hook's name.
pub const MD_PREPARE: &str = "md-reset";

/// The key under `ctx.u` that carries the block state's slot index.
pub const MD_STATE_KEY: &str = "md";

/// A node of the AST, its rule and the token its rule opens on. A
/// container's token carries its head (the fields before its `children`,
/// `type` first), and its `#EN` its tail; a leaf's token carries the whole
/// node. The rule is named for the node's type. The GFM nodes are the
/// `gfm` group's.
#[derive(Debug, Clone, Copy)]
pub struct NodeRule {
    pub rule: &'static str,
    pub token: &'static str,
    pub container: bool,
    pub gfm: bool,
    pub desc: &'static str,
}

const fn node_rule(
    rule: &'static str,
    token: &'static str,
    container: bool,
    gfm: bool,
    desc: &'static str,
) -> NodeRule {
    NodeRule {
        rule,
        token,
        container,
        gfm,
        desc,
    }
}

/// Every node's rule, as `ts/src/engine-block.ts` lists them.
pub const NODE_RULES: &[NodeRule] = &[
    node_rule("blockquote", "#BQ", true, false, "block quote"),
    node_rule("list", "#LS", true, false, "list"),
    node_rule("listItem", "#LI", true, false, "list item"),
    node_rule("paragraph", "#PA", true, false, "paragraph"),
    node_rule("heading", "#HD", true, false, "ATX or setext heading"),
    node_rule("table", "#TB", true, true, "GFM table"),
    node_rule("tableRow", "#TR", true, true, "table row"),
    node_rule("tableCell", "#TC", true, true, "table cell"),
    node_rule("emphasis", "#EM", true, false, "emphasis"),
    node_rule("strong", "#SG", true, false, "strong emphasis"),
    node_rule("delete", "#DL", true, true, "strikethrough"),
    node_rule("link", "#LK", true, false, "link"),
    node_rule("linkReference", "#LR", true, false, "link reference"),
    node_rule("thematicBreak", "#HR", false, false, "thematic break"),
    node_rule("code", "#CD", false, false, "fenced or indented code"),
    node_rule("html", "#HT", false, false, "HTML block or raw HTML"),
    node_rule(
        "definition",
        "#DF",
        false,
        false,
        "link reference definition",
    ),
    node_rule("text", "#TT", false, false, "text"),
    node_rule("inlineCode", "#IC", false, false, "code span"),
    node_rule("break", "#BR", false, false, "hard line break"),
    node_rule("image", "#IM", false, false, "image"),
    node_rule("imageReference", "#IR", false, false, "image reference"),
];

/// The token a node of each AST type opens with.
fn token_of(node_type: &str) -> &'static str {
    NODE_RULES
        .iter()
        .find(|n| n.rule == node_type)
        .map_or(LINE_TOKEN, |n| n.token)
}

/// The `type` of an AST node's fields, or of an AST node.
fn type_of<'v>(fields: &'v [(&'static str, Value)]) -> &'v str {
    match fields.iter().find(|(k, _)| *k == "type") {
        Some((_, Value::String(t))) => t,
        _ => "",
    }
}

fn node_type_of(node: &Value) -> &str {
    match value_get(node, "type") {
        Some(Value::String(t)) => t,
        _ => "",
    }
}

/// A token the line matcher makes: its name and its value.
type Made = (&'static str, Value);

/// One key of an object value. Both object spellings the engine has.
pub(crate) fn value_get<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    match value {
        Value::Object(map) => map.get(key),
        Value::MapRef(map) => map.value.get(key),
        _ => None,
    }
}

/// The slot index a prepare hook stored under `ctx.u[key]`: a bare
/// number (the inline driver's form), or a number alone in an array (the
/// block driver's form, where the array's `Arc` doubles as the lease
/// described at the top of this module).
pub(crate) fn slot_of(ctx: &Context, key: &str) -> Option<usize> {
    let mut value = ctx.u.get(key)?;
    if let Value::Array(items) = value {
        value = items.first()?;
    }
    match value {
        Value::Number(n) if *n >= 0.0 => Some(*n as usize),
        _ => None,
    }
}

// --- the per-parse state ----------------------------------------------------

/// What `ctx.u["md"]` points at: the live block parser, the keepTree
/// request read off the parse meta, and the line matcher's own state.
struct BlockState {
    bp: BlockParser,
    keep_tree: bool,
    /// The inline parse a block's AST is made with: references kept. Held
    /// in an option so that it can travel with the nested inline parse.
    inline: Option<InlineParser>,
    /// The tokens the line at the cursor made, handed out one a call
    /// before the line's own token (`next` the one due), and whether that
    /// line has been read: its blocks are decided, and its `#LB` is what is
    /// left.
    made: Vec<Made>,
    next: usize,
    read: bool,
    /// The array `ctx.u["md"]` holds, shared with the context. While the
    /// context lives the count is at least two; once the engine has
    /// dropped it, this is the last holder and the slot is stale.
    lease: Arc<Vec<Value>>,
}

impl BlockState {
    fn stale(&self) -> bool {
        Arc::strong_count(&self.lease) == 1
    }
}

thread_local! {
    static BLOCK_STATES: RefCell<Vec<Option<BlockState>>> = const { RefCell::new(Vec::new()) };

    /// The native tree a `keepTree` parse hands back. Set by the finish
    /// action, taken by [`take_kept_tree`] right after the parse returns.
    static KEPT_TREE: RefCell<Option<Tree>> = const { RefCell::new(None) };
}

/// Claim a slot for a new parse and return the value `ctx.u["md"]` must
/// carry. Slots whose parse has ended without the finish action (see the
/// module docs) are freed first, so the slab never grows past the parses
/// that are actually live on this thread.
fn block_slot_insert(bp: BlockParser, keep_tree: bool, options: Options) -> (usize, Value) {
    BLOCK_STATES.with(|cell| {
        let mut slab = cell.borrow_mut();
        for entry in slab.iter_mut() {
            if entry.as_ref().is_some_and(BlockState::stale) {
                *entry = None;
            }
        }
        let slot = slab.iter().position(Option::is_none).unwrap_or_else(|| {
            slab.push(None);
            slab.len() - 1
        });
        let lease = Arc::new(vec![Value::Number(slot as f64)]);
        slab[slot] = Some(BlockState {
            bp,
            keep_tree,
            inline: Some(InlineParser::keeping_references(options)),
            made: Vec::new(),
            next: 0,
            read: false,
            lease: Arc::clone(&lease),
        });
        (slot, Value::Array(lease))
    })
}

/// How many slots hold a block state right now, stale ones included.
#[cfg(test)]
fn block_slots_held() -> usize {
    BLOCK_STATES.with(|cell| cell.borrow().iter().filter(|s| s.is_some()).count())
}

fn block_slot_take(slot: usize) -> Option<BlockState> {
    BLOCK_STATES.with(|cell| cell.borrow_mut().get_mut(slot).and_then(Option::take))
}

fn with_block_state<R>(ctx: &Context, f: impl FnOnce(&mut BlockState) -> R) -> Option<R> {
    let slot = slot_of(ctx, MD_STATE_KEY)?;
    BLOCK_STATES.with(|cell| {
        let mut slab = cell.borrow_mut();
        slab.get_mut(slot).and_then(Option::as_mut).map(f)
    })
}

/// The native tree the last `keepTree` parse on this thread produced, if
/// any. `None` after a parse that did not ask for it, and after an empty
/// source, which the engine answers with `lex.empty_result` before any
/// rule runs.
pub fn take_kept_tree() -> Option<Tree> {
    KEPT_TREE.with(|cell| cell.borrow_mut().take())
}

/// True when the parse meta carries `{"md": {"keepTree": true}}`.
fn keep_tree_requested(meta: &Value) -> bool {
    matches!(
        value_get(meta, MD_STATE_KEY).and_then(|md| value_get(md, "keepTree")),
        Some(Value::Bool(true))
    )
}

// --- the lexer matcher ------------------------------------------------------

/// One physical line at the cursor.
struct Line {
    text: String,
    src: String,
    /// The characters the line takes, its terminator included.
    chars: usize,
    last: bool,
}

/// One physical line at the cursor, read through the lexer's window: up to
/// (not including) the next `\n`, `\r` or `\r\n`, with section 2.3's NUL
/// replacement. `last` says whether the source ends after it (which is why
/// a final line ending does not make a trailing blank line). The window is
/// filled past the line's end, so a token never ends at the window's end,
/// where the lexer would cut it again over a wider one, and a line longer
/// than the window is read on in doubling steps. A line is read twice, once
/// to decide its blocks and once for its own token, and gives the same both
/// times. Every byte compared is ASCII; [`Lexer::fill`] counts characters.
fn cut_line(lexer: &mut Lexer<'_>) -> Option<Line> {
    if lexer.window().is_empty() && !lexer.fill(1) {
        return None;
    }

    let mut i = 0;
    loop {
        let w = lexer.window().as_bytes();
        while i < w.len() && w[i] != b'\n' && w[i] != b'\r' {
            i += 1;
        }
        if i < w.len() {
            break;
        }
        let had = w.len();
        let chars = lexer.window().chars().count();
        lexer.fill(2 * chars + 1);
        if lexer.window().len() == had {
            break;
        }
    }

    let text_chars = lexer.window()[..i].chars().count();
    let mut next = i;
    if i < lexer.window().len() {
        if lexer.window().as_bytes()[i] == b'\r' {
            if i + 1 >= lexer.window().len() {
                lexer.fill(text_chars + 2);
            }
            let w = lexer.window().as_bytes();
            next = if i + 1 < w.len() && w[i + 1] == b'\n' {
                i + 2
            } else {
                i + 1
            };
        } else {
            next = i + 1;
        }
    }

    let chars = text_chars + (next - i);
    let last = !lexer.fill(chars + 1);
    let w = lexer.window();
    let mut text = w[..i].to_string();
    if text.contains('\0') {
        text = text.replace('\0', "\u{FFFD}");
    }
    Some(Line {
        text,
        src: w[..next].to_string(),
        chars,
        last,
    })
}

/// What one invocation of the matcher hands out.
enum Out {
    /// A token of a block, of no width.
    Block(Made),
    /// The line itself.
    Line,
}

/// Build the `mdBlock` matcher. Each invocation returns one token. At a
/// line not yet read, it reads the line, feeds it to the shared
/// [`BlockParser`] (and, for the last line, closes every block still
/// open), and gathers the tokens of the blocks the line closed and opened;
/// those are handed out first, of no width, where the line starts, and then
/// the line itself as a `#LB` token. Row and column bookkeeping is left to
/// the lexer's own `advance_chars`, which counts CHARACTERS and resets the
/// column on a consumed `\n`; `tests/engine_columns_test.rs` pins the
/// resulting positions. Nothing reads the source but [`cut_line`], through
/// the window.
pub fn make_md_block_matcher(
    opts: Options,
    tins: HashMap<&'static str, Tin>,
    inline_tn: Arc<Tabnas>,
) -> impl for<'s> Fn(&mut Lexer<'s>, &mut Rule, &mut Context) -> Option<Token> + Send + Sync + 'static
{
    move |lexer, _rule, ctx| {
        let tin = |name: &str| tins.get(name).copied().unwrap_or_default();

        let pending = with_block_state(ctx, |state| {
            if state.next < state.made.len() {
                let made = state.made[state.next].clone();
                state.next += 1;
                Some(made)
            } else {
                None
            }
        })?;
        if let Some((name, value)) = pending {
            let point = lexer.point();
            return Some(lexer.token(name, tin(name), value, "", point));
        }

        let line = cut_line(lexer)?;

        let out = with_block_state(ctx, |state| {
            if !state.read {
                state.bp.incorporate_line(&line.text);
                if line.last {
                    state.bp.close_all();
                }
                let events = state.bp.events.take().unwrap_or_default();
                state.bp.events = Some(Vec::new());
                let mut made = Vec::new();
                for event in events {
                    event_tokens(state, &opts, &inline_tn, event, &mut made);
                }
                // The blocks the line took out of the tree with no close
                // of their own are written by nothing; a parse that keeps
                // no tree gives their slots back.
                for block in std::mem::take(&mut state.bp.dropped) {
                    if !state.keep_tree {
                        state.bp.release_block(block);
                    }
                }
                if let Some(first) = made.first().cloned() {
                    state.made = made;
                    state.next = 1;
                    state.read = true;
                    return Out::Block(first);
                }
            }
            state.made.clear();
            state.next = 0;
            state.read = false;
            Out::Line
        })?;

        let point = lexer.point();
        match out {
            Out::Block((name, value)) => Some(lexer.token(name, tin(name), value, "", point)),
            Out::Line => {
                let token = lexer.token(
                    LINE_TOKEN,
                    tin(LINE_TOKEN),
                    Value::Undefined,
                    line.src.as_str(),
                    point,
                );
                lexer.advance_chars(line.chars);
                Some(token)
            }
        }
    }
}

/// The tokens one block event makes, appended to `out`.
fn event_tokens(
    state: &mut BlockState,
    opts: &Options,
    inline_tn: &Tabnas,
    event: BlockEvent,
    out: &mut Vec<Made>,
) {
    match event {
        BlockEvent::Open(id) => {
            let head = block_head(&state.bp.tree, id);
            out.push((token_of(type_of(&head)), object(head)));
        }

        BlockEvent::Row(row) => {
            out.push(("#TR", object(block_head(&state.bp.tree, row))));
            let mut cell = state.bp.tree.get(row).first_child;
            while let Some(c) = cell {
                inline_block_tokens(state, opts, inline_tn, c, out);
                cell = state.bp.tree.get(c).next;
            }
            out.push((END_TOKEN, object(block_tail(&state.bp.tree, row))));
        }

        BlockEvent::Definition(def) => out.push(("#DF", definition_node(&def))),

        BlockEvent::Close(id) => {
            close_tokens(state, opts, inline_tn, id, out);
            // Its tokens are written: a parse that keeps no tree lets go of
            // a block at the top of the document, with everything below it,
            // and of a paragraph that held nothing but definitions, which
            // is out of the tree already. Without this the arena kept every
            // block to the document's end (tabnas/markdown#100).
            if !state.keep_tree
                && (state.bp.is_top_level(id) || state.bp.tree.get(id).parent.is_none())
            {
                state.bp.release_block(id);
            }
        }
    }
}

/// The tokens a block's `Close` makes, appended to `out`.
fn close_tokens(
    state: &mut BlockState,
    opts: &Options,
    inline_tn: &Tabnas,
    id: NodeId,
    out: &mut Vec<Made>,
) {
    match state.bp.tree.get(id).node_type {
        NodeType::BlockQuote | NodeType::List | NodeType::Item | NodeType::Table => {
            out.push((END_TOKEN, object(block_tail(&state.bp.tree, id))));
        }

        NodeType::Paragraph | NodeType::Heading => {
            // A paragraph that held nothing but definitions is gone:
            // its definitions were written as they were read.
            if state.bp.tree.get(id).parent.is_some() {
                inline_block_tokens(state, opts, inline_tn, id, out);
            }
        }

        NodeType::ThematicBreak | NodeType::CodeBlock | NodeType::HtmlBlock => {
            if let Some(leaf) = leaf_block(&state.bp.tree, id) {
                out.push((token_of(node_type_of(&leaf)), leaf));
            }
            if !state.keep_tree {
                release(&mut state.bp.tree, id);
            }
        }

        _ => {}
    }
}

/// A block whose content is inline nodes (a paragraph, a heading, a table
/// cell), parsed now: its head, its inline nodes, its end. The block's raw
/// text is parsed in a tree of its own, so the native tree keeps it when
/// the caller wants that tree back, which is parsed again, references
/// resolved, once the document ends.
fn inline_block_tokens(
    state: &mut BlockState,
    opts: &Options,
    inline_tn: &Tabnas,
    node: NodeId,
    out: &mut Vec<Made>,
) {
    let head = block_head(&state.bp.tree, node);
    let tail = block_tail(&state.bp.tree, node);

    let mut scratch = Tree::new(state.bp.tree.get(node).node_type);
    let root = scratch.root();
    scratch.get_mut(root).string_content = state.bp.tree.get(node).string_content.clone();
    let parser = state
        .inline
        .take()
        .unwrap_or_else(|| InlineParser::keeping_references(*opts));
    let (parser, scratch) = parse_block_engine(inline_tn, parser, scratch, root);
    state.inline = Some(parser);

    out.push((token_of(type_of(&head)), object(head)));
    inline_tokens(inline_children(&scratch, root, opts), out);
    out.push((END_TOKEN, object(tail)));
    if !state.keep_tree {
        release(&mut state.bp.tree, node);
    }
}

/// The tokens of a run of inline nodes, in document order: a container's
/// head, its children's, its end; a leaf whole. Iterative, since inline
/// nesting is the input's to choose.
fn inline_tokens(nodes: Vec<Value>, out: &mut Vec<Made>) {
    let mut stack: Vec<std::vec::IntoIter<Value>> = vec![nodes.into_iter()];
    while let Some(top) = stack.last_mut() {
        let Some(node) = top.next() else {
            stack.pop();
            if !stack.is_empty() {
                out.push((END_TOKEN, object(Vec::new())));
            }
            continue;
        };
        let token = token_of(node_type_of(&node));
        match value_get(&node, "children") {
            Some(Value::Array(children)) => {
                let children = children.as_ref().clone();
                let mut head = IndexMap::new();
                if let Value::Object(fields) = &node {
                    for (k, v) in fields.iter() {
                        if k != "children" {
                            head.insert(k.clone(), v.clone());
                        }
                    }
                }
                out.push((token, Value::object(head)));
                stack.push(children.into_iter());
            }
            _ => out.push((token, node)),
        }
    }
}

/// Drop what a written block holds that nothing reads again: its raw text,
/// its literal and its inline nodes. The block itself stays in the native
/// tree, whose later lines still read its flags.
fn release(tree: &mut Tree, node: NodeId) {
    let n = tree.get_mut(node);
    n.string_content = String::new();
    n.literal = String::new();
    n.first_child = None;
    n.last_child = None;
}

// --- the actions ------------------------------------------------------------

/// The `parse.prepare` hook: fresh block-parser state for every parse,
/// with the keepTree request captured from the parse meta.
///
/// The engine runs this hook before it short-circuits an empty source
/// through `lex.empty_result`, and in a parse from a reader before it knows
/// whether the source is empty (`ctx.source` is empty in every such
/// parse), so a slot is claimed for an empty source too. Its finish action
/// never runs, and the next prepare on the thread reclaims the slot by its
/// lease (see the module docs).
pub fn make_md_prepare(opts: Options) -> impl Fn(&mut Context) + Send + Sync + 'static {
    move |ctx| {
        let mut bp = BlockParser::new(opts);
        bp.events = Some(Vec::new());
        let (_, lease) = block_slot_insert(bp, keep_tree_requested(&ctx.meta), opts);
        ctx.u.insert(MD_STATE_KEY.to_string(), lease);
    }
}

/// A list value with `item` added at its end.
fn list_push(list: &mut Value, item: Value) {
    if let Value::Array(items) = list {
        Arc::make_mut(items).push(item);
    }
}

/// `markdown` open: the document's children gather here, in a cell of the
/// rule's own, which the children's rules share.
pub fn md_begin(rule: &mut Rule, _ctx: &mut Context) {
    rule.node = Rc::new(RefCell::new(Value::array(Vec::new())));
}

/// `markdown` close (`#ZZ`): the document. When the caller passed
/// `meta.md.keepTree`, the native tree, with its references resolved, is
/// parked for [`take_kept_tree`]: the engine-path conformance harness
/// renders it.
pub fn make_md_finish(
    opts: Options,
    inline_tn: Arc<Tabnas>,
) -> impl Fn(&mut Rule, &mut Context) + Send + Sync + 'static {
    move |rule, ctx| {
        if let Some(state) = slot_of(ctx, MD_STATE_KEY).and_then(block_slot_take) {
            if state.keep_tree {
                let tree = parse_inlines_engine(&inline_tn, state.bp.tree, state.bp.refmap, &opts);
                KEPT_TREE.with(|cell| *cell.borrow_mut() = Some(tree));
            }
        }
        let children = rule.node.borrow().clone();
        // A fresh cell, never a write through the inherited one: the
        // engine shares node cells between a rule and its parent.
        rule.node = Rc::new(RefCell::new(object(vec![
            ("type", Value::String("document".to_string())),
            ("children", children),
        ])));
    }
}

/// A container node's open action: its children gather in a cell of its
/// own, and its head waits in `u` for its close.
pub fn md_open(rule: &mut Rule, _ctx: &mut Context) {
    let head = rule.o0().map_or(Value::Undefined, |t| t.val.clone());
    rule.u_mut().insert("head".to_string(), head);
    rule.node = Rc::new(RefCell::new(Value::array(Vec::new())));
}

/// A container node's close action on `#EN`: its node is its head, its
/// children and its tail, in that order, added to its parent's children.
pub fn md_close(rule: &mut Rule, _ctx: &mut Context) {
    let mut node = IndexMap::new();
    if let Some(Value::Object(head)) = rule.u.get("head") {
        for (k, v) in head.iter() {
            node.insert(k.clone(), v.clone());
        }
    }
    node.insert("children".to_string(), rule.node.borrow().clone());
    if let Some(Value::Object(tail)) = rule.c0().map(|t| &t.val) {
        for (k, v) in tail.iter() {
            node.insert(k.clone(), v.clone());
        }
    }
    if let Some(parent) = &rule.parent_node {
        list_push(&mut parent.borrow_mut(), Value::object(node));
    }
}

/// A leaf node's open action: the token's value is the node.
pub fn md_leaf(rule: &mut Rule, _ctx: &mut Context) {
    let value = rule.o0().map_or(Value::Undefined, |t| t.val.clone());
    list_push(&mut rule.node.borrow_mut(), value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_reused() {
        // The leases stand in for the contexts that would hold them.
        let (a, lease_a) = block_slot_insert(
            BlockParser::new(Options::default()),
            false,
            Options::default(),
        );
        let (b, lease_b) = block_slot_insert(
            BlockParser::new(Options::default()),
            false,
            Options::default(),
        );
        assert_ne!(a, b);
        assert!(block_slot_take(a).is_some());
        assert!(block_slot_take(a).is_none());
        let (c, lease_c) = block_slot_insert(
            BlockParser::new(Options::default()),
            true,
            Options::default(),
        );
        assert_eq!(a, c);
        assert!(block_slot_take(b).is_some());
        assert!(block_slot_take(c).is_some_and(|s| s.keep_tree));
        drop((lease_a, lease_b, lease_c));
    }

    /// A slot whose context is gone is freed by the next insert; one
    /// whose context still lives is not.
    #[test]
    fn stale_slots_are_reclaimed_by_the_next_insert() {
        let (a, lease_a) = block_slot_insert(
            BlockParser::new(Options::default()),
            false,
            Options::default(),
        );
        let (b, lease_b) = block_slot_insert(
            BlockParser::new(Options::default()),
            false,
            Options::default(),
        );
        drop(lease_a); // the engine dropped that parse's context
        let (c, lease_c) = block_slot_insert(
            BlockParser::new(Options::default()),
            false,
            Options::default(),
        );
        assert_eq!(c, a, "the stale slot is the one reused");
        assert_ne!(c, b, "the live slot is left alone");
        assert!(block_slot_take(b).is_some());
        assert!(block_slot_take(c).is_some());
        drop((lease_b, lease_c));
    }

    /// Finding 2 of the review: an engine parse that aborts before the
    /// finish action leaves its slot behind, and a worker fed such input
    /// repeatedly must not accumulate them. A parse budget that cancels
    /// after a few iterations is the aborted parse; the slab is read
    /// after each one.
    #[test]
    fn aborted_parses_do_not_accumulate_slots() {
        let mut tn = crate::make();
        tn.parse_budget(1, |ctx| ctx.iteration < 4);
        let src = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let mut held = 0;
        for _ in 0..200 {
            let error = tn.parse(src).expect_err("the budget cancels the parse");
            assert_eq!(error.code, "cancel");
            held = held.max(block_slots_held());
        }
        // At most the parse that just aborted, whose context is gone but
        // which no prepare has run since.
        assert!(held <= 1, "{held} slots held across 200 aborted parses");

        // A parse that runs to completion on this thread frees it too.
        let doc = crate::make()
            .parse("ok")
            .expect("a parse under no budget completes");
        assert!(!doc.is_undefined());
        assert_eq!(block_slots_held(), 0);
    }
}
