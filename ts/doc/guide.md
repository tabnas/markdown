# How-to guide: @tabnas/markdown (TypeScript)

Task-oriented recipes. Each one is self-contained. If this is your first run,
start with the [tutorial](tutorial.md); for exact types and defaults, the
[reference](reference.md).

Two answers first, because most of these recipes depend on them:

- **The AST is the primary output.** `parseDocument(src, opts)` returns it,
  without running the renderer and without loading the engine.
- **HTML output is available**: `toHtml(src, opts)`. The CommonMark suite
  scores HTML, so the renderer is what makes the conformance result measurable;
  it is a first-class output, not a side utility. It is also **not sanitized** —
  see [Render untrusted Markdown safely](#render-untrusted-markdown-safely).

**The parser is conformant to CommonMark 0.31.2**: 652/652 examples of the
specification's own suite, all 26 sections, in both the TypeScript and the Go
runtime. The suite is vendored in this repository, so the claim is checkable —
`npm run conformance` runs it; see
[Check conformance yourself](#check-conformance-yourself).

On top of CommonMark it implements the complete set of five GFM extensions —
tables, task list items, autolink literals, strikethrough and the
disallowed-raw-HTML filter. That is 24/24 on the vendored GFM extension suite,
which `npm run conformance-gfm` runs. All five are gated on the `gfm` option,
default `true`.

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

Then render the mutated tree with `renderHTML`, which takes a tree rather than a
source string:

```js
import { renderHTML } from '@tabnas/markdown'

const html = renderHTML(tree)
```

which for the tree above produces:

```html
<h1>Title</h1>
<p>See <a href="https://example.com/docs">docs</a>.</p>
```

`renderHTML` takes the same options object as everything else. `breaks` affects
it, and `gfm` selects one thing only — the disallowed-raw-HTML filter, which the
extension defines at render time. Called with no options, it uses the `gfm` the
tree was parsed with.

## Render untrusted Markdown safely

Sanitize the HTML downstream. This package does not, and cannot: CommonMark
specifies that raw HTML passes through verbatim, and passing it through is part
of scoring 652/652.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<img onerror="alert(1)">\n') // => '<img onerror="alert(1)">\n'
toHtml('[click](javascript:alert(1))\n') // => '<p><a href="javascript:alert(1)">click</a></p>\n'
```

What reaches the output untouched:

- HTML blocks (§4.6) — whole `<div>`, `<form>`, `<table>` blocks and anything
  else that starts a block-level tag.
- Inline raw HTML (§6.6) — `<b onclick="...">` and friends, attributes intact.
- Link and image destinations, including `javascript:` URLs. They are
  percent-encoded and entity-decoded, not filtered by scheme.

The one thing `gfm` changes here is the disallowed-raw-HTML filter, which
rewrites the leading `<` of nine tag names — `title`, `textarea`, `style`,
`xmp`, `iframe`, `noembed`, `noframes`, `script`, `plaintext` — and leaves
every other tag, and every attribute, alone:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>\n') // => '&lt;script>alert(1)&lt;/script>\n'
toHtml('<script>alert(1)</script>\n', { gfm: false }) // => '<script>alert(1)</script>\n'
```

That is nine tag names out of the whole of HTML. It is not a sanitizer.

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

## Turn the GFM extensions off, or hard breaks on

There are two options, `gfm` (default `true`) and `breaks` (default `false`),
and they behave the same on `parseDocument`, `parseInline`, `toHtml`,
`parseTree` and `.use(Markdown, opts)`.

`gfm` gates five extensions as one switch — tables, strikethrough, task list
items, autolink literals and the disallowed-raw-HTML filter. Footnotes are not
implemented, with `gfm: true` or without it. With `gfm: false` the output is
plain CommonMark.

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('~~gone~~').children[0].children // => [{ type: 'delete', children: [{ type: 'text', value: 'gone' }] }]
parseDocument('~~gone~~', { gfm: false }).children[0].children // => [{ type: 'text', value: '~~gone~~' }]
toHtml('~~gone~~', { gfm: false }) // => '<p>~~gone~~</p>\n'
```

## Read a table

A table is three AST node types: `table`, `tableRow` and `tableCell`. Start from
the `table` node and read `align` for the column alignments.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('| Item | Qty |\n| :--- | ---: |\n| pen | 3 |\n| ink | 12 |\n')
const table = doc.children[0]

table.type // => 'table'
table.align // => ['left', 'right']
table.children.length // => 3
```

`align` has one entry per column, taken from the colons in the delimiter row:
`:--` is `'left'`, `--:` is `'right'`, `:-:` is `'center'`, and a cell with no
colon gives `null`.

There is no header flag — this is mdast's shape, and mdast's convention is that
the **first** row is the header row. Take it off the front and the rest are body
rows:

```js
import { parseDocument } from '@tabnas/markdown'

const table = parseDocument('| Item | Qty |\n| :--- | ---: |\n| pen | 3 |\n| ink | 12 |\n').children[0]
const [header, ...body] = table.children

header.children.map((cell) => cell.children[0].value) // => ['Item', 'Qty']
body.map((row) => row.children.map((cell) => cell.children[0].value)) // => [['pen', '3'], ['ink', '12']]
```

That indexing is safe because every row has exactly as many cells as `align` has
entries: the block phase pads short rows with empty cells and truncates long
ones. So `table.align[i]` is the alignment of cell `i` of every row, and
`row.children.length === table.align.length` always holds.

A cell's `children` are inline nodes, the same ones you get anywhere else, so
walk them the way you walk a paragraph:

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('| a | b |\n| - | - |\n| *x* | `y` |\n').children[0].children[1] // => { type: 'tableRow', children: [{ type: 'tableCell', children: [{ type: 'emphasis', children: [{ type: 'text', value: 'x' }] }] }, { type: 'tableCell', children: [{ type: 'inlineCode', value: 'y' }] }] }
```

To get a pipe inside a cell, escape it. `\|` never splits a cell, and it is
turned into a literal `|` before the inline phase runs, so a code span in that
cell holds a real pipe:

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('| a |\n| - |\n| b `\\|` az |\n').children[0].children[1].children[0].children // => [{ type: 'text', value: 'b ' }, { type: 'inlineCode', value: '|' }, { type: 'text', value: ' az' }]
toHtml('| a |\n| - |\n| b `\\|` az |\n') // => '<table>\n<thead>\n<tr>\n<th>a</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>b <code>|</code> az</td>\n</tr>\n</tbody>\n</table>\n'
```

If a table does not appear where you expected one, the shape of the source is
usually why:

- A table is recognised when a delimiter row directly follows a paragraph whose
  **last line** has the same number of cells. Earlier lines of that paragraph
  stay a paragraph, and the table starts after it.
- A delimiter cell is hyphens with an optional leading and/or trailing colon and
  nothing else. One stray character and the line is just text.
- Leading and trailing pipes are optional, and may differ from row to row.
- The table ends at a blank line or at the start of another block.
- With `gfm: false` there is no table at all: the lines stay one paragraph.

Rendering is `toHtml` as usual. A table with no body rows emits no `<tbody>`:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('| a | b |\n| - | - |\n') // => '<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n'
```

## Work with task lists and bare URLs

A list item whose first paragraph starts with `[ ]` or `[x]` and at least one
space is a task list item: the marker is consumed and `listItem.checked` records
its state (`null` on an ordinary item).

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('- [x] done\n- [ ] todo\n- plain\n').children[0].children.map((i) => i.checked) // => [true, false, null]
toHtml('- [x] done\n') // => '<ul>\n<li><input checked="" disabled="" type="checkbox"> done</li>\n</ul>\n'
```

Bare URLs and addresses become links without angle brackets, at the start of a
line, after whitespace, or after `*`, `_`, `~` or `(`. Trailing sentence
punctuation is left out of the link.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Visit www.commonmark.org.\n') // => '<p>Visit <a href="http://www.commonmark.org">www.commonmark.org</a>.</p>\n'
toHtml('mail me at a@b.co\n') // => '<p>mail me at <a href="mailto:a@b.co">a@b.co</a></p>\n'
toHtml('www.commonmark.org\n', { gfm: false }) // => '<p>www.commonmark.org</p>\n'
```

`breaks` promotes soft line breaks to hard ones:

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('a\nb', { breaks: true }).children[0].children // => [{ type: 'text', value: 'a' }, { type: 'break' }, { type: 'text', value: 'b' }]
toHtml('a\nb', { breaks: true }) // => '<p>a<br />\nb</p>\n'
```

## Handle Markdown written for GitHub, or for another dialect

The five GFM extensions are the whole of what `gfm` turns on. Two things that
authors write anyway will parse without complaint and render as something else,
so check for them rather than waiting for a bug report.

**Footnotes are not implemented.** They are a GitHub product feature, not part
of the GFM specification suite. Nothing errors, because `[^1]` is a valid
CommonMark link label: the reference falls through to literal text, and the
definition line becomes an ordinary paragraph.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Text[^1]\n\n[^1]: Note text here.\n') // => '<p>Text[^1]</p>\n<p>[^1]: Note text here.</p>\n'
```

Worse, a definition whose body happens to look like a destination *is* a valid
link reference definition, so the footnote quietly becomes a link:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Text[^1]\n\n[^1]: /note\n') // => '<p>Text<a href="/note">^1</a></p>\n'
```

Scan for `[^` before you convert a corpus, and strip or rewrite footnotes in the
source. There is no option that changes this.

**Single-tilde subscript collides with strikethrough.** GFM's strikethrough
accepts a single `~` as well as `~~`, so the `H~2~O` subscript syntax that
Pandoc and several other dialects use comes out as `<del>`:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('H~2~O\n') // => '<p>H<del>2</del>O</p>\n'
toHtml('H~2~O\n', { gfm: false }) // => '<p>H~2~O</p>\n'
```

If your input is really Pandoc-flavoured, escape the tildes (`H\~2\~O`) or parse
with `gfm: false`; there is no way to keep strikethrough and lose the collision.

Everything else outside CommonMark and the five GFM extensions is also absent —
math, front matter, definition lists, heading attributes, admonitions, wiki
links, emoji shortcodes, highlight, and sub/superscript. Each would need its own
opt-in flag rather than joining `gfm`, so none of them appear under `gfm: true`.

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
`link`, …), not the AST's mdast-adjacent names. A table is `table`, `table_row`
and `table_cell` there.

## Check conformance yourself

The TypeScript suite runs straight from source — no build step, and no
`@tabnas/parser` installed:

```bash
cd ts
npm run conformance
```

It prints a per-section table and a total, which is `652/652 100.00%` across all
26 sections — that is the substantiation for "conformant to CommonMark 0.31.2",
and the spec suite it reads, `test/commonmark/spec.json`, is vendored in the
repository. Narrow it down when you are chasing one case:

```bash
node tools/conformance.mjs --section=Emphasis\ and\ strong\ emphasis
node tools/conformance.mjs --example=42
node tools/conformance.mjs --failures
node tools/conformance.mjs --json
```

The GFM extension corpus, `test/gfm/spec.json`, is vendored the same way and has
its own runner. It prints `24/24` over the five extension sections:

```bash
cd ts
npm run conformance-gfm
node tools/gfm-conformance.mjs --section=Tables
node tools/gfm-conformance.mjs --failures
```

The Go port runs the same 652 CommonMark examples and the same 24 GFM examples:

```bash
cd go
go test -run TestCommonMarkSpec -v ./...
go test -run TestGFMSpec -v ./...
```

The 75 shared AST fixtures in `test/spec/*.tsv` are asserted by both runtimes —
`npm test` (after `npm run build`) on the TypeScript side, `go test ./...` on
the Go side. They pin the AST through the plugin path; they are a regression
net, not a proof that the runtimes agree.

The claim that the runtimes agree rests on a wider comparison: all 652
CommonMark spec inputs under all four `gfm` × `breaks` combinations — 2608
records — with both the AST and the HTML compared on each, and 0 differences in
either; extending the same run to the 24 GFM examples makes it 676 inputs and
2704 records, again with none. Neither check covers `sourcepos`, which the AST
drops and the HTML does not encode.
