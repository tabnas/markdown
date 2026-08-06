# Concepts: how the markdown plugin works (Go)

Background and design rationale, plus the differences between the Go port and
the canonical TypeScript implementation. For the API see the
[reference](reference.md).

## It is a grammar plugin, not a parser — but the prose complexity lives in Go

The package is a **plugin for the tabnas engine**, ported from the canonical
TypeScript package `@tabnas/markdown`. The pipeline is:

```
tabnasjsonic.Make()                                  // engine + jsonic base grammar
  .UseDefaults(tabnasmarkdown.Markdown, Defaults)    // layer the Markdown grammar
  .Parse(src)                                        // => document node
```

The engine itself is a deterministic, backtracking-free rule machine. Markdown's
block grammar — headings interrupting paragraphs, lists inside blockquotes, fenced
code boundaries — could be encoded as many declarative alts, but at CommonMark
scale the indentation and line-prefix logic is clearer as a small line scanner.

So the `markdown` rule is intentionally tiny (see `markdown-grammar.jsonic` and
`dx-report.md` §2.1):

- `bo` (`markdown-bo`) reads the full source via `ctx.Src` and calls `parseDocument(src, opts)` to build a `document` node.
- `open: [{s:'#AA', r:'markdown'}, {}]` and `close: [{s:'#ZZ'}, {}]` then consume the underlying token stream so the engine's trailing-content check (`lex` must end at `#ZZ`) passes.

The *actual* Markdown — ATX/Setext headings, thematic breaks, fenced/indented
code, blockquotes, lists (ordered/unordered, nested), paragraphs, HTML — is
parsed by `parseDocument` and its helper `parseBlocks`, which operate
line-by-line. Inline structure (emphasis/strong/code/links/images/autolinks/breaks/strikethrough)
is parsed by `parseInline`, invoked per block (headings, paragraphs, list items).

This keeps one grammar to maintain, lets `debug.model()` stay honest (a single
`markdown` rule), and makes TS→Go parity a matter of porting `parseDocument`/`parseInline`,
not of replicating a large declarative alt table. The railroad diagram therefore
shows `markdown → document` rather than a fine-grained CFG — an explicit trade-off.

## Why the bare engine (not jsonic base)

`zon/TEMPLATE.md` says: JSON-family formats layer on `@tabnas/jsonic` (and strip
what they don't need via `rule.exclude`); non-JSON-family formats use the bare
engine directly. Markdown is not JSON-shaped, so this package disables the
`string`/`comment`/`number`/`value` lexers that would otherwise turn backticks
or `# headings` into bad tokens before `parseDocument` runs, and sets
`lex.emptyResult` to `{type:"document", children:[]}`. The dependency on
`github.com/tabnas/jsonic/go` is still present because the Go test harness and
engine API are re-exported through it, but the Markdown prose parser does not
use jsonic's `val`/`map`/`list` rules.

## Blocks — a line scanner, then recursion

`parseDocument(src)` splits on `\r?\n` and calls `parseBlocks(lines, 0, opts)`. Precedence matters (see `go/markdown.go`):

1. Blank lines (skipped; significant inside lists/blockquote via spread)
2. Fenced code (`^ {0,3}(`{3,}|~{3,})` …) — content consumed until matching closing fence; language and meta split on first space.
3. Indented code (`^ {4}|\t` and not a list marker)
4. ATX heading (`^ {0,3}#{1,6}[ \t]+…`)
5. Thematic break (`---` `***` `___` with spaces) — but `---` as a setext underline for a preceding paragraph is handled inside the paragraph path.
6. Blockquote (`^ {0,3}>\s?`) — lines collected, `>` stripped, recursive `parseBlocks` for the inner content.
7. HTML block (`<...>` line until blank or block boundary) — raw.
8. List (`-*+` / `\d+[.)]`) — consecutive items grouped into one `list`; each item's lines (including indented continuations and blank lines) are joined and recursively `parseBlocks`-ed to produce its `children: Block[]`.
9. Paragraph — lines until blank or block boundary; if the last collected line is a setext underline (`===`/`---`) and at least two lines were collected, the paragraph is reinterpreted as a Setext heading.

`spread` on `list`/`listItem` records whether blank lines occurred inside the item — callers that render to HTML can use it to decide tight vs loose wrapping.

## Inline — a delimiter scanner

`parseInline(text, opts)` walks the text, emitting `Text` by default and flushing on delimiters:

- Escapes (`\*` → `*`, etc.)
- Code spans (`` ` ``, `` `` ``) — trimmed and internal newlines collapsed to spaces.
- Images (`![alt](url "title")`) and links (`[label](url "title")`) — handled before emphasis so `[ *a* ](url)` parses `*a*` inside the label.
- Autolinks (`<https://example.com>`, `<mailto>`, `<a@b>` email) — gated on `gfm`
- Strong (`**`/`__`) before emphasis (`*`/`_`) so `**a *b* c**` nests correctly; `_` inside `foo_bar_baz` is word-boundary-checked and left as text (per CommonMark).
- GFM strikethrough (`~~`, gated on `opts.gfm`)
- Breaks: a single `\n` inside a paragraph source becomes a `break` when preceded by two spaces or when `opts.breaks:true`; otherwise a soft break becomes a single space inside the current text run.

`parseInline` recurses for nested delimiters (`**a *b* c**` → strong containing emphasis).

## The embedded grammar and `@`-refs

The prose grammar is intentionally trivial (`markdown-grammar.jsonic` contains a single `markdown` rule with `open: [{s:'#ZZ'}]` etc., whose `bo` action is the JS/Go block scanner). It is embedded verbatim into `go/markdown.go` (and `ts/src/markdown.ts`) by `ts/embed-grammar.js` during the TS build. Never hand-edit the embedded block.

At runtime the GramSpec is built from that text and wired into the engine; `@`-prefixed names resolve against the `refs` map (`@markdown-bo` etc.). This is the same declarative pattern as the TS plugin, which is what keeps the two ports aligned from one grammar source.

## Re-invocation guard

`Use` re-runs a plugin on later `SetOptions` calls, so the Go plugin guards
against double-application with a decoration (`j.Decoration("markdown-init")`).
That keeps the plugin idempotent.

## Differences from the TS version

The TypeScript implementation is canonical and the Go package is a port. The
parity fixtures in `test/spec/` keep them aligned for the cases they cover, but a
few shape and behaviour differences remain:

### API shape

| | TypeScript | Go |
|---|---|---|
| Construct | `new Tabnas().use(Markdown, opts)` | `tabnasjsonic.Make()` then `j.UseDefaults(Markdown, Defaults, opts)` |
| Options | a partial `MarkdownOptions` object | `map[string]any` |
| Defaults merge | the plugin fills defaults itself | you pass `Defaults` to `UseDefaults` |
| Parse | `j.parse(src)` returns the value, throws on error | `j.Parse(src)` returns `(any, error)` |
| Helpers | `parseDocument`, `parseInline` are exported | unexported (internal only) |

### Value types

| Value | TypeScript | Go |
|---|---|---|
| Document | plain JS object `{type:"document", children:[...]}` | `map[string]any` with same shape — type-assert to `map[string]any`, read `["children"].([]any)` |
| Block/Inline | plain objects | `map[string]any` / `[]any` |
| `null` | JS `null` | Go `nil` (appears as `nil` in `map`, `<nil>` when printed; `null` after `json.Marshal`) |
| Empty | `{type:"document", children:[]}` | same |

### Behaviour

- **`gfm`/`breaks` only.** The legacy CSV options (`field.*`, `record.*`, `string.*`, `header`, `object`, `strict`, `trim`, `comment`, `number`, `value`, `stream`) are not part of the prose parser in either runtime; the Go `concepts.md` previously documented them because it was copied from the `csv` template — now removed.
- **HTML block detection.** Go's `isHtmlBlockLine` uses `reHtmlTag = ^<\/?[a-zA-Z][\w-]*(?:\s[^>]*)?>` so `<https://example.com>` is not mistaken for an HTML block (it is an autolink). This matches the TS `isHtmlBlockLine` regex change made for the autolink fix.

Everything else — ATX/Setext headings, thematic breaks, fenced/indented code, blockquotes, lists, paragraphs, HTML, and inline (emphasis/strong/code/links/images/autolinks/breaks/delete) — behaves the same across both runtimes, as enforced by `test/spec/`.
