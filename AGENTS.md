# Agents Guide — markdown

## What this project is

`@tabnas/markdown` is a **CommonMark 0.31.2 parser** for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine. It is not a
subset. Both runtimes score **652/652 on the CommonMark 0.31.2 spec
suite**, across all 26 sections.

On top of CommonMark it implements exactly **one** GFM extension:
strikethrough (`~~x~~`), gated on the `gfm` option (default `true`).
Tables, task list items, autolink literals, footnotes and
disallowed-raw-HTML filtering are **not** implemented. `gfm:false` turns
strikethrough off and gates nothing else, because there is nothing else
to gate. When you describe this package — in code comments, in docs, in
commit messages — say "CommonMark, with one GFM extension". Do not write
"CommonMark/GFM".

There are three public outputs, in both runtimes:

| Output | TypeScript | Go |
|---|---|---|
| mdast-adjacent JSON AST (**primary**) | `parseDocument(src, opts)` | `ParseDocument(src, opts)` |
| CommonMark-conformant HTML (opt-in) | `toHtml(src, opts)` | `ToHTML(src, opts)` |
| Native CommonMark node tree | `parseTree(src, opts)` / `renderHTML(tree)` | `ParseTree(src, opts)` / `RenderHTML(tree)` |

The AST is what the plugin's `.parse()` / `.Parse()` returns, and asking
for it runs no renderer. The HTML emitter is not incidental: the spec
suite scores HTML output, so the renderer is the instrument that makes
the 652/652 claim measurable. **The HTML is not sanitized** — CommonMark
passes raw HTML through verbatim by specification. Every document that
shows HTML output must say so.

It is **not** a record/field (CSV-family) parser — that role was moved
to [`@tabnas/csv`](https://github.com/tabnas/csv). The RFC-4180 leftovers
(`BuildMarkdownStringMatcher`, `test/fixtures/*.csv`) are gone; do not
reintroduce that shape. See `dx-report.md` §1 and the 2026-08-06 entry.

It is a **bare-engine** plugin (not jsonic-based). Install on a Tabnas
instance — `new Tabnas().use(Markdown)`, or `tabnasmarkdown.Make()` in Go.
Its only runtime tabnas dependency is the engine.

There are two implementations that must behave identically — TypeScript
(canonical) and a Go port.

## Repository map

| Path | What it is |
|---|---|
| [`ts/`](ts/) | **Canonical** TypeScript implementation — the `@tabnas/markdown` package. Depends on `@tabnas/parser` only, and only in `src/markdown.ts`. |
| [`go/`](go/) | Go port — `github.com/tabnas/markdown/go`, package `tabnasmarkdown`. |
| [`test/spec/`](test/spec/) | 36 shared **AST** fixtures (`input → expected` JSON, `opts` JSON) in `*.tsv`, auto-discovered and run by both runtimes. The TS/Go parity contract. See `test/AGENTS.md`. |
| [`test/commonmark/spec.json`](test/commonmark/) | Vendored CommonMark 0.31.2 suite, 652 examples of Markdown → expected **HTML**. The conformance contract for both runtimes. See `test/AGENTS.md`. |
| [`markdown-grammar.jsonic`](markdown-grammar.jsonic) | The engine entry rule. **Inert** — see "The grammar is inert" below. |
| [`ts/embed-grammar.js`](ts/embed-grammar.js) | Embeds `markdown-grammar.jsonic` verbatim into both `ts/src/markdown.ts` and `go/markdown.go` between `BEGIN/END EMBEDDED` markers. |
| [`ts/tools/conformance.mjs`](ts/tools/conformance.mjs) | Runs the 652-example suite straight off `ts/src/*.ts` — no build step, no engine. `npm run conformance`. |
| [`ts/tools/check-doc-examples.mjs`](ts/tools/check-doc-examples.mjs) | Runs the `// =>` assertions in the docs without the engine, using a stand-in for it. The CI equivalent is `ts/test/doc-examples.test.ts`. |
| `ts/doc/{tutorial,guide,reference,concepts}.md`, `go/doc/{tutorial,guide,reference,concepts}.md` | Per-runtime Diátaxis docs. Keep the four modes distinct — see "Documentation rules". |
| `dx-report.md` | Running design notes. **Append-only**: new dated entries at the bottom, corrections to earlier sections stated as corrections in the new entry, never as edits to the original text. |

Source files, mirrored name for name across the two runtimes:

| TypeScript | Go | What it holds |
|---|---|---|
| `ts/src/node.ts` | `go/node.go` | The native CommonMark node tree — a linked tree (`parent`/`firstChild`/`lastChild`/`prev`/`next`), plus `sourcepos` / `SourcePos`. |
| `ts/src/common.ts` | `go/common.go` | Spec character classes, backslash/entity unescaping (§6.1, §6.2), link-label normalisation (§4.7), URL + XML escaping. |
| `ts/src/options.ts` | `go/options.go` | `ParserOptions` / `Options`, defaults, `RefDef` / `RefMap`. |
| `ts/src/commonmark.ts` | `go/commonmark.go` | Engine-free entry point: block phase, then inline phase. |
| `ts/src/block.ts` | `go/block.go` | Phase 1 — block structure. |
| `ts/src/inline.ts` | `go/inline.go` | Phase 2 — inlines, delimiter stack, bracket stack. |
| `ts/src/ast.ts` | `go/ast.go` | Projection from the native tree to the public JSON AST. |
| `ts/src/html.ts` | `go/html.go` | HTML renderer over the native tree. |
| `ts/src/markdown.ts` | `go/markdown.go` | Plugin wiring and the public surface. **The only file that imports the engine.** |
| `ts/src/entities.ts` | — | Generated HTML5 named character references (2125 semicolon-terminated entries). Go uses the standard library's table instead, gated to reject the legacy semicolon-less forms §6.2 does not allow. |

There is **no CLI** (`package.json` has no `bin`). There is no
`test/fixtures/` — the 258 orphaned CSV files that lived there were
deleted with the rescope.

## The tabnas engine dependency

Both runtimes depend on the **bare engine**, not jsonic:

- TypeScript: `@tabnas/parser` is a `peerDependency` (`>=0`) and a `file:../../parser/ts` devDependency. `@tabnas/debug`, `@tabnas/railroad` and `@tabnas/jsonic` are dev-only (debug for `debug-model.test.ts`, railroad for `ts/doc/grammar.{svg,txt}`). `engines.node` is `>=24`.
- Go: `go/go.mod` requires `github.com/tabnas/parser/go` and **nothing else** — no jsonic, no indirect requirements. Earlier revisions of this file claimed that while `go.mod` said otherwise; it is now true. Keep it true: a new direct requirement in `go/go.mod` needs a reason stated here.

Development uses `replace github.com/tabnas/parser/go => ../../parser/go`
(via the repo-set `go.work`, not checked in). Clone `parser` (plus
`debug`/`railroad` for optional diagrams/tests) as a sibling and build its
TS (`cd parser/ts && npm i && npm run build`), then work here.
`admin/scripts/link.sh` does this for the whole tabnas folder.

Note the layering, and preserve it: **nothing reachable from
`commonmark.ts` / `commonmark.go` may import `@tabnas/parser`.** That is
what lets the conformance suite and `check-doc-examples.mjs` run with no
engine installed. `markdown.ts` imports `commonmark.ts`; never the
reverse.

## Executable contracts — do not weaken either

Two things in this repo are contracts, not samples:

1. **The conformance corpus.** `test/commonmark/spec.json` is the
   vendored 0.31.2 suite, unmodified. Both runtimes are at 652/652. A
   change that drops an example is a regression, not a trade-off. Do not
   edit `spec.json`, do not skip examples, do not add a tolerance to the
   comparison — it is a byte-for-byte HTML match and must stay one.
2. **The doc-example assertions.** Every ` ```js ` block in `README.md`,
   `ts/README.md`, `go/README.md` and `ts/doc/` that contains a `// =>`
   line is executed by `ts/test/doc-examples.test.ts`. A wrong expected
   value is a failing test. Fix the doc or fix the code; do not delete
   the assertion, and do not mark a block ` ```js ignore ` to make it
   pass.

Both harnesses run ` ```js ` blocks only, so **no Go example anywhere is
executed** — not in `go/README.md`, not in `go/doc/`. Verify Go examples by compiling
them against the real package — a scratch test under `/tmp` run with
`go test`. This is exactly how a `Make()` that did not exist survived in
`README.md` for a long time. (`tabnasmarkdown.Make()` exists now.)

## Authority and alignment rules

**TypeScript is canonical. Go is a port of it.** When you change behaviour:

1. Change the TypeScript first, in the module that owns the concern
   (`block.ts`, `inline.ts`, `ast.ts`, `html.ts`, …).
2. Port the same change to the same-named Go file. The two are mirrored
   deliberately so a diff can be read side by side.
3. Add/extend shared fixture(s) in `test/spec/*.tsv` so both runtimes
   assert the new AST behaviour. The fixtures are the parity contract;
   both suites resolve them (TS: `ts/test/parity.test.ts` →
   `../../test/spec`; Go: `go/parity_test.go` → `../test/spec`).
4. Mirror unit cases across `ts/test/markdown.test.ts` and
   `go/markdown_test.go`.
5. Run both suites, plus both conformance runs, and confirm green before
   landing.

Do not let Go drift from TS. Where Go differs on purpose it is because
the standard library is the better tool (entity decoding, Unicode
punctuation — both noted in `go/common.go`), and the observable behaviour
is still identical. If Go cannot match, document the gap here and in the
relevant `go/doc/*.md`.

The AST comparison alone is not a complete parity check: `sourcepos` is
not projected into the public AST, so a divergence there is invisible to
it. When you touch anything positional, compare the native trees too.

## The grammar is inert — and never hand-edit the embedded block

`markdown-grammar.jsonic` does not drive parsing. Block structure is
decided by the line algorithm in `block.ts` / `block.go`; the `markdown`
rule exists only so the engine's token stream is consumed and its
trailing-content check passes. The railroad diagram it produces
(`ts/doc/grammar.svg`) is empty — a bare track, no boxes. Say that plainly in docs
rather than implying the grammar is a source of truth.

The file is kept because `ts/embed-grammar.js` embeds it verbatim into
**both** `ts/src/markdown.ts` and `go/markdown.go`, between:

```
// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
...
// --- END EMBEDDED markdown-grammar.jsonic ---
```

Edit the `.jsonic`, then run:

```bash
cd ts && node embed-grammar.js   # writes into ts/src/markdown.ts AND go/markdown.go
```

Never hand-edit the embedded block in either runtime. The grammar may not
contain backticks (Go raw-string limitation).

## Build & test

TypeScript (from `ts/`):

```bash
npm install            # resolves file: siblings
npm run build          # embeds grammar, then tsc --build src test
npm test               # node --test over dist-test/*.test.js
npm run conformance    # 652-example suite off src/*.ts — no build, no engine
node tools/check-doc-examples.mjs --verbose   # the // => assertions, no engine
```

Go (from `go/`):

```bash
go build ./...
go test -v ./...                          # unit + shared fixtures + conformance
go test -run TestCommonMarkSpec -v ./...   # conformance only, per-section table
```

`npm run conformance` and `check-doc-examples.mjs` both stage `src/*.ts`
into a temp directory whose `package.json` says `{"type":"module"}` and
run them under Node's type stripping, because `ts/package.json` is
`"type": "commonjs"`. Use the same trick for ad-hoc TypeScript
experiments.

The repo root `Makefile` wraps both halves: `make build|test|clean` run TS
and Go sides; `make publish-go V=x.y.z` tags `go/vX.Y.Z`. CI is the
org-standard `polyglot-ci` caller in `.github/workflows/ci.yml`.

## Architecture notes

* **Two phases, per spec 0.31.2 Appendix A.** Block structure over the
  whole document first, then inlines inside each leaf block. The order is
  forced: block syntax is line-oriented and inline syntax is not, so
  deciding blocks first makes block-beats-inline precedence structural
  rather than something the inline scanner has to remember.
* **Block phase** (`block.ts` / `block.go`) keeps a spine of open blocks.
  For each line it walks the spine consuming continuation markers, looks
  for new block starts at the offset the walk reached, and puts the
  remainder in the deepest open block. Unmatched blocks are closed
  *after* the search for new starts, not during the walk — that ordering
  is the whole of lazy continuation. Two positions are tracked per line
  (a UTF-16/byte offset for slicing, a tab-expanded column for every
  indentation decision) because a tab can be partially consumed by a list
  marker.
* **Inline phase** (`inline.ts` / `inline.go`) is a left-to-right scan.
  Code spans, autolinks, raw HTML, entities, escapes and breaks are
  decided locally. Two constructs cannot be: emphasis needs a delimiter
  stack (flanking rules, the rule of three) and links need a bracket
  stack. Link reference definitions are collected during paragraph
  finalization in the block phase and resolved here.
* **Native tree, then projection.** The parse result is a linked
  CommonMark node tree (`node.ts` / `node.go`) — linked rather than
  children-arrays because both phases splice nodes mid-walk, which an
  array-of-children shape turns into an index rewrite every time.
  `ast.ts` / `ast.go` projects it to the public AST; `html.ts` /
  `html.go` renders the tree. The projection is **deliberately lossy**:
  soft breaks collapse into a space in the surrounding text run
  (long-standing documented behaviour; `breaks:true` promotes them to
  `break` nodes) and source positions are dropped. The tree keeps
  `sourcepos` / `SourcePos` on block nodes.
* **HTML newline placement is a correctness contract**, not formatting.
  Every block writes `cr()` before its opening tag and after its closing
  tag, `cr()` is a no-op at line start, and no block emits a newline on
  behalf of a neighbour or child. Changing that breaks conformance
  examples.
* **Plugin wiring** (`markdown.ts` / `markdown.go`): entry rule is
  `markdown`; the `bo` action reads the whole source via `ctx.src()` and
  hands it to the parser. Lex is disabled for `string`/`comment`/
  `number`/`value` so backticks, `# headings` and `1. lists` are not
  mis-lexed first. `lex.emptyResult` returns an empty document for `""`.
  `rule.clear()` is required before defining `markdown`, or inherited
  `val` alts try to match a leading `#`.
* **Options**: `gfm` (default `true`, strikethrough only) and `breaks`
  (default `false`). `Markdown.defaults` / `Defaults` carry the same pair.

## Documentation rules

The docs follow [Diátaxis](https://diataxis.fr), one file per quadrant per
runtime, and the four modes must not blur:

* **tutorial.md** — a lesson. One path, concrete steps, no options, no
  API tables, no alternatives.
* **guide.md** — recipes. Each section is "How do I … ?" for someone who
  already has the basics. Not a second tutorial, not a second reference.
* **reference.md** — dry and complete. Tables, exact types, exact
  defaults, exhaustive node lists. Describes; does not explain, instruct
  or persuade.
* **concepts.md** — discursive. Why the design is what it is, what the
  alternatives were, what they cost. No step-by-step, no full API dumps.

A paragraph that would sit equally well in two of them is in the wrong
file or is too vague. Two questions must be answerable within the first
screen of the README and near the top of each quadrant document: does
this emit HTML (yes, `toHtml` / `ToHTML`), and can I just have the AST
(yes, it is the primary output and costs nothing extra). Keep the
Diátaxis cross-link table in `README.md` current.
