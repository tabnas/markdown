# Concepts: how @tabnas/markdown works (TypeScript)

Background and design rationale. For the API see the [reference](reference.md).

## It is a grammar plugin, not a parser — but the prose complexity lives in JS

`@tabnas/markdown` is a plugin for the Tabnas engine. The pipeline is:

```
new Tabnas()        // the bare engine: a table-driven lexer + rule parser
  .use(Markdown)    // layer the Markdown grammar, then call .parse()
```

The engine itself is a deterministic, backtracking-free rule machine: rules have `open` and `close` phases each holding ordered alternates (`s` lookahead, `p` push, `r` repeat, `a` action, etc.). Markdown's block grammar — headings interrupting paragraphs, lists inside blockquotes, fenced code boundaries — could be encoded as many declarative alts, but at CommonMark scale the indentation and line-prefix logic is clearer as a small line scanner.

So the `markdown` rule is intentionally tiny (see `markdown-grammar.jsonic` and `dx-report.md` §2.1):

- `bo` (`@markdown-bo`) reads the full source via `ctx.src()` and calls `parseDocument(src, opts)` to build a `document` node.
- `open: [{s:'#AA', r:'markdown'}, {}]` and `close: [{s:'#ZZ'}, {}]` then consume the underlying token stream so the engine's trailing-content check (`lex` must end at `#ZZ`) passes.

The *actual* Markdown — ATX/Setext headings, thematic breaks, fenced/indented code, blockquotes, lists (ordered/unordered, nested), paragraphs, HTML — is parsed by `parseDocument` and its helper `parseBlocks`, which operate line-by-line. Inline structure (emphasis/strong/code/links/images/autolinks/breaks/strikethrough) is parsed by `parseInline`, invoked per block (headings, paragraphs, list items).

This keeps one grammar to maintain, lets `debug.model()` stay honest (a single `markdown` rule), and makes TS→Go parity a matter of porting `parseDocument`/`parseInline`, not of replicating a large declarative alt table. The railroad diagram therefore shows `markdown → document` rather than a fine-grained CFG — an explicit trade-off.

## Why the bare engine (not jsonic)

`zon/TEMPLATE.md` says: JSON-family formats layer on `@tabnas/jsonic` (and strip what they don't need via `rule.exclude`); non-JSON-family formats use the bare engine directly. Markdown is not JSON-shaped, so this package depends only on `@tabnas/parser` (peerDep `>=0`) and disables the `string`/`comment`/`number`/`value` lexers that would otherwise turn backticks or `# headings` into bad tokens before `parseDocument` runs.

## Blocks — a line scanner, then recursion

`parseDocument(src)` splits on `\r?\n` and calls `parseBlocks(lines, 0, opts)`. Precedence matters (see `ts/src/markdown.ts`):

1. Blank lines (skipped; significant inside lists/blockquote via spread)
2. Fenced code (`^ {0,3}(`{3,}|~{3,})` …) — content consumed until matching closing fence; language and meta split on first space.
3. Indented code (`^ {4}|\t` and not a list marker)
4. ATX heading (`^ {0,3}#{1,6}[ \t]+…`)
5. Thematic break (`---` `***` `___` with spaces) — but `---` as a setext underline for a preceding paragraph is handled inside the paragraph path (see below).
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
- Autolinks (`<https://example.com>`, `<mailto>`, `<a@b>` email)
- Strong (`**`/`__`) before emphasis (`*`/`_`) so `**a *b* c**` nests correctly; `_` inside `foo_bar_baz` is word-boundary-checked and left as text (per CommonMark).
- GFM strikethrough (`~~`, gated on `opts.gfm`)
- Breaks: a single `\n` inside a paragraph source becomes a `break` when preceded by two spaces or when `opts.breaks:true`; otherwise a soft break becomes a single space inside the current text run.

`parseInline` recurses for nested delimiters (`**a *b* c**` → strong containing emphasis).

## Why a single rule is not a limitation

A tabnas grammar being "data" means an agent can emit one and you can validate it before running it. Markdown's prose grammar being a single rule plus a JS helpers set preserves that property: the helpers *are* the grammar, they are checkable, diff-friendly, and JSON-serialisable at the AST level. A future change that moves more of `parseBlocks` into declarative alts would show up as more rules in `debug.model()` and as a wider railroad — but is not required for correctness and would complicate parity.

## Deferred scope

See `dx-report.md` §2.3: reference-style link resolution (two-pass), GFM tables and task lists (`- [x]`) are recorded as deferred. `gfm` gates only strikethrough for now; a `gfmTables` follow-up would add block-level pipe handling and inline `|` inside table cells. Loose vs tight list wrapping is exposed as `spread` but not yet normalised into paragraph wrappers.
