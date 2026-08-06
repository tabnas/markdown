# Agents Guide — shared test corpora

Two corpora live here, and they test different things. Know which one a
change belongs in before you add to either.

| Directory | Contents | Compares | Canonical for |
|---|---|---|---|
| [`spec/`](spec/) | 36 hand-written cases in `*.tsv` | the **JSON AST**, through the engine | TS ↔ Go parity, and the shape of the public AST |
| [`commonmark/`](commonmark/) | the vendored CommonMark 0.31.2 suite, 652 examples in `spec.json` | the **HTML** output, byte for byte | spec conformance |

Both are run by both runtimes. Both are executable contracts: do not
weaken either to make a change pass.

---

# `spec/*.tsv` — the parity corpus

Both runtimes auto-discover and run **every** file in `spec/`, so a change
there affects TypeScript and Go together — edit with that in mind. Each
case goes through a real engine instance with the plugin installed
(`Tabnas().use(Markdown).parse()` / `parser.Make()` + `UseDefaults`), so
this corpus covers the plugin path, not just the parser.

This is where the public AST is pinned. The CommonMark suite cannot do
that job: it scores HTML, and the AST is a lossy projection of the tree
the renderer walks, so a projection change can leave 652/652 untouched.

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
implementation must produce. **Both runtimes pass 652/652.**

The comparison is a byte-for-byte string equality on rendered HTML. It is
the conformance contract, and the reason the HTML renderer exists — without
a renderer there is nothing to score.

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

Cases of our own — including GFM strikethrough, which this corpus cannot
cover because it runs with `gfm:false` — go in `spec/*.tsv` or in the
runtimes' unit tests, not here.
