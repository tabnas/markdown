# How-to guide: @tabnas/markdown (TypeScript)

Task-oriented recipes. Each one is self-contained. If this is your first run,
start with the [tutorial](tutorial.md); for exact types and defaults, the
[reference](reference.md).

Two answers first, because most of these recipes depend on them:

- **The AST is the primary output.** `parseDocument(src, opts)` returns it,
  without running the renderer and without loading the engine.
- **HTML output is available**: `toHtml(src, opts)`. The CommonMark suite
  scores HTML, so the renderer is what makes the 652/652 result measurable; it
  is a first-class output, not a side utility. It is also **not sanitized** —
  see [Render untrusted Markdown safely](#render-untrusted-markdown-safely).

The parser is CommonMark 0.31.2 plus one GFM extension (strikethrough).

## Get just the AST, without the engine

Call `parseDocument`. It takes a source string and returns the JSON AST.

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('# Hello') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
```

`@tabnas/parser` is a peer dependency of this package, but on this path it is a
type-only import: nothing loads the engine, and the HTML renderer is not
touched. `parseInline(text, opts)` does the same for a single run of inline
Markdown, returning `Inline[]`.

## Render HTML

```js
import { toHtml } from '@tabnas/markdown'

toHtml('# Hello\n\n*hi*\n') // => '<h1>Hello</h1>\n<p><em>hi</em></p>\n'
```

The output is byte-exact against the CommonMark 0.31.2 expected HTML —
including where the newlines fall, which is a correctness contract, not
formatting. `toHtml` parses from source; it does not take an AST.

The HTML is not sanitized. If any of the input came from someone else, read
[Render untrusted Markdown safely](#render-untrusted-markdown-safely) before
you ship it.

## Use the plugin on an existing Tabnas instance

Register `Markdown` on the instance and call `.parse()`. Options go in the
`.use()` call.

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown, parseDocument } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# Hello') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
JSON.stringify(j.parse('- a\n- b')) === JSON.stringify(parseDocument('- a\n- b')) // => true
```

`.parse()` returns exactly what `parseDocument` returns. The instance is
reusable — call `.parse()` as often as you like. If you load other plugins,
`Markdown` must go last, because it claims the `markdown` start rule.

The plugin also hangs the other entry points off the instance, already bound to
the options you registered it with:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown, { gfm: false })

j.markdown.toHtml('# Hello') // => '<h1>Hello</h1>\n'
j.parse('~~keep~~') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'text', value: '~~keep~~' }] }] }
```

## Collect every link URL

The AST is plain JSON, so a recursive walk over `children` is all you need.
Container nodes have `children`; leaf nodes such as `text`, `inlineCode`,
`code`, `html` and `image` do not.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('See [docs](https://example.com/docs) and [spec](https://spec.commonmark.org).\n\n- [home](/)\n')

const urls = []
const visit = (node) => {
  if ('link' === node.type) urls.push(node.url)
  for (const child of node.children ?? []) visit(child)
}
visit(doc)

urls // => ['https://example.com/docs', 'https://spec.commonmark.org', '/']
```

Images carry their destination on `url` too, and their text on `alt` rather
than in children. Swap the type test to `'image'` to collect those instead.

## Rewrite links, then render

`parseDocument` projects the parse to JSON and drops what JSON has no room for.
When you need to change a document and still render it, work on the native tree
instead: `parseTree` gives you the `MdNode` the renderer actually reads, and
`renderHTML` turns it back into HTML.

`MdNode` is a linked tree (`firstChild`, `next`, `parent`, …), and
`node.walker()` gives you a depth-first cursor yielding `{entering, node}`.

```js
import { parseTree } from '@tabnas/markdown'

const tree = parseTree('# Title\n\nSee [docs](/docs).\n')

const walker = tree.walker()
let ev
while ((ev = walker.next())) {
  const n = ev.node
  if (ev.entering && 'link' === n.type && n.destination.startsWith('/')) {
    n.destination = 'https://example.com' + n.destination
  }
}

tree.lastChild.firstChild.next.destination // => 'https://example.com/docs'
```

Then render the mutated tree. `renderHTML` is not on the package's main export;
it comes from the engine-free parser module:

```js
import { renderHTML } from '@tabnas/markdown/dist/commonmark.js'

const html = renderHTML(tree)
```

which for the tree above produces:

```html
<h1>Title</h1>
<p>See <a href="https://example.com/docs">docs</a>.</p>
```

`renderHTML` takes the same options object as everything else; only `breaks`
affects it, since `gfm` is decided at parse time.

## Render untrusted Markdown safely

Sanitize the HTML downstream. This package does not, and cannot: CommonMark
specifies that raw HTML passes through verbatim, and passing it through is part
of scoring 652/652.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>\n') // => '<script>alert(1)</script>\n'
toHtml('[click](javascript:alert(1))\n') // => '<p><a href="javascript:alert(1)">click</a></p>\n'
```

What reaches the output untouched:

- HTML blocks (§4.6) — whole `<script>`, `<style>`, `<iframe>` blocks and
  anything else that starts a block-level tag.
- Inline raw HTML (§6.6) — `<b onclick="...">` and friends, attributes intact.
- Link and image destinations, including `javascript:` URLs. They are
  percent-encoded and entity-decoded, not filtered by scheme.

GFM's disallowed-raw-HTML filter is not implemented, so `gfm: false` changes
none of this — it only turns strikethrough off.

Run the output through a sanitizer (DOMPurify, sanitize-html, or whatever your
stack already uses) before it reaches a browser. If you would rather detect raw
HTML than strip it, the AST surfaces it as `html` nodes — one per tag inline,
one per block:

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('Hi <b>there</b>').children[0].children // => [{ type: 'text', value: 'Hi ' }, { type: 'html', value: '<b>' }, { type: 'text', value: 'there' }, { type: 'html', value: '</b>' }]
```

That tells you the input contains HTML; it does not make the HTML safe, and it
says nothing about link destinations. The sanitizer is still the fix.

## Turn strikethrough off, or hard breaks on

There are two options, `gfm` (default `true`) and `breaks` (default `false`),
and they behave the same on `parseDocument`, `parseInline`, `toHtml`,
`parseTree` and `.use(Markdown, opts)`.

`gfm` gates GFM strikethrough, and nothing else — strikethrough is the only GFM
extension implemented here. Tables, task lists, bare `www.`/`https://` autolink
literals and footnotes are not, with `gfm: true` or without it.

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('~~gone~~').children[0].children // => [{ type: 'delete', children: [{ type: 'text', value: 'gone' }] }]
parseDocument('~~gone~~', { gfm: false }).children[0].children // => [{ type: 'text', value: '~~gone~~' }]
toHtml('~~gone~~', { gfm: false }) // => '<p>~~gone~~</p>\n'
```

`breaks` promotes soft line breaks to hard ones:

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('a\nb', { breaks: true }).children[0].children // => [{ type: 'text', value: 'a' }, { type: 'break' }, { type: 'text', value: 'b' }]
toHtml('a\nb', { breaks: true }) // => '<p>a<br />\nb</p>\n'
```

## Report errors with line numbers

Use `parseTree`. The JSON AST has no source positions — they are one of the
things the projection drops — but every block node on the native tree carries
`sourcepos` as `[[startLine, startCol], [endLine, endCol]]`, 1-based, as in the
spec. Inline nodes carry `[[0, 0], [0, 0]]`.

```js
import { parseTree } from '@tabnas/markdown'

const tree = parseTree('# Title\n\n## Section\n\n#### Too deep\n\npara\n')

const problems = []
const walker = tree.walker()
let ev
while ((ev = walker.next())) {
  const n = ev.node
  if (ev.entering && 'heading' === n.type && 3 < n.level) {
    problems.push('line ' + n.sourcepos[0][0] + ': heading level ' + n.level)
  }
}

problems // => ['line 5: heading level 4']
```

Native node types are the CommonMark ones (`heading`, `paragraph`,
`block_quote`, `list`, `item`, `code_block`, `html_block`, `emph`, `strong`,
`link`, …), not the AST's mdast-adjacent names.

## Check conformance yourself

The TypeScript suite runs straight from source — no build step, and no
`@tabnas/parser` installed:

```bash
cd ts
npm run conformance
```

It prints a per-section table and a total, which is `652/652 100.00%`. Narrow
it down when you are chasing one case:

```bash
node tools/conformance.mjs --section=Emphasis\ and\ strong\ emphasis
node tools/conformance.mjs --example=42
node tools/conformance.mjs --failures
node tools/conformance.mjs --json
```

The Go port runs the same 652 examples:

```bash
cd go
go test -run TestCommonMarkSpec -v ./...
```

The shared AST fixtures in `test/spec/*.tsv` are asserted by both runtimes —
`npm test` (after `npm run build`) on the TypeScript side, `go test ./...` on
the Go side. Between them they are the parity contract: the two runtimes are
checked over all 652 examples across all four `gfm` × `breaks` combinations,
and produce identical ASTs and identical HTML.
