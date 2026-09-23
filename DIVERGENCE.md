# Divergences

TypeScript is the canonical implementation; the Go and Rust ports track
it. This file records where a runtime produces a **different result for
the same input**, and why the difference is allowed to stand.

Neither entry can be written as a row of `test/spec/*.tsv`, which is why
this repository still has no `test/spec/divergent.tsv`: one is invisible
to the AST those fixtures compare, and the other needs a document far
larger than a fixture cell. A divergence a row can express belongs in a
register, with a `rust` column, per [`AGENTS.md`](AGENTS.md).

## Token columns after an astral character (engine, Go and Rust)

The engine's token columns count characters, where the canonical
TypeScript engine counts UTF-16 units, so every column after a character
outside the Basic Multilingual Plane is one further left. It is recorded
upstream in `parser/DIVERGENCE.md` ("Column positions for astral
characters") and pinned here by `go/enginecol_test.go` and
`rs/tests/engine_columns_test.rs` on the same table.

It reaches the token stream alone. The AST and the native tree, whose
`sourcepos` the shared goldens compare, agree in all three runtimes.

## Nesting is capped in the Rust port

| input | TypeScript | Go | Rust |
|---|---|---|---|
| 100 `> ` markers | 100 block quotes | 100 block quotes | 100 block quotes |
| 101 `> ` markers | 101 block quotes | 101 block quotes | 100 block quotes, the last marker literal |
| 50 `- ` markers | 50 lists and items each | the same | the same |
| 51 `- ` markers | 51 lists and items each | the same | 50 each, the last marker literal |
| 100 `*` a side | 50 `strong` | 50 `strong` | 50 `strong` |
| 102 `*` a side | 51 `strong` | 51 `strong` | 50 `strong`, the extra stars literal |

The TypeScript and Go columns state what an implementation with no cap
produces: neither carries one. The only nesting bound those two share
with the Rust port is `MAX_LINK_PAREN_NESTING` (32 parentheses inside a
link destination), which CommonMark itself allows and all three apply.

`rs/src/block.rs` caps block quotes, lists and list items at
`MAX_CONTAINER_NESTING` (100), and `rs/src/inline.rs` caps emphasis,
link and image wrappers at `MAX_INLINE_NESTING` (50). A marker past
either bound stays literal text, as an unpaired marker does, so the text
the document carries is never dropped: only its nesting stops.
Every column of the table above is asserted, not described, and that
includes the half the Rust cells state in words: **the overflow markers
stay literal**.
`rs/tests/robust_test.rs::nesting_is_capped_at_the_constants` pins both
Rust boundaries and the text they produce past them -- `<p>&gt; x</p>`
one marker over, `<li>- x</li>` for a list, and `<p>**<strong>` …
`</strong>**</p>` for the stars, with the wrapper and star counts pinned
exactly. It used to check only the depth and that the content survived,
which a port that kept `x` and dropped the markers would also have
passed, while the recorded output had changed.
`ts/test/divergence.test.ts` and
`go/robust_test.go::TestNestingIsUncapped` pin the uncapped rows in the
other two runtimes, at the same depths and with the same deepest-run
count. Every count is over ONE node type. The cells above distinguish
lists from items and `strong` from `emph`, so a predicate matching
either of a pair would let one stand in for the other -- 100 lists and
no items, or emphasis where the table records strong -- and leave the
assertion green over a cell that had changed. So each of the three
suites counts `list`, `item` and `strong` separately, and pins `emph` at
zero where paired stars are the input. A list marker still opens two
containers in all three runtimes, which is why the bound is reached at
half as many markers as block quotes need.

Splitting the register across three suites is what the absence of a
`test/spec/divergent.tsv` row costs here, and it is the reason given
above: a fixture row states one expected value per input. Repairing the
divergence means deleting all three assertions along with this section.

The cap is there for the caller's stack, not the parser's. Every phase
of the Rust port is iterative, and a document of 8,000 nested block
quotes parses, projects and renders on an ordinary 2 MB thread. What
recurses is the AST type itself: a `tabnas::Value` walks its own nesting
in `to_json()` and again in its drop, both outside this crate. Measured
in a debug build on a 2 MB thread, the drop survives about 5,300 levels
and `to_json()` about 1,250; each container costs two levels, so the two
caps leave the deepest accepted document well inside the smaller of
them. Without a cap, an ordinary caller that merely dropped the returned
AST aborted the process, and a library cannot ask every caller to run on
a larger stack.

The canonical runtime reaches the same wall from the other side: a
deeply nested document throws `RangeError` from `JSON.stringify` rather
than returning a value. TypeScript can throw where Rust would abort, so
it needs no cap to stay safe.

The repair belongs upstream in `tabnas/parser` (`rs/src/value.rs`): an
iterative drop and an iterative `to_json` would let both constants go,
and this entry with them. The engine's own review records that work as
open.
