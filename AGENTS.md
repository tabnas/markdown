# Agents Guide — markdown

## What this project is

`@tabnas/markdown` is a **CommonMark / GFM prose parser** for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine. It parses
ATX/Setext headings, paragraphs, thematic breaks, fenced/indented code
blocks, blockquotes, ordered/unordered lists (nested), HTML blocks, and
inline (escapes, code spans, emphasis/strong, links, images, autolinks,
breaks, GFM strikethrough via `~~`) into a single
`{type:'document', children: Block[]}` JSON AST (mdast-adjacent).

It is **not** a record/field (CSV-family) parser — that role was moved
to [`@tabnas/csv`](https://github.com/tabnas/csv). The old record
grammar used `markdown`/`newline`/`record`/`text` + code-driven
`list`/`elem`/`val` rules, `strict`/`header`/`object`/`field` options,
RFC-4180 quoting, `test/fixtures/*.csv`, and `test/spec/*.tsv` record
cases. The prose rescope (see `dx-report.md` and `markdown-grammar.jsonic`
header) replaced all of that with a prose line scanner. The package name
was retained; see the README's rescope note and `dx-report.md` §1.

It is a **bare-engine** plugin (not jsonic-based). Install on a Tabnas
instance — `new Tabnas().use(Markdown)` — or, for backward compat,
after `jsonic` on an existing engine (`new Tabnas().use(jsonic).use(Markdown)`
still works; Markdown wins the `markdown` start rule). Its only runtime
tabnas dependency is the engine.

There are two implementations that must behave identically — TypeScript
(canonical) and a Go port.

## Repository map

| Path | What it is |
|---|---|
| [`ts/`](ts/) | **Canonical** TypeScript implementation — the `@tabnas/markdown` package. Plugin in `src/markdown.ts` (parseDocument/parseInline + Markdown plugin). Depends on `@tabnas/parser` only. |
| [`go/`](go/) | Go port — `github.com/tabnas/markdown/go`. Plugin in `markdown.go`. Depends on `github.com/tabnas/parser/go` (bare engine) via `replace` in development. |
| [`markdown-grammar.jsonic`](markdown-grammar.jsonic) | The grammar, **intentionally trivial** for prose: a single `markdown` rule whose `bo` calls `parseDocument(ctx.src())`; `open: [{s:'#AA', r:'markdown'}, {}]` / `close: [{s:'#ZZ'}, {}]` merely consumes the token stream so the engine's trailing-content check passes. The prose complexity lives in JS (`parseDocument`/`parseInline`), not in declarative alts — see `dx-report.md` §2.1. |
| [`ts/embed-grammar.js`](ts/embed-grammar.js) | Embeds `markdown-grammar.jsonic` verbatim into both `ts/src/markdown.ts` and `go/markdown.go` between `BEGIN/END EMBEDDED` markers. Run `node ts/embed-grammar.js`. The grammar may not contain backticks (Go raw-string limitation). |
| [`test/spec/`](test/spec/) | Shared **prose** conformance fixtures (`input → expected` JSON, `opts` JSON), run by both runtimes. Auto-discovered (`*.tsv`). See `test/AGENTS.md`. |
| [`test/fixtures/`](test/fixtures/) | Legacy CSV fixtures retained only for provenance; not used by the prose suite. Prose cases live in `test/spec`. |
| `ts/doc/{tutorial,guide,reference,concepts}.md`, `go/doc/{tutorial,guide,reference,concepts}.md` | Per-runtime Diátaxis docs (tutorial / how-to / reference / concepts). The `reference` documents the `DocumentNode`/`Block`/`Inline` AST. |
| `dx-report.md` | Running rescope notes: why prose is a single rule + JS scanner, block precedence, inline handling, deferred scope (reference links, GFM tables/task lists, loose/tight). |

There is **no CLI** (`package.json` has no `bin`).

## The tabnas engine dependency

Both runtimes depend on the **bare engine**, not jsonic:

- TypeScript: `@tabnas/parser` is a `peerDependency` (`>=0`) and a `file:../../parser/ts` devDependency. `@tabnas/debug` + `@tabnas/railroad` are dev-only `file:` deps (debug for `debug-model.test.ts`, railroad for `ts/doc/grammar.{svg,txt}`). `engines.node` is `>=24`.
- Go: `go/go.mod` requires `github.com/tabnas/parser/go` (bare engine) with `replace github.com/tabnas/parser/go => ../../parser/go` for sibling development.

Clone `parser` (plus `debug`/`railroad` for optional diagrams/tests) as a sibling and build its TS (`cd parser/ts && npm i && npm run build`), then work here. `admin/scripts/link.sh` does this for the whole tabnas folder.

## Authority and alignment rules

**TypeScript is canonical. Go is a port of it.** When you change behaviour:

1. Change `ts/src/markdown.ts` first (or `markdown-grammar.jsonic` if the `markdown` rule's alts change).
2. Port the same change to `go/markdown.go` (parseDocument/parseInline + Markdown plugin + Defaults + lex config).
3. Add/extend shared fixture(s) in `test/spec/*.tsv` so both runtimes assert the new behaviour. The fixtures are the parity contract; both suites resolve them (`TS: ts/test/parity.test.ts` → `../../test/spec`; `Go: go/parity_test.go` → `../test/spec`).
4. Mirror unit cases across `ts/test/markdown.test.ts` and `go/markdown_test.go`.
5. Run both suites and confirm green before landing.

Do not let Go drift from TS. If Go cannot match due to a parser limitation, document the gap here and in the relevant `go/doc/*.md`.

## The grammar is embedded — never hand-edit the embedded block

`markdown-grammar.jsonic` is embedded verbatim into **both** `ts/src/markdown.ts` and `go/markdown.go` between:

```
// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
...
// --- END EMBEDDED markdown-grammar.jsonic ---
```

Edit the `.jsonic`, then run:

```bash
cd ts && node embed-grammar.js   # writes into ts/src/markdown.ts AND go/markdown.go
```

The prose parser reads the full source via `ctx.src()` in `@markdown-bo`, so the embedded grammar may stay tiny (see the header there).

## Build & test

TypeScript (from `ts/`):

```bash
npm install            # resolves file: siblings
npm run build          # embeds grammar, then tsc --build src test
npm test               # node --test over dist-test/*.test.js
```

Go (from `go/`):

```bash
go build ./...
go test -v ./...       # unit + shared fixtures
```

The repo root `Makefile` wraps both halves: `make build|test|clean` run TS and Go sides; `make publish-go V=x.y.z` tags `go/vX.Y.Z`. `admin/scripts/link.sh` creates the sibling `go.work` + node_modules symlinks for local development; there is no checked-in `go.work` here.

## Architecture notes

* Entry rule is `markdown`; lex disabled for `string`/`comment`/`number`/`value` so backticks, `# headings`, and `1. lists` are not mis-lexed before `parseDocument` runs. `lex.emptyResult` returns an empty document for `""`.
* Blocks are line-scanned in precedence order (blank, fenced code, indented code, ATX heading, thematic break — handled after setext in the paragraph path — blockquote, HTML block, list, paragraph). Setext handling lives inside the paragraph path (a `===`/`---` underline after at least one paragraph line becomes a heading level 1/2). See `ts/src/markdown.ts` block scanner for the exact order.
* Inline is a delimiter walk invoked per block (heading/paragraph/listItem). See Concepts for the scanning order.
* `Markdown.defaults = { gfm:true, breaks:false }`; the only options are `gfm` (enables `~~strikethrough~~`) and `breaks` (soft breaks as breaks).
