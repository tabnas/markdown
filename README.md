# @tabnas/markdown

<!-- tabnas-badges -->
[![npm](https://tabnas.github.io/status/badges/markdown-npm.svg)](https://www.npmjs.com/package/@tabnas/markdown)
[![CI](https://github.com/tabnas/markdown/actions/workflows/ci.yml/badge.svg)](https://github.com/tabnas/markdown/actions/workflows/ci.yml)
[![go](https://tabnas.github.io/status/badges/markdown-go.svg)](https://pkg.go.dev/github.com/tabnas/markdown/go)
[![tabnas standard](https://tabnas.github.io/status/badges/markdown-standard.svg)](https://tabnas.github.io/status/)
<!-- /tabnas-badges -->

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine.

**This parser is conformant to CommonMark 0.31.2.** All 652 examples pass, across all 26
sections of the spec suite, in both implementations — TypeScript (canonical) and Go (a
port of it). The suite is vendored in this repository, so the claim is checkable rather
than asserted:

```bash
cd ts && npm run conformance                    # 652/652
cd go && go test -run TestCommonMarkSpec ./...  # 652/652
```

On top of CommonMark it implements **five GFM extensions** — tables, task list items,
autolink literals, strikethrough and disallowed raw HTML. That is the complete GFM
extension set: 24/24 on the vendored GFM corpus.

The parser itself is engine-free; the Tabnas plugin is wiring around it.

## Two outputs: the AST, and HTML if you ask

**The AST is the primary output.** `parseDocument()` returns a JSON tree and runs nothing
else — no renderer, no extra cost.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('# Hello\n\nHello *world*')

doc.type // => 'document'
doc.children[0] // => { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] }
doc.children[1] // => { type: 'paragraph', children: [ { type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [ { type: 'text', value: 'world' } ] } ] }
```

**Yes, there is an HTML emitter.** It is opt-in, and it is not a side utility: the
CommonMark suite scores HTML output, so the renderer is the instrument that makes the
652/652 claim measurable. Being useful to callers is a consequence of that, not the
motive.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('# Hello\n\nHello *world*') // => '<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n'
```

In Go the same pair is `ParseDocument()` and `ToHTML()`:

```go
opts := tabnasmarkdown.DefaultOptions

doc := tabnasmarkdown.ParseDocument("# Hello\n\nHello *world*", opts)
fmt.Println(doc["type"]) // document

fmt.Print(tabnasmarkdown.ToHTML("# Hello\n\nHello *world*", opts))
// <h1>Hello</h1>
// <p>Hello <em>world</em></p>
```

On a Tabnas engine instance, the plugin's `parse()` returns that same AST:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const tn = new Tabnas().use(Markdown)

tn.parse('# Hello') // => { type: 'document', children: [ { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] } ] }
```

There is a third output for callers who want to walk or mutate before rendering:
`parseTree()` / `ParseTree()` returns the native CommonMark node tree, which keeps
`sourcepos` / `SourcePos` on block nodes. `renderHTML()` / `RenderHTML()` renders a tree
back to HTML, so parse, transform and render are three separate steps when you need them
to be. See the reference for each runtime.

## Install

```bash
# TypeScript (Node >= 24)
npm install @tabnas/markdown @tabnas/parser

# Go (1.24+)
go get github.com/tabnas/markdown/go@latest
```

`@tabnas/parser` is a peer dependency of the npm package. The Go module requires
`github.com/tabnas/parser/go` — the bare engine — and nothing else.

## What is parsed

**CommonMark 0.31.2, in full.** All 26 sections of the spec suite pass: tabs, backslash
escapes, entity and numeric character references, precedence, thematic breaks, ATX
headings, Setext headings, indented code blocks, fenced code blocks, HTML blocks, link
reference definitions, paragraphs, blank lines, block quotes, list items, lists, inlines,
code spans, emphasis and strong emphasis, links, images, autolinks, raw HTML, hard line
breaks, soft line breaks, textual content.

**All five GFM extensions**, gated together on the `gfm` option (default `true`):

| Extension | TypeScript | Go |
|---|---|---|
| Tables | yes | yes |
| Task list items (`- [x] done`) | yes | yes |
| Autolink literals (bare `www.` / `https://` / `a@b.co`) | yes | yes |
| Strikethrough (`~~text~~`) | yes | yes |
| Disallowed raw HTML (`<script>` → `&lt;script>`) | yes | yes |
| Footnotes | no | no |

TypeScript is canonical and the Go port follows it; the two are level, and verified
example for example on both outputs. `gfm:false` turns every one of them off, and the
output is then plain CommonMark, byte for byte.

Tables contribute three node types, in mdast's shape — `table`, `tableRow` and
`tableCell`. `align` carries one entry per column, `null` where the delimiter cell had no
colon, and there is no header flag: the first row is the header row, by convention.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('| a | b |\n| :- | -: |\n| 1 | 2 |')

doc.children[0].align // => [ 'left', 'right' ]
doc.children[0].children[0].children[0] // => { type: 'tableCell', children: [ { type: 'text', value: 'a' } ] }
```

Every row has exactly as many cells as `align` has entries: the block phase pads short
rows and truncates long ones. The native tree names the same three nodes `table`,
`table_row` and `table_cell`.

**Footnotes are not implemented.** They are a GitHub product feature, not part of the GFM
spec suite. Because `[^1]` is a valid CommonMark link label, a GitHub-authored footnote
does not error — it silently renders as a broken link:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Text[^1]\n\n[^1]: note') // => '<p>Text<a href="note">^1</a></p>\n'
```

Nothing outside CommonMark and GFM is implemented either — no math, front matter,
definition lists, heading attributes, admonitions, wiki links, emoji shortcodes,
highlight or sub/superscript. Each would need its own opt-in flag; `gfm` is not going to
grow to mean "everything". One collision is worth knowing about: GFM's single-tilde
strikethrough takes the syntax other dialects use for subscript, so under the default
`gfm:true` a subscript becomes a deletion.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('H~2~O') // => '<p>H<del>2</del>O</p>\n'
```

The only options, in both runtimes, are `gfm` (default `true`) and `breaks` (default
`false`, which promotes soft line breaks to hard breaks when set). See the reference for
each runtime.

## The HTML is not sanitized

CommonMark passes raw HTML blocks and inline tags through verbatim, by specification, and
so does this renderer:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<img onerror="alert(1)">') // => '<img onerror="alert(1)">\n'
```

Anything that renders untrusted Markdown needs a sanitizer downstream. GFM's
disallowed-raw-HTML filter neutralises nine tag names and nothing else; it is not a
sanitizer, and it does nothing about attributes or `javascript:` destinations.

## Conformance

**The parser is conformant to CommonMark 0.31.2** — 652/652, all 26 sections, in both
runtimes. The suite is vendored at [`test/commonmark/spec.json`](test/commonmark/), so you
can check that for yourself rather than take it on trust:

```bash
cd ts && npm run conformance                    # no build step, no engine needed
cd go && go test -run TestCommonMarkSpec -v ./...
```

Both report 652/652, run with the GFM extensions off — which is what measuring
CommonMark conformance means. GFM deliberately changes the output of nine of those
examples (six raw-HTML, three autolink), so with `gfm: true` the same suite reports
643/652. That is the extensions working, not a conformance failure.

The GFM corpus is vendored the same way, at [`test/gfm/spec.json`](test/gfm/) (24
examples, covering all five extensions):

```bash
cd ts && npm run conformance-gfm
cd go && go test -run TestGFMSpec -v ./...
```

Both report 24/24.

Parity between the runtimes is checked separately: 676 examples (652 CommonMark + 24 GFM)
across 4 option combinations (`gfm` x `breaks`) is 2704 records, with 0 differing ASTs and
0 differing HTML outputs. The 75 shared AST fixtures in [`test/spec/`](test/spec/) also run
in both.

## Changes you may notice

* `spread` on lists and list items now follows mdast semantics. It was previously latched
  by whether the file ended in a newline, so `- a\n- b\n` and `- a\n\n- b\n` produced
  identical output and the field carried no information. It now does.
* Inline raw HTML produces `{ type: 'html', value: '<b>' }` inline nodes. There was
  previously no node type for it and tags leaked into `text`.
* Tables add three public AST node types — `table`, `tableRow` and `tableCell`. Input that
  previously came back as a paragraph of pipe characters is now a `table` node under the
  default `gfm:true`.
* Otherwise the AST is unchanged — the `test/spec/*.tsv` fixtures pass untouched.

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework — one file per
quadrant, per language.

| | TypeScript | Go |
|---|---|---|
| Tutorial (learn) | [ts/doc/tutorial.md](ts/doc/tutorial.md) | [go/doc/tutorial.md](go/doc/tutorial.md) |
| How-to (recipes) | [ts/doc/guide.md](ts/doc/guide.md) | [go/doc/guide.md](go/doc/guide.md) |
| Reference (API + options + AST) | [ts/doc/reference.md](ts/doc/reference.md) | [go/doc/reference.md](go/doc/reference.md) |
| Concepts (how it works) | [ts/doc/concepts.md](ts/doc/concepts.md) | [go/doc/concepts.md](go/doc/concepts.md) |

Per-language hubs: [ts/README.md](ts/README.md) · [go/README.md](go/README.md).

## Repository layout

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation (canonical). |
| [`go/`](go/) | Go port. |
| [`test/spec/`](test/spec/) | 75 shared AST fixtures, run by both runtimes. |
| [`test/commonmark/`](test/commonmark/) | Vendored CommonMark 0.31.2 spec suite (652 examples). |
| [`test/gfm/`](test/gfm/) | Vendored GFM extension corpus (24 examples), run by both runtimes. |
| [`markdown-grammar.jsonic`](markdown-grammar.jsonic) | The engine entry rule, embedded into both runtimes by [`ts/embed-grammar.js`](ts/embed-grammar.js). It is inert: block structure is decided by the line algorithm, not by declarative alts. Kept because the embedder embeds it. |

> **Rescope note:** this package was previously a CSV-family record parser (copied from
> `@tabnas/csv`). It now parses prose Markdown; record parsing is available via
> [`@tabnas/csv`](https://github.com/tabnas/csv). See `dx-report.md`.

## License

MIT. Copyright (c) Richard Rodger and other contributors.
