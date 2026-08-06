# @tabnas/markdown (TypeScript)

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine, scoring
**652/652 on the CommonMark 0.31.2 spec suite**, with one GFM extension (strikethrough).
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
CommonMark specifies, and GFM's disallowed-raw-HTML filter is not implemented. Put a
sanitizer downstream of any untrusted Markdown.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>') // => '<script>alert(1)</script>\n'
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
npm run conformance              # 652/652
npm run conformance -- --failures
```

The `// =>` assertions in this repo's Markdown are executed as tests
(`test/doc-examples.test.ts`, and `tools/check-doc-examples.mjs` for the engine-free
check), so a wrong expected value is a failing test.

Options are `gfm` (default `true`) and `breaks` (default `false`).

## Documentation

- [Tutorial](doc/tutorial.md) — first parse, start to finish.
- [How-to guide](doc/guide.md) — task recipes.
- [Reference](doc/reference.md) — API, options, AST node types.
- [Concepts](doc/concepts.md) — how it works, and why.

Top-level [README](../README.md) · Go port: [go/README.md](../go/README.md).

## License

MIT. Copyright (c) Richard Rodger and other contributors.
