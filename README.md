# @tabnas/markdown

<!-- tabnas-badges -->
[![npm](https://tabnas.github.io/status/badges/markdown-npm.svg)](https://www.npmjs.com/package/@tabnas/markdown)
[![CI](https://github.com/tabnas/markdown/actions/workflows/ci.yml/badge.svg)](https://github.com/tabnas/markdown/actions/workflows/ci.yml)
[![go](https://tabnas.github.io/status/badges/markdown-go.svg)](https://pkg.go.dev/github.com/tabnas/markdown/go)
[![tabnas standard](https://tabnas.github.io/status/badges/markdown-standard.svg)](https://tabnas.github.io/status/)
<!-- /tabnas-badges -->

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine. It scores
**652/652 on the CommonMark 0.31.2 spec suite**, in both of its implementations —
TypeScript (canonical) and Go (a port of it) — plus a handful of GFM extensions.
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

**GFM extensions**, all gated on the `gfm` option (default `true`):

| Extension | TypeScript | Go |
|---|---|---|
| Strikethrough (`~~text~~`) | yes | yes |
| Task list items (`- [x] done`) | yes | yes |
| Autolink literals (bare `www.` / `https://` / `a@b.co`) | yes | yes |
| Disallowed raw HTML (`<script>` → `&lt;script>`) | yes | yes |
| Tables | no | no |
| Footnotes | no | no |

TypeScript is canonical and the Go port follows it; the two are level, and verified
example for example on both outputs. `gfm:false` turns every one of them off, and the
output is then plain CommonMark, byte for byte.

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

The CommonMark 0.31.2 suite is vendored at [`test/commonmark/spec.json`](test/commonmark/)
(652 examples). Run it:

```bash
cd ts && npm run conformance                    # no build step, no engine needed
cd go && go test -run TestCommonMarkSpec -v ./...
```

Both report 652/652.

Parity between the runtimes is checked separately: 652 examples across 4 option
combinations (`gfm` x `breaks`) is 2608 records, with 0 differing ASTs and 0 differing
HTML outputs. The 36 shared AST fixtures in [`test/spec/`](test/spec/) also run in both.

## Changes you may notice

* `spread` on lists and list items now follows mdast semantics. It was previously latched
  by whether the file ended in a newline, so `- a\n- b\n` and `- a\n\n- b\n` produced
  identical output and the field carried no information. It now does.
* Inline raw HTML produces `{ type: 'html', value: '<b>' }` inline nodes. There was
  previously no node type for it and tags leaked into `text`.
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
| [`test/spec/`](test/spec/) | 36 shared AST fixtures, run by both runtimes. |
| [`test/commonmark/`](test/commonmark/) | Vendored CommonMark 0.31.2 spec suite (652 examples). |
| [`markdown-grammar.jsonic`](markdown-grammar.jsonic) | The engine entry rule, embedded into both runtimes by [`ts/embed-grammar.js`](ts/embed-grammar.js). It is inert: block structure is decided by the line algorithm, not by declarative alts. Kept because the embedder embeds it. |

> **Rescope note:** this package was previously a CSV-family record parser (copied from
> `@tabnas/csv`). It now parses prose Markdown; record parsing is available via
> [`@tabnas/csv`](https://github.com/tabnas/csv). See `dx-report.md`.

## License

MIT. Copyright (c) Richard Rodger and other contributors.
