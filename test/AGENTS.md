# Agents Guide — shared test corpora

Three corpora live here, and they test different things. Know which one a
change belongs in before you add to any of them.

| Directory | Contents | Compares | Canonical for | Add to it? |
|---|---|---|---|---|
| [`spec/`](spec/) | 75 hand-written cases across 10 `*.tsv` files | the **JSON AST**, through the engine | TS ↔ Go parity, and the shape of the public AST | **yes** — this is where cases of our own go |
| [`commonmark/`](commonmark/) | the vendored CommonMark 0.31.2 suite, 652 examples in `spec.json` | the **HTML** output, byte for byte | spec conformance | no — upstream data |
| [`gfm/`](gfm/) | the extension sections of the GFM spec, 24 examples in `spec.json` | the **HTML** output, byte for byte | the GFM extensions | no — upstream data |

Scores, both runtimes: **652/652** on the CommonMark suite — the parser
is conformant to CommonMark 0.31.2, all 26 sections — and **24/24** on
the GFM corpus, which is the complete GFM extension set.

All three are executable contracts: do not weaken any of them to make a
change pass. All three are run by both runtimes.

---

# `spec/*.tsv` — the parity corpus

Both runtimes auto-discover and run **every** file in `spec/`, so a change
there affects TypeScript and Go together — edit with that in mind. Each
case goes through a real engine instance with the plugin installed
(`Tabnas().use(Markdown).parse()` / `parser.Make()` + `UseDefaults`), so
this corpus covers the plugin path, not just the parser.

This is where the public AST is pinned. Neither HTML corpus can do that
job: they score HTML, and the AST is a lossy projection of the tree the
renderer walks, so a projection change can leave 652/652 and 24/24 both
untouched.

75 cases in 10 files, one construct each: `autolink.tsv` (23),
`blockquote.tsv` (2), `code.tsv` (4), `heading.tsv` (7), `inline.tsv`
(13), `list.tsv` (6), `mixed.tsv` (1), `paragraph.tsv` (4), `table.tsv`
(10), `thematic.tsv` (5). Eight of those rows carry `listItem.checked`
(six in `list.tsv`, one each in `blockquote.tsv` and `mixed.tsv`), which
is the task-list extension's AST surface; if they fail, the fixtures are
right and the runtime is wrong.

The table node types — `table` (with `align`), `tableRow` and
`tableCell` — are pinned here too, by `table.tsv`: delimiter-row forms,
alignment, escaped pipes, and row padding and truncation. The HTML side
of tables is still asserted per runtime, in `ts/test/commonmark.test.ts`
and `go/gfm_test.go`, which are mirrored by hand rather than shared.

## Format

Tab-separated, one case per line, with a header row naming the columns.
Blank lines are skipped, and so are comment lines — a line starting with
`#` that contains no tab. (A data row always has at least one tab, so a
`#`-leading source such as an ATX heading still works.)

| Column | Meaning |
|---|---|
| `input` | Markdown source. Escapes `\n` `\r` `\t` `\\` are decoded. |
| `expected` | A JSON value (the parse result — a `{type:'document', children:[…]}` AST), or `ERROR` / `ERROR:<substring>` for inputs that must fail. |
| `opts` | Optional JSON object of plugin options. Empty means the defaults, `{"gfm":true,"breaks":false}`. |

`expected` and `opts` are **not** escape-decoded — they are raw JSON, so
JSON's own escape rules apply (`"a\nb"` is a string containing a newline).
To put a literal backslash in `input`, write `\\`.

Results are compared after a JSON round-trip, so key order and the
`OrderedMap` / null-prototype-object representations do not affect the
comparison.

## Who runs what

- TypeScript: `ts/test/parity.test.ts` — reads `../../test/spec` at runtime
  from `dist-test/`, one `describe` per file.
- Go: `go/parity_test.go` — `TestSpec` globs `../test/spec/*.tsv`.

Both discover files by directory listing: adding a `.tsv` here runs it in
both runtimes without touching either runner.

## Adding a case

1. Append a row to the file that owns the construct (`heading.tsv`,
   `list.tsv`, `inline.tsv`, …), or add a new `*.tsv` — it is picked up
   automatically. Keep one construct per file.
2. Get the `expected` JSON from the canonical runtime, not by hand.
3. Run `npm test` (from `ts/`) and `go test ./...` (from `go/`). A new
   fixture must pass in BOTH.

## Rules

- Prefer adding a fixture here over a one-off in-language assertion when a
  case is expressible as input → output. That is what keeps the two
  runtimes honest against each other.
- TypeScript is canonical. If the two runtimes disagree, the TS behaviour is
  the expected value — unless Go has exposed a genuine TS defect, in which
  case fix TS first and pin the corrected behaviour here.
- This corpus does not cover `sourcepos` / `SourcePos`, which the AST
  projection drops. Positional changes need a native-tree comparison as
  well; a green `spec/` run cannot see them.

---

# `commonmark/spec.json` — the conformance corpus

The CommonMark 0.31.2 specification suite, vendored verbatim: 652 examples
across 26 sections, each a Markdown source and the exact HTML a conformant
implementation must produce. **Both runtimes pass 652/652, all 26
sections.**

The comparison is a byte-for-byte string equality on rendered HTML. It is
the conformance contract, and the reason the HTML renderer exists — without
a renderer there is nothing to score. It is also the evidence behind the
conformance claim the READMEs and the doc quadrants make: they state that
the parser is conformant to CommonMark 0.31.2 and print the command that
runs this corpus, so a reader can check it. Keep it runnable in one
command from each runtime.

Run with GFM **off** (`{gfm:false, breaks:false}`). The suite is pure
CommonMark; strikethrough changes the expected output for several examples.

## Who runs what

- TypeScript: `ts/test/commonmark.test.ts`, part of `npm test`. Imports the
  built parser directly, so it exercises no engine code.
- TypeScript, no build and no engine: `npm run conformance`
  (`ts/tools/conformance.mjs`), which reports a per-section table and takes
  `--failures`, `--section=`, `--example=`, `--json`.
- Go: `go/commonmark_test.go` — `go test -run TestCommonMarkSpec -v ./...`,
  which logs the same per-section table. `TestCommonMarkOptionMatrix` in the
  same file re-runs the corpus across all four `GFM` × `Breaks`
  combinations, asserting only that nothing panics — the corpus itself never
  exercises those.

Both loaders assert the file still holds exactly 652 examples, so a
truncated or half-replaced vendored file fails loudly rather than quietly
scoring well.

## Editing it

Don't. This file is upstream data. The only legitimate change is replacing
it wholesale with a later published spec suite, which is a deliberate
version bump: update the 652 count in both loaders, update every stated
version, and expect real failures to fix. Never edit an individual example,
never delete one, and never add a tolerance to the comparison to make
something pass.

Cases of our own go in `spec/*.tsv` or in the runtimes' unit tests, not
here. The GFM extensions in particular cannot be covered by this corpus at
all, because it runs with `gfm:false`; they have their own corpus below.

---

# `gfm/spec.json` — the GFM extension corpus

The extension sections of the GFM spec, vendored in the same shape as the
CommonMark suite: 24 examples across Tables (8), Autolinks (11), Task list
items (2), Strikethrough (2) and Disallowed Raw HTML (1). Run with GFM
**on** (`{gfm:true, breaks:false}`).

The core CommonMark examples of that document are deliberately excluded:
cmark-gfm tracks CommonMark 0.29, and nine of its emphasis cases expect
pre-0.31.2 output this parser correctly no longer produces. Core
conformance is `commonmark/spec.json`, against 0.31.2.

**24/24 today, in both runtimes.** Every section passes in both, and every
section is asserted in both — there is nothing failing and nothing
reported-but-tolerated. That covers the whole extension set the spec
defines: tables, task list items, autolink literals, strikethrough and
disallowed raw HTML.

Footnotes are not here, and their absence is not a gap in this corpus:
they are a GitHub product feature rather than part of the GFM spec suite.
Nothing outside CommonMark+GFM belongs here either.

## Who runs what

- TypeScript: `ts/test/commonmark.test.ts` runs every section as part of
  `npm test`.
- TypeScript, no build and no engine: `node ts/tools/gfm-conformance.mjs`,
  which reports a per-section table and takes `--failures` and `--section=`.
- Go: `go/gfm_test.go` (`go test -run TestGFMSpec -v ./...`) prints the
  same per-section table, and asserts every section.

Both loaders assert the file still holds exactly 24 examples, the same
guard the CommonMark loaders use.

## Editing it

Don't — same rule as the CommonMark suite. It is upstream data, and the
score is only meaningful because the corpus is unmodified.

Cases of our own go in `spec/*.tsv` (for AST shape) or in the runtimes'
unit tests (for HTML and for behaviour the 24 upstream examples do not
reach — delimiter-row forms, paragraph splitting, escaped pipes, row
padding and truncation, `gfm:false` inertness). `ts/test/commonmark.test.ts`
and `go/gfm_test.go` already hold that layer; mirror any addition across
both, since nothing shares it for you.
