/* Copyright (c) 2026 Richard Rodger, MIT License */

//! The engine-facing inline driver: a second engine instance whose lexer
//! IS the inline scanner. Port of `ts/src/engine-inline.ts` and
//! `go/engineinline.go`.
//!
//! Each custom matcher is a thin adapter over one [`InlineParser`] scanner
//! method, the same methods the engine-free path runs, so the anti-drift
//! rule holds: recognition, node building, the delimiter stack and the
//! bracket stack all live once, in `inline.rs`.
//!
//! Two engine features carry the design:
//!
//! * **Context-sensitive matchers.** Every adapter reaches the per-parse
//!   state through `ctx.u["inl"]`. That is what makes the `]` matcher the
//!   cursor owner: `parse_close_bracket` consumes a successful link tail
//!   directly from the subject, and because that happens at *lex* time,
//!   the engine's point advances past the tail and everything after it is
//!   lexed fresh. The corpus-blind straddle cases (a backtick inside a
//!   successful destination or title, followed by a code span; see
//!   test/spec/mixed.tsv) are correct by construction: no token is ever
//!   cut across a tail boundary, so no re-lex or resynchronization
//!   machinery exists at all.
//!
//! * **Matcher order as precedence.** At `<`, the autolink adapter runs
//!   before the raw-HTML adapter, exactly as the hand scanner's dispatch
//!   tries them; the text-run adapter and the single-character literal
//!   fallback come last. The registered order *is* §6.3's precedence
//!   table, visible in the instance's configuration instead of buried in
//!   a `match`.
//!
//! The one-token effects are applied at lex time on purpose. The engine's
//! rule loop reads lookahead tokens before earlier alts' actions run, so
//! any state a matcher consults (the tree for break trimming, the bracket
//! stack for `]`) must be current when the *lexer* reaches it, not when
//! the parser does. The `inline` rule therefore carries no per-token
//! actions: the tokens are the observable record of the scan (subscribe
//! with `Tabnas::subscribe_tokens` and watch), and the rule's close action
//! finalizes the block exactly as the engine-free `parse` tail does.
//!
//! # One instance per install
//!
//! TypeScript builds one inline instance per plugin install; Go builds one
//! per parse because its engine's concurrency guarantee covers
//! construction only. The Rust engine parses through `&self` and the
//! instance is `Send + Sync`, so this port follows TypeScript: one
//! instance, built when the plugin installs, shared by every parse. The
//! per-parse state it needs (the scanner, the tree, the block) travels
//! through a thread-local slab exactly as the block phase's does, keyed by
//! a slot index in the parse meta.

use std::cell::RefCell;
use std::sync::Arc;

use indexmap::IndexMap;
use tabnas::{AltSpec, Context, Lexer, Rule, Tabnas, Tin, Token, Value, TIN_ZZ};

use crate::engine_block::{slot_of, value_get};
use crate::inline::{inline_content_blocks, linkify_autolinks, InlineParser};
use crate::node::{NodeId, Tree};
use crate::options::{Options, RefMap};

/// The inline token alphabet, in the order the rule's alt lists them.
pub const INLINE_TOKENS: [&str; 12] = [
    "#IBK", // line break (soft or hard, decided against the tree)
    "#IES", // backslash escape
    "#ICS", // code span (or an unmatched literal backtick run)
    "#IDL", // emphasis/strikethrough delimiter run
    "#IOB", // [
    "#IBG", // ![
    "#ICB", // ] (including a consumed inline link tail)
    "#IAL", // angle autolink
    "#IHT", // raw HTML tag
    "#IEN", // entity reference
    "#ITX", // ordinary text run
    "#ILI", // single literal character nothing else claimed
];

/// The key under `ctx.u` (and under `meta.md`) that carries the inline
/// state's slot index.
pub const INLINE_STATE_KEY: &str = "inl";

/// The `parse.prepare` hook's name.
pub const INLINE_PREPARE: &str = "inl-reset";

// --- the per-parse state ----------------------------------------------------

/// What `ctx.u["inl"]` points at while one block's inlines are being
/// scanned: the scanner, the tree it appends to, and the block.
struct InlineState {
    parser: InlineParser,
    tree: Tree,
    block: NodeId,
}

thread_local! {
    static INLINE_STATES: RefCell<Vec<Option<InlineState>>> = const { RefCell::new(Vec::new()) };
}

fn inline_slot_insert(state: InlineState) -> usize {
    INLINE_STATES.with(|cell| {
        let mut slab = cell.borrow_mut();
        match slab.iter().position(Option::is_none) {
            Some(i) => {
                slab[i] = Some(state);
                i
            }
            None => {
                slab.push(Some(state));
                slab.len() - 1
            }
        }
    })
}

fn inline_slot_take(slot: usize) -> Option<InlineState> {
    INLINE_STATES.with(|cell| cell.borrow_mut().get_mut(slot).and_then(Option::take))
}

fn with_inline_state<R>(ctx: &Context, f: impl FnOnce(&mut InlineState) -> R) -> Option<R> {
    let slot = slot_of(ctx, INLINE_STATE_KEY)?;
    INLINE_STATES.with(|cell| {
        let mut slab = cell.borrow_mut();
        slab.get_mut(slot).and_then(Option::as_mut).map(f)
    })
}

// --- the adapters -----------------------------------------------------------

/// One scanner method, taking the tree and the block it appends to.
type Run = fn(&mut InlineParser, &mut Tree, NodeId) -> bool;

/// The token a successful run is reported as. `!` is the one whose name
/// depends on what was consumed (`![` versus a literal `!`).
type Name = Box<dyn Fn(&InlineParser, usize) -> (&'static str, Tin) + Send + Sync>;

/// Adapt one [`InlineParser`] scanner method into a lexer matcher: sync
/// the parser to the engine's point, run the method (which appends nodes
/// and moves `pos`), and emit a token covering exactly what it consumed.
///
/// Row and column bookkeeping is observability data: a matcher that
/// consumed line endings (soft breaks, multiline code spans or titles)
/// moved the scan onto a later row, and later tokens must say so. The
/// lexer's own `advance_chars` does that arithmetic, in CHARACTERS;
/// `tests/engine_columns_test.rs` pins the positions.
fn adapt(
    guard: fn(u8) -> bool,
    run: Run,
    name: Name,
) -> impl for<'s> Fn(&mut Lexer<'s>, &mut Rule, &mut Context) -> Option<Token> + Send + Sync + 'static
{
    move |lexer, _rule, ctx| {
        let point = lexer.point();
        let src = lexer.source();
        let start = point.site.si;
        if start >= src.len() || !guard(src.as_bytes()[start]) {
            return None;
        }

        let (token_name, tin, end) = with_inline_state(ctx, |st| {
            st.parser.pos = start;
            if !run(&mut st.parser, &mut st.tree, st.block) {
                return None;
            }
            let (token_name, tin) = name(&st.parser, start);
            Some((token_name, tin, st.parser.pos))
        })??;

        let consumed = &src[start..end];
        let consumed_chars = consumed.chars().count();
        let token = lexer.token(token_name, tin, Value::Undefined, consumed, point);
        lexer.advance_chars(consumed_chars);
        Some(token)
    }
}

fn fixed(name: &'static str, tin: Tin) -> Name {
    Box::new(move |_, _| (name, tin))
}

fn imperative(
    name: &str,
    order: f64,
    matcher: impl for<'s> Fn(&mut Lexer<'s>, &mut Rule, &mut Context) -> Option<Token>
        + Send
        + Sync
        + 'static,
) -> tabnas::LexMatcher {
    tabnas::LexMatcher {
        name: name.to_string(),
        order,
        matcher: None,
        imperative: Some(Arc::new(matcher)),
        factory: None,
    }
}

/// The `parse.prepare` hook: the slot index arrives on the parse meta as
/// `meta.md.inl`, and is copied to `ctx.u["inl"]` where the adapters and
/// the close action read it.
fn inline_prepare(ctx: &mut Context) {
    let slot = value_get(&ctx.meta, "md")
        .and_then(|md| value_get(md, INLINE_STATE_KEY))
        .cloned();
    if let Some(slot @ Value::Number(_)) = slot {
        ctx.u.insert(INLINE_STATE_KEY.to_string(), slot);
    }
}

/// The `inline` rule's close action on `#ZZ`: resolve emphasis over the
/// whole delimiter stack and clear the block's raw content, exactly as
/// the engine-free `parse` tail does.
fn inline_finish_action(_rule: &mut Rule, ctx: &mut Context) {
    with_inline_state(ctx, |st| {
        let block = st.block;
        st.parser.finish_block(&mut st.tree, block);
    });
}

// --- the instance -----------------------------------------------------------

/// Build the inline engine instance for one option set. All built-in
/// matchers are disabled; the registered matcher set is the complete
/// lexical description of the inline phase, in precedence order.
///
/// GFM strikethrough is a separate matcher registered only when the
/// option is on: the option reshapes the registered matcher set itself,
/// so the base dialect's lexer simply has no tilde matcher rather than a
/// disabled branch inside one.
pub fn make_inline_tn(opts: &Options) -> Tabnas {
    let mut tn = Tabnas::new();

    let mut tins: IndexMap<&'static str, Tin> = IndexMap::with_capacity(INLINE_TOKENS.len());
    for name in INLINE_TOKENS {
        tins.insert(name, tn.token(name));
    }
    let tin = |name: &str| tins[name];

    tn.set_options(|o| {
        o.fixed.lex = false;
        o.space.lex = false;
        o.line.lex = false;
        o.text.lex = false;
        o.string.lex = false;
        o.comment.lex = false;
        o.number.lex = false;
        o.value.lex = false;
        o.rule.start = "inline".to_string();
    })
    .expect("the inline engine's option set is fixed and valid");

    let ibg = tin("#IBG");
    let ili = tin("#ILI");
    let bang_name: Name = Box::new(move |p: &InlineParser, start: usize| {
        if p.pos - start == 2 {
            ("#IBG", ibg)
        } else {
            ("#ILI", ili)
        }
    });

    let mut matchers = vec![
        imperative(
            "inlBreak",
            100_000.0,
            adapt(
                |c| c == b'\n',
                InlineParser::parse_newline,
                fixed("#IBK", tin("#IBK")),
            ),
        ),
        imperative(
            "inlEscape",
            100_100.0,
            adapt(
                |c| c == b'\\',
                InlineParser::parse_backslash,
                fixed("#IES", tin("#IES")),
            ),
        ),
        imperative(
            "inlCode",
            100_200.0,
            adapt(
                |c| c == b'`',
                InlineParser::parse_backticks,
                fixed("#ICS", tin("#ICS")),
            ),
        ),
        imperative(
            "inlDelim",
            100_300.0,
            adapt(
                |c| c == b'*' || c == b'_',
                |p, tree, block| {
                    let cc = p.subject.as_bytes()[p.pos];
                    p.handle_delim(cc, tree, block)
                },
                fixed("#IDL", tin("#IDL")),
            ),
        ),
        imperative(
            "inlOpenBracket",
            100_400.0,
            adapt(
                |c| c == b'[',
                InlineParser::parse_open_bracket,
                fixed("#IOB", tin("#IOB")),
            ),
        ),
        // `!` introduces an image only before `[`; otherwise a literal.
        imperative(
            "inlBang",
            100_500.0,
            adapt(|c| c == b'!', InlineParser::parse_bang, bang_name),
        ),
        // The cursor owner: on a successful inline link tail,
        // parse_close_bracket has already consumed it when this token is
        // emitted.
        imperative(
            "inlCloseBracket",
            100_600.0,
            adapt(
                |c| c == b']',
                InlineParser::parse_close_bracket,
                fixed("#ICB", tin("#ICB")),
            ),
        ),
        // At `<`: autolink first, raw HTML second. Matcher order is
        // precedence.
        imperative(
            "inlAutolink",
            100_700.0,
            adapt(
                |c| c == b'<',
                InlineParser::parse_autolink,
                fixed("#IAL", tin("#IAL")),
            ),
        ),
        imperative(
            "inlHtmlTag",
            100_800.0,
            adapt(
                |c| c == b'<',
                InlineParser::parse_html_tag,
                fixed("#IHT", tin("#IHT")),
            ),
        ),
        imperative(
            "inlEntity",
            100_900.0,
            adapt(
                |c| c == b'&',
                InlineParser::parse_entity,
                fixed("#IEN", tin("#IEN")),
            ),
        ),
        imperative(
            "inlText",
            101_000.0,
            adapt(
                |_| true,
                InlineParser::parse_string,
                fixed("#ITX", tin("#ITX")),
            ),
        ),
        // Nothing claimed the character: one literal char, same as the
        // hand scanner's dispatch fallback.
        imperative(
            "inlLiteral",
            101_100.0,
            adapt(
                |_| true,
                InlineParser::parse_literal_char,
                fixed("#ILI", tin("#ILI")),
            ),
        ),
    ];

    if opts.gfm {
        matchers.push(imperative(
            "inlTilde",
            100_350.0,
            adapt(
                |c| c == b'~',
                |p, tree, block| p.handle_delim(b'~', tree, block),
                fixed("#IDL", tin("#IDL")),
            ),
        ));
    }

    // The lexer runs custom matchers in registration order and stops at
    // the first whose order is past the band, so they are registered
    // sorted by order: `inlTilde` slots in between the delimiter and the
    // open bracket matchers.
    matchers.sort_by(|a, b| a.order.total_cmp(&b.order));
    for matcher in matchers {
        tn.options
            .lex
            .matchers
            .insert(matcher.name.clone(), matcher);
    }

    tn.options.parse.named_prepare.insert(
        INLINE_PREPARE.to_string(),
        tabnas::ParsePrepare::Context(Arc::new(inline_prepare)),
    );

    // One OR-position over the whole alphabet: any inline token loops the
    // rule; the empty alts fall through to the close on #ZZ.
    let or_set: Vec<Tin> = INLINE_TOKENS.iter().map(|name| tin(name)).collect();

    tn.define_rule("inline", |rs| {
        rs.clear();
        rs.add_open(AltSpec {
            s: vec![or_set],
            r: Some("inline".to_string()),
            ..Default::default()
        });
        rs.add_open(AltSpec::default());

        let mut finish = AltSpec {
            s: vec![vec![TIN_ZZ]],
            ..Default::default()
        };
        finish.add_action(inline_finish_action);
        rs.add_close(finish);
        rs.add_close(AltSpec::default());
    });

    tn
}

/// The engine-path inline phase: the same block walk as `parse_inlines`,
/// with each block's scan driven by the engine instance's lexer. An empty
/// subject never reaches the engine (its empty-source path returns before
/// the rules run), and finishes the block directly: a no-op resolve,
/// exactly as the hand scanner's `parse` does.
///
/// Takes and returns the tree by value: while a block is being scanned
/// the tree belongs to the per-parse slot the adapters reach through the
/// context, and it comes back when the engine returns.
pub fn parse_inlines_engine(
    inline_tn: &Tabnas,
    mut tree: Tree,
    refmap: RefMap,
    options: &Options,
) -> Tree {
    let mut parser = InlineParser::new(refmap, *options);

    for block in inline_content_blocks(&tree) {
        if !parser.begin_block(&tree, block) {
            parser.finish_block(&mut tree, block);
        } else {
            let subject = parser.subject.clone();
            let slot = inline_slot_insert(InlineState {
                parser,
                tree,
                block,
            });

            let mut md = IndexMap::with_capacity(1);
            md.insert(INLINE_STATE_KEY.to_string(), Value::Number(slot as f64));
            let mut meta = IndexMap::with_capacity(1);
            meta.insert("md".to_string(), Value::object(md));

            let result = inline_tn.parse_with_meta(&subject, Value::object(meta));

            let state = inline_slot_take(slot)
                .expect("the inline state slot is released only here, after its parse");
            parser = state.parser;
            tree = state.tree;

            if let Err(error) = result {
                // CommonMark defines an output for every input; an engine
                // error here is an internal wiring defect, not a parse
                // result.
                panic!("markdown: the inline engine rejected a block's content: {error}");
            }
        }
        if options.gfm {
            linkify_autolinks(&mut tree, block);
        }
    }

    tree
}
