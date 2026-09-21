/* Copyright (c) 2026 Richard Rodger, MIT License */

//! The engine-facing block driver: a custom lexer matcher that turns the
//! engine's scan into one `#LB` token per physical line, and the actions
//! that feed those lines to the shared [`BlockParser`]. Port of
//! `ts/src/engine-block.ts` and `go/engineblock.go`.
//!
//! This is the seam of the anti-drift rule (dx-report §42): nothing here
//! recognizes anything. Line cutting and NUL replacement come from
//! [`segment_next_line`], blank detection from [`is_blank`], and every
//! block decision (continuation, starts, lazy continuation, finalization)
//! from the same [`BlockParser`] the engine-free path runs. The engine
//! owns tokenization, dispatch, per-parse state carriage and
//! observability; the algorithm stays in the shared core.
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
use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;
use tabnas::{Context, Lexer, Rule, Tin, Token, Value};

use crate::ast::to_ast;
use crate::block::{is_blank, segment_next_line, BlockParser};
use crate::node::Tree;
use crate::options::{Options, RefMap};

/// The block phase's one token: a physical line.
pub const LINE_TOKEN: &str = "#LB";

/// The `mdLine` matcher's name in `options.lex.matchers`.
pub const MD_LINE_MATCHER: &str = "mdLine";

/// The `mdLine` matcher's order: ahead of every built-in, so a line is
/// consumed whole before any built-in matcher sees its first character.
pub const MD_LINE_ORDER: f64 = 100_000.0;

/// The `parse.prepare` hook's name.
pub const MD_PREPARE: &str = "md-reset";

/// The key under `ctx.u` that carries the block state's slot index, and
/// the key under a `#LB` token's `use_data` that carries its [`LineInfo`].
pub const MD_STATE_KEY: &str = "md";

/// The `#LB` token's payload: one physical line, pre-classified. A
/// downstream dialect's alt conditions can read it back from the token
/// with [`LineInfo::from_token`] (see the extension recipe in
/// `doc/guide.md`).
///
/// On the token, the text is the token's `val` and the two flags sit
/// under `use_data["md"]`, so a line costs one string, not two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineInfo<'a> {
    /// The line's text, terminator excluded, NULs already replaced.
    pub text: &'a str,
    /// True when the line holds nothing but spaces and tabs.
    pub blank: bool,
    /// GFM table arming: true when the line contains a `|` or a `-`
    /// anywhere. A deliberate, *provable* superset of "could be a
    /// delimiter row": every delimiter cell requires at least one `-`
    /// (`:-:` rows open pipe-less single-column tables), and multi-column
    /// rows carry `|`. The real decision needs the container-adjusted
    /// offset only the block algorithm knows (a delimiter row nested in a
    /// block quote matches after the `>` marker), so the `gfm`-tagged alt
    /// reading this bit only *arms* the probe, and `try_open_table`
    /// re-verifies (see test/spec/mixed.tsv).
    pub tbl_arm: bool,
}

impl<'a> LineInfo<'a> {
    /// Classify one line of text.
    pub fn of(text: &'a str) -> LineInfo<'a> {
        LineInfo {
            text,
            blank: is_blank(text),
            tbl_arm: text.contains('|') || text.contains('-'),
        }
    }

    /// Read the payload back off a `#LB` token. `None` for any other
    /// token.
    pub fn from_token(token: &'a Token) -> Option<LineInfo<'a>> {
        let text = match &token.val {
            Value::String(s) => s.as_str(),
            Value::Text(t) => t.string.as_str(),
            _ => return None,
        };
        let flags = token.use_data().get(MD_STATE_KEY)?;
        let blank = matches!(value_get(flags, "blank"), Some(Value::Bool(true)));
        let tbl_arm = matches!(value_get(flags, "tblArm"), Some(Value::Bool(true)));
        Some(LineInfo {
            text,
            blank,
            tbl_arm,
        })
    }

    fn flags_value(&self) -> Value {
        let mut map = IndexMap::with_capacity(2);
        map.insert("blank".to_string(), Value::Bool(self.blank));
        map.insert("tblArm".to_string(), Value::Bool(self.tbl_arm));
        Value::object(map)
    }
}

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

/// What `ctx.u["md"]` points at: the live block parser and the keepTree
/// request read off the parse meta.
struct BlockState {
    bp: BlockParser,
    keep_tree: bool,
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
fn block_slot_insert(bp: BlockParser, keep_tree: bool) -> (usize, Value) {
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

/// Build the `mdLine` matcher: each invocation consumes one physical line,
/// terminator included, and emits a single `#LB` token carrying the line
/// as a [`LineInfo`]. The matcher owns the engine's point: the byte offset
/// advances past the terminator, the row counts line endings, and the
/// column resets per line. (The engine's column is not used for
/// indentation: the shared [`BlockParser`] computes spec §2.2
/// tab-expanded columns itself.)
///
/// Row and column bookkeeping is observability data (`#ZZ` position,
/// traces) and is left to the lexer's own `advance_chars`, which counts
/// CHARACTERS and resets the column on a consumed `\n`. That is the
/// arithmetic the Go port spells out by hand; `tests/engine_columns_test.rs`
/// pins the resulting positions.
pub fn make_md_line_matcher(
    lb: Tin,
) -> impl for<'s> Fn(&mut Lexer<'s>, &mut Rule, &mut Context) -> Option<Token> + Send + Sync + 'static
{
    move |lexer, _rule, _ctx| {
        let point = lexer.point();
        let src = lexer.source();
        let seg = segment_next_line(src, point.site.si)?;
        let raw = &src[point.site.si..seg.next];
        let raw_chars = raw.chars().count();
        let flags = LineInfo::of(&seg.text).flags_value();
        let mut token = lexer.token(LINE_TOKEN, lb, Value::String(seg.text), raw, point);
        token.use_data_mut().insert(MD_STATE_KEY.to_string(), flags);
        lexer.advance_chars(raw_chars);
        Some(token)
    }
}

// --- the actions ------------------------------------------------------------

/// The `parse.prepare` hook: fresh block-parser state for every parse,
/// with the keepTree request captured from the parse meta.
///
/// An empty source is skipped on purpose. The engine runs this hook
/// before it short-circuits `""` through `lex.empty_result`, so the finish
/// action that would release the slot never runs for one.
pub fn make_md_prepare(opts: Options) -> impl Fn(&mut Context) + Send + Sync + 'static {
    move |ctx| {
        if ctx.source.is_empty() {
            return;
        }
        let (_, lease) = block_slot_insert(BlockParser::new(opts), keep_tree_requested(&ctx.meta));
        ctx.u.insert(MD_STATE_KEY.to_string(), lease);
    }
}

/// The `line` GFM open alt's condition (tagged `g: "gfm"`): the lookahead
/// `#LB` token's `tblArm` bit.
pub fn md_line_gfm_condition(_rule: &mut Rule, ctx: &mut Context) -> bool {
    ctx.t0()
        .and_then(LineInfo::from_token)
        .is_some_and(|info| info.tbl_arm)
}

fn feed_line(rule: &Rule, ctx: &Context, table_armed: bool) {
    let Some(info) = rule.o0().and_then(LineInfo::from_token) else {
        return;
    };
    with_block_state(ctx, |state| {
        state.bp.table_armed = table_armed;
        state.bp.incorporate_line(info.text);
    });
}

/// The `line` base open alt's action: disarm the table probe and feed the
/// matched `#LB` line to the shared algorithm. Reaching this alt means
/// the `gfm`-tagged alt ahead of it did not match (the line cannot
/// contain a delimiter row), or GFM is off entirely (`rule.exclude`
/// removed the tagged alt, and `try_open_table` is closed by the option
/// anyway).
pub fn md_line_action(rule: &mut Rule, ctx: &mut Context) {
    feed_line(rule, ctx, false);
}

/// The `line` GFM open alt's action (tagged `g: "gfm"`, condition on the
/// token's `tblArm` bit): arm the table probe for this line, then feed it
/// to the same shared algorithm. This is the extension seam a downstream
/// dialect would use: an alt injected ahead of the base one, gated on a
/// group tag so `gfm: false` can subtract it wholesale.
pub fn md_line_gfm_action(rule: &mut Rule, ctx: &mut Context) {
    feed_line(rule, ctx, true);
}

/// The `markdown` close alt's action on `#ZZ`: finalize blocks, run the
/// phase-2 driver, and project the public AST into the rule's node.
/// `run_inlines` is the engine inline path from `engine_inline.rs`,
/// injected so this module stays a block-phase concern. When the caller
/// passed `meta.md.keepTree`, the native tree is parked for
/// [`take_kept_tree`]: the engine-path conformance harness renders it.
pub fn make_md_finish_action(
    opts: Options,
    run_inlines: impl Fn(Tree, RefMap) -> Tree + Send + Sync + 'static,
) -> impl Fn(&mut Rule, &mut Context) + Send + Sync + 'static {
    move |rule, ctx| {
        let Some(state) = slot_of(ctx, MD_STATE_KEY).and_then(block_slot_take) else {
            return;
        };
        let (tree, refmap) = state.bp.finish();
        let tree = run_inlines(tree, refmap);

        let ast = to_ast(&tree, &opts);
        if state.keep_tree {
            KEPT_TREE.with(|cell| *cell.borrow_mut() = Some(tree));
        }

        // A fresh cell, never a write through the inherited one: the
        // engine shares node cells between a rule and its parent.
        rule.node = Rc::new(RefCell::new(ast));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_info_flags() {
        let info = LineInfo::of("a | b");
        assert!(!info.blank);
        assert!(info.tbl_arm);
        let info = LineInfo::of(" \t");
        assert!(info.blank);
        assert!(!info.tbl_arm);
        let info = LineInfo::of(":-:");
        assert!(info.tbl_arm);
    }

    #[test]
    fn slots_are_reused() {
        // The leases stand in for the contexts that would hold them.
        let (a, lease_a) = block_slot_insert(BlockParser::new(Options::default()), false);
        let (b, lease_b) = block_slot_insert(BlockParser::new(Options::default()), false);
        assert_ne!(a, b);
        assert!(block_slot_take(a).is_some());
        assert!(block_slot_take(a).is_none());
        let (c, lease_c) = block_slot_insert(BlockParser::new(Options::default()), true);
        assert_eq!(a, c);
        assert!(block_slot_take(b).is_some());
        assert!(block_slot_take(c).is_some_and(|s| s.keep_tree));
        drop((lease_a, lease_b, lease_c));
    }

    /// A slot whose context is gone is freed by the next insert; one
    /// whose context still lives is not.
    #[test]
    fn stale_slots_are_reclaimed_by_the_next_insert() {
        let (a, lease_a) = block_slot_insert(BlockParser::new(Options::default()), false);
        let (b, lease_b) = block_slot_insert(BlockParser::new(Options::default()), false);
        drop(lease_a); // the engine dropped that parse's context
        let (c, lease_c) = block_slot_insert(BlockParser::new(Options::default()), false);
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
