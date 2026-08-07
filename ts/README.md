# @tabnas/markdown (TypeScript)

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine.

**This parser is conformant to CommonMark 0.31.2** — all 652 examples, across all 26
sections of the spec suite, in both runtimes. The suite is vendored in this repository, so
the claim is checkable: `npm run conformance` reports 652/652, with no build step and no
engine installed. It runs with the GFM extensions off, which is what measuring CommonMark
conformance means — with `gfm: true` the extensions deliberately change nine of those
examples. It also implements **all five GFM extensions** — tables, task list
items, autolink literals, strikethrough and disallowed raw HTML — 24/24 on the vendored
GFM corpus, via `npm run conformance-gfm`.

This is the canonical implementation; [`go/`](../go/README.md) is a port of it.

[![npm version](https://img.shields.io/npm/v/@tabnas/markdown.svg)](https://npmjs.com/package/@tabnas/markdown)
[![build](https://github.com/tabnas/markdown/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/markdown/actions/workflows/build.yml)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |

## Two outputs

**The AST is the primary output.** `parseDocument()` returns it directly; the renderer
does not run.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('# Hello\n\nHello *world*')

doc.children[0] // => { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] }
doc.children[1] // => { type: 'paragraph', children: [ { type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [ { type: 'text', value: 'world' } ] } ] }
```

**HTML is available, on request, via `toHtml()`.** The CommonMark suite scores HTML
output, so the renderer is what makes the 652/652 claim measurable; that it is useful to
callers is a consequence.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('# Hello\n\nHello *world*') // => '<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n'
```

**The HTML is not sanitized.** Raw HTML blocks and inline tags pass through verbatim, as
CommonMark specifies. Put a sanitizer downstream of any untrusted Markdown.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<img onerror="alert(1)">') // => '<img onerror="alert(1)">\n'
```

GFM's disallowed-raw-HTML filter (on with `gfm`, the default) rewrites the leading `<` of
nine tag names — `title`, `textarea`, `style`, `xmp`, `iframe`, `noembed`, `noframes`,
`script`, `plaintext` — and touches nothing else:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>') // => '&lt;script>alert(1)&lt;/script>\n'
```

As a Tabnas plugin, `parse()` returns the same AST as `parseDocument()`:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const tn = new Tabnas().use(Markdown)

tn.parse('# Hello') // => { type: 'document', children: [ { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] } ] }
```

`parseTree()` returns the native CommonMark node tree instead — it keeps `sourcepos` on
block nodes, and can be walked or mutated and then rendered. See the
[reference](doc/reference.md).

GFM tables add three node types, in mdast's shape — `table`, `tableRow` and `tableCell`.
`align` has one entry per column, `null` where the delimiter cell had no colon; there is
no header flag, so the first row is the header row by convention, and every row has
exactly as many cells as `align` has entries.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('| a | b |\n| :- | -: |\n| 1 | 2 |')

doc.children[0].align // => [ 'left', 'right' ]
doc.children[0].children[0].children[0] // => { type: 'tableCell', children: [ { type: 'text', value: 'a' } ] }
```

## Install

```bash
npm install @tabnas/markdown @tabnas/parser
```

`@tabnas/parser` (>=0) is the only peer dependency. Requires Node >=24.

## Specific to this runtime

The parser is **engine-free**: nothing under `src/commonmark.ts` imports `@tabnas/parser`.
Only `src/markdown.ts`, the plugin wiring, does. That is what lets the conformance runner
work with no build step and no engine installed — it stages `src/*.ts` in a temp directory
whose `package.json` says `"type": "module"` and runs them under Node's type stripping:

```bash
npm run conformance              # 652/652, all 26 sections
npm run conformance -- --failures
npm run conformance-gfm          # 24/24, all five extensions
```

The `// =>` assertions in this repo's Markdown are executed as tests
(`test/doc-examples.test.ts`, and `tools/check-doc-examples.mjs` for the engine-free
check), so a wrong expected value is a failing test.

Options are `gfm` (default `true`) and `breaks` (default `false`). `gfm` gates five
extensions together — tables, strikethrough, task list items, autolink literals (bare
`www.` / `https://` / `a@b.co`) and the disallowed-raw-HTML filter. Footnotes are not
implemented. With `gfm:false` the output is plain CommonMark.

Footnotes are a GitHub product feature rather than part of the GFM spec suite, and their
absence is quiet: `[^1]` is a valid CommonMark link label, so a footnote authored on
GitHub renders as a broken link instead of raising an error.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Text[^1]\n\n[^1]: note') // => '<p>Text<a href="note">^1</a></p>\n'
```

Nothing outside CommonMark and GFM is implemented — no math, front matter, definition
lists, heading attributes, admonitions, wiki links, emoji shortcodes, highlight or
sub/superscript. Each would need its own opt-in flag; `gfm` is not going to grow to mean
"everything". Note one collision: GFM's single-tilde strikethrough takes the syntax other
dialects use for subscript, so `H~2~O` is a deletion under the default `gfm:true`.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('H~2~O') // => '<p>H<del>2</del>O</p>\n'
```

## Documentation

- [Tutorial](doc/tutorial.md) — first parse, start to finish.
- [How-to guide](doc/guide.md) — task recipes.
- [Reference](doc/reference.md) — API, options, AST node types.
- [Concepts](doc/concepts.md) — how it works, and why.

Top-level [README](../README.md) · Go port: [go/README.md](../go/README.md).

## License

MIT. Copyright (c) Richard Rodger and other contributors.
