# Agents Guide: rs/

The Rust port of the canonical TypeScript in [`../ts`](../ts). Read
[`../AGENTS.md`](../AGENTS.md) first: it holds the cross-runtime rules
(extension placement, the layering rule, the release procedure, the
untrusted-input rule), and this file covers only what is specific to
this crate.

## Layout

| Path | |
|---|---|
| `src/lib.rs` | plugin wiring and the public surface (the `markdown.ts` role): `VERSION`, `markdown()`, `plugin()`, `make()`, `make_with()`, `parse()`, `parse_keep_tree()`, and the engine-free `parse_document()` / `to_html()` / `parse_tree()` / `parse_inline()`; also the `#[cfg(doctest)]` include of `README.md` |
| `src/commonmark.rs` | engine-free entry: block phase, then inline phase |
| `src/block.rs`, `src/inline.rs`, `src/ast.rs`, `src/html.rs`, `src/node.rs`, `src/common.rs`, `src/options.rs` | the same split as `go/`, name for name |
| `src/entities.rs` | GENERATED from `ts/src/entities.ts` (2125 entries, sorted for binary search); `tests/entities_test.rs` pins it entry for entry |
| `src/engine_block.rs` | the `mdLine` matcher, the `parse.prepare` hook, the `line` rule actions, the finish action, and the thread-local block-state slab |
| `src/engine_inline.rs` | the inline engine instance: twelve matchers over the shared `InlineParser`, plus `inlTilde` under `gfm`, one instance per install |
| `tests/parity_test.rs` | every `../test/spec/*.tsv` through `tabnas_support::Runner`, a fresh `make_with(&opts)` per row; no file is exempt |
| `tests/tree_golden_test.rs` | `../test/spec/tree/*.json`, the native tree with `sourcepos`; fails when a fixture file has no golden |
| `tests/commonmark_test.rs`, `tests/gfm_test.rs` | the two HTML-corpus graders (engine-free), `go/commonmark_test.go` and `go/gfm_test.go` ported, with the option matrix and the hand-mirrored GFM behaviour tests |
| `tests/engine_conformance_test.rs` | both corpora again, through the engine (`make_with` + `parse_keep_tree`) |
| `tests/differential_test.rs` | plugin path against engine-free path: corpora, fixtures, and the seeded fuzz shared with TS and Go |
| `tests/markdown_test.rs` | the plugin surface end to end, and the shared default instance across threads |
| `tests/recognizer_test.rs`, `tests/entities_test.rs`, `tests/robust_test.rs`, `tests/engine_columns_test.rs`, `tests/engine_inline_test.rs`, `tests/perf_test.rs`, `tests/version_test.rs` | the Go test files of the same names, ported |
| `tests/layering_test.rs` | the layering rule below, asserted over `src/*.rs` with comments stripped |
| `tests/common/mod.rs` | the corpus loaders (which FAIL on a missing or short corpus), the `to_json` number normaliser, the fuzz generator |
| `README.md` | the crate front page, doctested |

## The graders fail loudly

`load_spec_cases` / `load_gfm_cases` in `tests/common/mod.rs` panic when
`test/commonmark/spec.json` or `test/gfm/spec.json` is missing,
unparsable, or does not hold exactly 652 / 24 examples. There is no skip
path and must never be one: a grader that reports green having run
nothing is worse than a red one. The same holds for the fixtures
(`tabnas_support::Runner::dir` fails on an empty spec directory or an
empty file) and the goldens (`every_fixture_has_a_golden` fails on a
fixture file without one).

## The layering rule, precisely

Only `lib.rs`, `engine_block.rs` and `engine_inline.rs` use engine
BEHAVIOUR (`Tabnas`, `Context`, `Lexer`, `Rule`, `Token`, `AltSpec`,
`Plugin`). `ast.rs` and `options.rs` name `tabnas::Value`, because the
AST IS an engine value and the plugin option bag is one; that is a type,
not the engine. Nothing reachable from `commonmark.rs` calls into the
engine, so the corpora are graded by the engine-free functions alone,
and `differential_test.rs` plus `engine_conformance_test.rs` hold the
plugin path to the same output.

That rule is asserted, not eyeballed: `tests/layering_test.rs` strips
comments from every `src/*.rs` and classifies what is left, so a file
outside the five named above may name no engine item at all, and the
classification itself fails when an entry goes stale. Do not reach for a
grep instead. Comments are prose about the engine, not use of it, and a
grep counts them: `grep -l "Tabnas\|Context\|Lexer" src/*.rs` names
`options.rs` for a doc comment about `Tabnas::use_plugin`, and a grep for
`tabnas::` adds `block.rs` and `inline.rs` for the comments explaining
the nesting caps. The test reads code alone, which is where a real
breach would land.

## State carriage is a thread-local slab

The other runtimes hang the live `BlockParser` on `ctx.u.md` as an
opaque object. `Context::u` here holds only `Value`s, so the parser
lives in a `thread_local!` slab and `ctx.u["md"]` carries a slot index;
the inline phase does the same under `"inl"`. The slot is claimed by the
`parse.prepare` hook and released by the `markdown` rule's finish action
(the last thing a parse runs). An aborted parse leaves its slot occupied
until the next parse on that thread reuses it (prepare takes the lowest
free slot), so nothing accumulates. A parse never leaves the thread it
started on, which is why thread-local storage is sound with a
`Send + Sync` instance serving many threads.

`parse_keep_tree` is the `keepTree` handshake: `meta.md.keepTree = true`
makes the finish action stash the native tree in a thread-local, and the
caller takes it back; it clears any stale tree first. Use it in tests
that need the tree the ENGINE built. Production callers use
`parse_tree`.

## One inline instance per install

TypeScript builds one inline engine instance per plugin install; Go
builds one per parse because its engine's concurrency guarantee covers
construction only. Rust follows TypeScript: `make_inline_tn` runs once in
`markdown()` and the `Arc` is captured by the finish action.
`tests/engine_inline_test.rs` pins the alphabet (twelve tokens, the same
list `ts/test/engine-inline.test.ts` and `go/engineinline_test.go` pin),
that the `inline` rule never looks ahead more than one token (the
matchers apply one-token effects at LEX time, which is only sound with
`s.len() <= 1`), and that the tilde matcher exists only under `gfm`.

The token count and the matcher count are not the same number, and the
test pins both. Twelve matchers serve the base dialect and `inlTilde` is
a thirteenth under `gfm`, so the registered set is 13 by default and 12
with the option off. The alphabet stays at twelve either way, because
`inlTilde` emits `#IDL`, the delimiter-run token the `*` and `_` matcher
already emits, rather than one of its own. Go and TypeScript pin the
alphabet alone; the matcher counts are a Rust-side addition.

## `gfm` is subtracted, not branched

The GFM alt on the `line` rule carries `g: "gfm"` and `markdown()` sets
`rule.exclude = "gfm"` when the option is off, so the base dialect is the
grammar minus the tagged alts. The block algorithm still reads
`opts.gfm` for the table and task-list decisions and the renderer reads
it for the tag filter; the tag is the ENGINE-visible seam.
`tests/engine_inline_test.rs` and `tests/gfm_test.rs::gfm_disabled` pin
both sides.

## Numbers are `f64`

`tabnas::Value::Number` is an `f64`, so `to_json()` renders `depth: 1`
as `1.0`. `tests/common/mod.rs::to_json` normalises integral floats to
integers before comparing against Go-shaped expectations, and the
fixture runner compares numerically. Do not "fix" this in the
projection: it is the engine's value type, and every other Rust port
has the same shape.

## Two things TypeScript has that this crate does not

Neither is a parity gap, and neither belongs in `../DIVERGENCE.md`: no
input parses differently because of them. Both are recorded so the
absence reads as a decision rather than an oversight.

**Token descriptions.** `ts/src/markdown.ts` and
`ts/src/engine-inline.ts` register a `config.modify` hook that hangs a
human description on `#LB` and on the inline tokens, which
`@tabnas/railroad` reads off a live instance to label the diagrams in
`ts/doc/`. There is no counterpart here, and none is available: the Rust
engine has a `config_modify` hook but no `token_desc` field for one to
write to, and neither has the Go engine. Porting it is an engine change
in `tabnas/parser`, for a TypeScript documentation tool, so it is left
where it is. Go is in the same position, for the same reason.

**A debug composition test.** `ts/test/debug-model.test.ts` layers
`@tabnas/debug` over the plugin and inspects the serialized model.
`tabnas-debug` does have a Rust crate with a `model()`, so the test is
writable, but taking it would add a third sibling path dependency, and
`../ci/rust/run.sh` checks for exactly two siblings and exempts exactly
two crates from its lockfile comparison. Go carries no such test either.
Adding it means widening that gate first, deliberately, rather than as a
side effect of a test.

## The divergences

Token columns after an astral character: TypeScript counts UTF-16
units, Go and Rust count characters. It is recorded in
`parser/DIVERGENCE.md` ("Column positions for astral characters");
`tests/engine_columns_test.rs` cites it and pins the Rust answer on the
same table `go/enginecol_test.go` uses. It never reaches the AST or the
native tree (the goldens pass).

The Rust port adds one of its own, the nesting cap described below.
Both are written up in [`../DIVERGENCE.md`](../DIVERGENCE.md). There is
still no `test/spec/divergent.tsv` register: a shared fixture row states
one expected value per input, and neither divergence can be spelled that
way (one is invisible to the AST, the other needs a document larger than
a fixture cell). A divergence that a row CAN express belongs in a
register, with a `rust` column, per [`../AGENTS.md`](../AGENTS.md).

Without that file, the per-runtime tests ARE the register, so all three
columns have to be asserted or the record is prose again. The nesting
cap is pinned here by `tests/robust_test.rs::nesting_is_capped_at_the_constants`
and, on the uncapped side, by `../ts/test/divergence.test.ts` and
`../go/robust_test.go::TestNestingIsUncapped`. Changing either constant
means updating all three and the table in `../DIVERGENCE.md`; removing
the caps means deleting all three.

## Deep nesting: the crate is off the stack, the AST type is not

`block.rs`, `inline.rs`, `html.rs` and `ast.rs` never recurse on the
document's nesting, and `tests/robust_test.rs::deep_nesting_stays_off_the_stack`
pins 8000 levels of input on a 2 MB test thread. What still recurses is
outside this crate: `tabnas::Value::to_json()` and the default drop of a
`tabnas::Value` (`parser/rs/src/value.rs`), which walk the projected
tree once per level. Measured in a debug build on a 2 MB thread, the
drop survives about 5,300 levels and `to_json()` about 1,250, and each
container costs two levels.

Two constants keep the projected AST inside that headroom:
`block::MAX_CONTAINER_NESTING` (100 block quotes, lists and list items)
and `inline::MAX_INLINE_NESTING` (50 emphasis, link and image
wrappers). Markers past either bound stay literal text, which is what
`tests/robust_test.rs` pins. The canonical TypeScript has no such cap,
so this is a divergence and it is recorded in `../DIVERGENCE.md`, with
the reasoning here. Raising a constant means re-measuring both walks
first; removing them needs the engine to stop recursing, which is
`parser/rs`'s to fix, not this crate's.

## The README is doctested

`src/lib.rs` includes `README.md` under `#[cfg(doctest)]`, so every
```` ```rust ```` fence in it is compiled and run by `cargo test --doc`,
which `--all-targets` does NOT include (`make test-rs` and
`ci/rust/run.sh` run both). Keep each fence a complete
`fn main() -> Result<(), Box<dyn std::error::Error>>` program with no
hidden `# ` lines: rustdoc runs it as written and GitHub renders it as
written. `cargo test --doc` must list one `readme_examples (line N)`
entry per fence.

## Prose rules

`README.md` is published (the Rust hub beside `../ts/README.md` and
`../go/README.md`) and follows [`../docs/STYLE-GUIDE.md`](../docs/STYLE-GUIDE.md):
no em dashes in prose, no first person, British spelling, the banned
list in `.vale/styles/config/vocabularies/Tabnas/reject.txt`, and no
link from it to any `AGENTS.md`.

It is listed in `ts/scripts/gated-docs.cjs`, so both halves of the gate
already cover it: `ts/test/docs.test.js` runs inside `npm test` from
`ts/`, and Vale runs over the same list in
`.github/workflows/docs.yml`. An earlier revision of this file said the
page was not yet listed and asked for a hand check against `reject.txt`
instead. Editing `README.md` and running only the Rust gate will not
report a prose failure, because no Rust target reads the page as prose;
run the TypeScript suite for that.

This file is not gated. Keep it inside the same rules anyway, so moving
a paragraph from here to a published page does not fail the gate.

## Running it

```bash
cd rs
cargo build --all-targets
cargo test --all-targets          # 16 binaries: fixtures, goldens, both graders twice, the rest
cargo test --doc                  # crate docs plus the README fences
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test commonmark_test -- --nocapture     # the 652/652 table
cargo test --test gfm_test gfm_spec -- --nocapture   # the 24/24 table
```

`make test-rs` from the root is tests, doctests and clippy;
`ci/rust/run.sh` is the full gate CI would run (fmt, build, tests,
doctests, clippy, the MSRV pin when that toolchain is installed, and a
lockfile diff exempting the two siblings' versions), and it leaves the
tree as it found it. `parser` and `support` must be checked out as
siblings of this repo. On a shared box use `CARGO_BUILD_JOBS=2` and do
not run two cargo commands at once.

Version sites: `Cargo.toml`, `src/lib.rs::VERSION`, and the crate's own
entry in `Cargo.lock`, all equal to `ts/package.json`.
`make version-rs V=x.y.z` moves all three and `tests/version_test.rs`
checks the first two.
