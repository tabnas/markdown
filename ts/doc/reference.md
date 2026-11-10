# Reference: @tabnas/markdown (TypeScript)

Complete, dry reference for the public API, every option, the AST, the native tree, and the
HTML output rules. For a guided introduction start with the [tutorial](tutorial.md); for
task recipes see the [how-to guide](guide.md); for design rationale see
[concepts](concepts.md).

**Conformant to CommonMark 0.31.2.** 652/652 examples of the specification's own test
suite, all 26 sections, in this runtime and in the other two. The suite is vendored
at `test/commonmark/spec.json` and is run with `npm run conformance`. The five GFM
extensions score 24/24 on the vendored extension corpus `test/gfm/spec.json`, run with
`npm run conformance-gfm`. Full figures in [Conformance](#conformance).

## Outputs

The package has two documented outputs. The AST is the primary one; HTML is opt-in.

**AST.** `parseDocument(src, opts?)` returns a `DocumentNode`. The HTML renderer does not
run.

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('# Hello\n\nHello *world*')

doc.type // => 'document'
doc.children[0] // => { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] }
doc.children[1] // => { type: 'paragraph', children: [ { type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [ { type: 'text', value: 'world' } ] } ] }
```

**HTML.** `toHtml(src, opts?)` returns a string. The CommonMark suite scores HTML output,
so this renderer is the instrument the 652/652 conformance figure is measured with.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('# Hello\n\nHello *world*') // => '<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n'
```

**The HTML is not sanitized.** Raw HTML blocks and inline tags pass through verbatim, as
CommonMark specifies. The one exception is GFM's disallowed-raw-HTML filter, which is
applied when `gfm` is on and rewrites the leading `<` of nine tag names; it is not a
sanitizer. See [Sanitization](#sanitization).

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>') // => '&lt;script>alert(1)&lt;/script>\n'
toHtml('<script>alert(1)</script>', { gfm: false }) // => '<script>alert(1)</script>\n'
```

A third output, `parseTree(src, opts?)`, returns the native CommonMark node tree that both
of the above are derived from.

## Package

```bash
npm install @tabnas/markdown @tabnas/parser
```

| | |
|---|---|
| Package | `@tabnas/markdown` |
| Version | 0.5.1 |
| Module type | CommonJS (`main: dist/markdown.js`, `types: dist/markdown.d.ts`) |
| Peer dependencies | `@tabnas/parser` (`>=0`) |
| Runtime dependencies | none |
| Node | `>=24` |
| Published files | `src`, `dist`, `LICENSE` |
| License | MIT |

## Modules

| Module | Source | Built to | Engine |
|---|---|---|---|
| `@tabnas/markdown` | `src/markdown.ts` | `dist/markdown.js` | References `@tabnas/parser` for its `Tabnas`, `Plugin`, `Rule` and `Context` types only (`import type`). The `Markdown` plugin requires an engine instance at use time. |
| engine-free entry | `src/commonmark.ts` | `dist/commonmark.js` | None. Nothing reachable from this module imports `@tabnas/parser` in any form. |

`package.json` declares no `exports` map, so the engine-free module is reachable at
`@tabnas/markdown/dist/commonmark.js`.

The parser modules (`block.ts`, `inline.ts`, `html.ts`, `ast.ts`, `common.ts`,
`node.ts`, `options.ts`, `entities.ts`) are all reachable from `commonmark.ts` and are
therefore all engine-free.

## Exports of `@tabnas/markdown`

```ts
import { Markdown, make, parseDocument, parseInline, resolveReferences, toHtml, parseTree, MdNode, VERSION } from '@tabnas/markdown'
import type { MarkdownOptions, ParserOptions, DocumentNode, Block, Inline } from '@tabnas/markdown'
```

| Export | Kind | Signature / value |
|---|---|---|
| `Markdown` | `Plugin` | `(tn: Tabnas, options?: MarkdownOptions) => void` |
| `Markdown.defaults` | `MarkdownOptions` | `{ gfm: true, breaks: false }` |
| `make` | function | `(opts?: MarkdownOptions) => Tabnas`, an engine with the plugin installed and `lex.window` declared. |
| `parseDocument` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => DocumentNode` |
| `resolveReferences` | function | `(doc: DocumentNode) => DocumentNode`, the tree with its references resolved. |
| `parseInline` | function | `(text: string, opts?: MarkdownOptions \| ParserOptions) => Inline[]` |
| `toHtml` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => string` |
| `parseTree` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => MdNode` |
| `MdNode` | class | Re-export of `src/node.ts`. The native tree node. |
| `VERSION` | `string` | This package's version, kept equal to `package.json` "version" by `test/version.test.ts`. Mirrors `VERSION` in `go/markdown.go`. |
| `MarkdownOptions` | type | See [Options](#options). |
| `ParserOptions` | type | Re-export of `src/options.ts`. See [Options](#options). |
| `DocumentNode`, `Block`, `HeadingNode`, `ParagraphNode`, `BlockquoteNode`, `ListNode`, `ListItemNode`, `CodeNode`, `HtmlNode`, `ThematicBreakNode`, `DefinitionNode`, `TableNode`, `TableRowNode`, `TableCellNode` | types | See [Block nodes](#block-nodes) and [Table nodes](#table-nodes). |
| `Inline`, `TextNode`, `EmphasisNode`, `StrongNode`, `InlineCodeNode`, `LinkNode`, `ImageNode`, `LinkReferenceNode`, `ImageReferenceNode`, `BreakNode`, `DeleteNode` | types | See [Inline nodes](#inline-nodes). |

### `parseDocument(src, opts?)`

Runs both parse phases and projects the result to the public AST. Returns
`{ type: 'document', children: Block[] }`. For `''` the `children` array is empty. Never
throws on Markdown input. References stay unresolved: a bracketed label is a
`linkReference` or an `imageReference`, and each link reference definition is a
`definition` node where it stands. See [References](#references).

### `resolveReferences(doc)`

Returns a new tree with the references of `doc` resolved, as CommonMark reads the
document: a `linkReference` whose identifier a `definition` names becomes a `link`, an
`imageReference` an `image`, with the first definition of an identifier winning; one
with no definition becomes its source text, merged with the text beside it; and the
`definition` nodes are gone. `doc` is left as it was. Iterative over the tree's depth.

```js
import { parseDocument, resolveReferences } from '@tabnas/markdown'

const doc = parseDocument('[a] [b]\n\n[a]: /u')

resolveReferences(doc).children // => [ { type: 'paragraph', children: [ { type: 'link', url: '/u', title: null, children: [ { type: 'text', value: 'a' } ] }, { type: 'text', value: ' [b]' } ] } ]
```

### `make(opts?)`

Returns `new Tabnas().use(Markdown, opts)` with `lex.window` declared, so a parse from a
reader (`tn.parseReader`) holds the lines it has not finished with rather than the whole
source. The plugin itself leaves the option to the parser that installs it.

### `parseInline(text, opts?)`

Parses `text` as a document and returns the `children` of the first block if that block is
a `paragraph`, otherwise `[]`.

```js
import { parseInline } from '@tabnas/markdown'

parseInline('a *b* `c`') // => [ { type: 'text', value: 'a ' }, { type: 'emphasis', children: [ { type: 'text', value: 'b' } ] }, { type: 'text', value: ' ' }, { type: 'inlineCode', value: 'c' } ]
parseInline('# not a paragraph') // => []
```

### `toHtml(src, opts?)`

Runs both parse phases and renders the native tree. Output ends with a newline whenever
the document has at least one block. See [HTML output](#html-output).

### `parseTree(src, opts?)`

Runs both parse phases and returns the root `MdNode` (`type: 'document'`) without
projecting or rendering. See [Native tree](#native-tree).

### `Markdown` (plugin)

`tn.use(Markdown, options?)` applies the following to the engine instance:

| Setting | Value |
|---|---|
| `rule.start` | `'markdown'` |
| `space.lex`, `line.lex`, `string.lex`, `comment.lex`, `number.lex`, `value.lex`, `text.lex` | `false` |
| `lex.match.mdBlock` | The one matcher: cuts a physical line through the window of source the lexer holds and hands out the tokens of the blocks it decides. |
| `lex.emptyResult` | `{ type: 'document', children: [] }` |
| `parse.prepare['md-reset']` | Seeds a fresh block parser for each parse. |
| `rule.exclude` | `'gfm'` when `gfm` is `false`. |

The `mdBlock` matcher runs the block algorithm over each line. The blocks the line closes
and opens come back as tokens of no width at the line's start, ahead of the line's own
`#LB` token: a container's token (`#BQ` block quote, `#LS` list, `#LI` list item, `#PA`
paragraph, `#HD` heading, `#TB` table, `#TR` table row, `#TC` table cell, and the inline
containers) carries the fields before its `children`, and `#EN` the fields after them; a
leaf's token (`#HR`, `#CD`, `#HT`, `#DF`, and the inline leaves) carries the whole node.
A paragraph, a heading, and a table row are parsed inline as they close.

The rules are `markdown` (pushes `children`, closes on `#ZZ`), `children` (one node or
one `#LB` an iteration, going on by replacement, so a repetition adds no rule depth), and
one rule per node type, named for it (`blockquote`, `list`, `listItem`, `paragraph`,
`heading`, `table`, `tableRow`, `tableCell`, `emphasis`, `strong`, `delete`, `link`,
`linkReference`, `thematicBreak`, `code`, `html`, `definition`, `text`, `inlineCode`,
`break`, `image`, `imageReference`). A container's rule opens on its token, pushes
`children` and closes on `#EN`; a leaf's opens on its token and closes at once. The GFM
node rules' alternates on `children` carry the group `gfm`.

`tn.parse(src)` returns the same `DocumentNode` as `parseDocument(src, opts)`.

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const tn = new Tabnas().use(Markdown)

tn.parse('# Hello') // => { type: 'document', children: [ { type: 'heading', depth: 1, children: [ { type: 'text', value: 'Hello' } ] } ] }
```

The plugin also attaches a non-contract convenience object to the instance:

| Property | Signature |
|---|---|
| `tn.markdown.parseDocument` | `(t: string) => DocumentNode` |
| `tn.markdown.parseInline` | `(t: string) => Inline[]` |
| `tn.markdown.toHtml` | `(t: string) => string` |
| `tn.markdown.parseTree` | `(t: string) => MdNode` |

Each is bound to the options passed to `use()`.

## Exports of the engine-free module

```ts
import { parse, renderHTML, parseBlocks, parseInlines, resolveOptions, DEFAULT_OPTIONS, MdNode } from '@tabnas/markdown/dist/commonmark.js'
import type { ParserOptions, RefMap, RefDef } from '@tabnas/markdown/dist/commonmark.js'
```

| Export | Kind | Signature / value |
|---|---|---|
| `parse` | function | `(input: string, opts?: Partial<ParserOptions>, keepReferences?: boolean) => MdNode`, both phases, returns the native tree. With `keepReferences`, a bracketed label is a `link_reference` or `image_reference` node and each definition is kept, as `parseDocument` reads them. |
| `renderHTML` | function | `(doc: MdNode, options?: Partial<ParserOptions>) => string` |
| `parseBlocks` | function | `(input: string, options: ParserOptions) => { doc: MdNode, refmap: RefMap }`, phase 1 only. Paragraph and heading text is left raw in `stringContent`. |
| `parseInlines` | function | `(doc: MdNode, refmap: RefMap, options: ParserOptions, keepReferences?: boolean) => void`, phase 2, in place. |
| `resolveOptions` | function | `(opts?: Partial<ParserOptions>) => ParserOptions`, fills omitted keys from `DEFAULT_OPTIONS`. |
| `DEFAULT_OPTIONS` | `ParserOptions` | `{ gfm: true, breaks: false }` |
| `MdNode` | class | The native tree node. |
| `ParserOptions`, `RefMap`, `RefDef` | types | See below. |

`parse(src)` followed by `renderHTML(doc)` is exactly what `toHtml(src)` does. `parse(src)`
followed by the projection in `ast.ts` is exactly what `parseDocument(src)` does.

## Options

`MarkdownOptions` (all keys optional) and `ParserOptions` (all keys required) carry the
same two fields. `resolveOptions` converts the former to the latter.

```ts
type MarkdownOptions = { gfm?: boolean; breaks?: boolean }
type ParserOptions = { gfm: boolean; breaks: boolean }
```

| Option | Type | Default | Effect |
|---|---|---|---|
| `gfm` | `boolean` | `true` | Enables five GFM extensions together: tables (`table`, `tableRow`, `tableCell` nodes; a delimiter row under a paragraph's last line), strikethrough (`~~text~~` → a `delete` node / `<del>`, opening and closing runs the same length), task list items (`listItem.checked`), autolink literals (bare `www.` / `http://` / `https://` / `ftp://` / `a@b.co`), and the disallowed-raw-HTML filter. The first four are parse-time (tables in the block phase, task list markers in the block phase, strikethrough in the inline scanner, autolink literals in a post-pass over the inline tree) and the filter alone is applied by the renderer. |
| `breaks` | `boolean` | `false` | When `true`, a soft line break becomes a `break` node in the AST and `<br />\n` in HTML. When `false`, a soft line break becomes a single space in the AST and `\n` in HTML. Hard line breaks (two or more trailing spaces, or a trailing backslash) are `break` nodes and `<br />` either way. |

`renderHTML` reads `breaks`, and reads `gfm` for one thing only, the disallowed-raw-HTML
filter, which the extension defines at render time. Everything else `gfm` controls is
already settled in the tree by then. Called as `renderHTML(tree)` with no options at all,
it takes `gfm` from the parse that produced the tree, so a `gfm: false` parse renders as
plain CommonMark.

### Reference map types

| Type | Definition |
|---|---|
| `RefDef` | `{ destination: string; title: string \| null }` |
| `RefMap` | `Record<string, RefDef>`, whose keys are link labels normalised by trimming, collapsing internal whitespace to one space, and case folding (`toLowerCase().toUpperCase()`). |

## AST

All nodes are plain JSON: no classes, no cycles, no positions. `document` is the root.

```ts
type DocumentNode = { type: 'document'; children: Block[] }

type Block =
  | HeadingNode | ParagraphNode | BlockquoteNode | ListNode
  | ListItemNode | CodeNode | HtmlNode | ThematicBreakNode
  | TableNode | DefinitionNode

type Inline =
  | TextNode | EmphasisNode | StrongNode | InlineCodeNode
  | LinkNode | ImageNode | LinkReferenceNode | ImageReferenceNode
  | BreakNode | DeleteNode | HtmlNode
```

Fields are listed in the order the AST holds them, which is the order a stream writes
them: a field known only once a node's children are read follows `children`.

### Block nodes

**`heading`**. ATX (`#`…`######`) and Setext (`===`, `---`).

| Field | Type | Notes |
|---|---|---|
| `type` | `'heading'` | |
| `depth` | `1 \| 2 \| 3 \| 4 \| 5 \| 6` | Setext produces 1 or 2 only. |
| `children` | `Inline[]` | |

**`paragraph`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'paragraph'` | |
| `children` | `Inline[]` | |

**`blockquote`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'blockquote'` | |
| `children` | `Block[]` | |

**`list`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'list'` | |
| `ordered` | `boolean` | |
| `start` | `number \| null` | The start number for ordered lists; `null` for bullet lists. |
| `children` | `ListItemNode[]` | |
| `spread` | `boolean` | `true` when the list is loose, that is, any item is followed by a blank line or directly contains two blocks separated by one. mdast semantics. |

**`listItem`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'listItem'` | |
| `children` | `Block[]` | |
| `spread` | `boolean` | `true` when two of the item's own children are separated by a blank line. Derived from the block phase's `sourcepos` line ranges. |
| `checked` | `boolean \| null` | GFM task list item state: `true` for `- [x]`, `false` for `- [ ]`, `null` for an ordinary item, and so `null` on every item when `gfm` is off. mdast's field. |

**`code`**. Fenced and indented code blocks.

| Field | Type | Notes |
|---|---|---|
| `type` | `'code'` | |
| `lang` | `string \| null` | First word of the info string; `null` for indented code and for an empty info string. |
| `meta` | `string \| null` | Remainder of the info string after the first word, trimmed; `null` when empty. |
| `value` | `string` | Content with exactly one trailing newline removed, if present. |

**`definition`**. A link reference definition, where it stands among the blocks: in a
paragraph's place, or ahead of the paragraph or setext heading whose leading lines it
took. Every definition is a node, a label's second one too.

| Field | Type | Notes |
|---|---|---|
| `type` | `'definition'` | |
| `identifier` | `string` | The label normalized as CommonMark matches labels (white space collapsed and trimmed, case folded), in lower case. A reference with the same identifier names it. |
| `label` | `string` | The label's text after its escapes and character references are read. |
| `url` | `string` | As for `link`. |
| `title` | `string \| null` | `null` when absent. |

**`html`**. An HTML block. The same node type is also an inline node, described below.

| Field | Type | Notes |
|---|---|---|
| `type` | `'html'` | |
| `value` | `string` | Raw source, verbatim. |

**`thematicBreak`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'thematicBreak'` | No other fields. |

### Table nodes

Produced only when `gfm` is `true`. Three types, in mdast's shape. `table` is a `Block`;
`tableRow` appears only as a child of `table`, and `tableCell` only as a child of
`tableRow`.

```ts
type TableAlign = 'left' | 'right' | 'center' | null

type TableNode = { type: 'table'; align: TableAlign[]; children: TableRowNode[] }
type TableRowNode = { type: 'tableRow'; children: TableCellNode[] }
type TableCellNode = { type: 'tableCell'; children: Inline[] }
```

**`table`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'table'` | |
| `align` | `('left' \| 'right' \| 'center' \| null)[]` | One entry per column, in order, from the colons in the delimiter row: `:--` `'left'`, `--:` `'right'`, `:-:` `'center'`, `---` `null`. |
| `children` | `TableRowNode[]` | The first row is the header row. mdast has no header flag; the ordering carries it. A header-only table has exactly one child. |

**`tableRow`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'tableRow'` | |
| `children` | `TableCellNode[]` | Length always equals `table.align.length`: the block phase pads short rows with empty cells and truncates long ones. |

**`tableCell`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'tableCell'` | |
| `children` | `Inline[]` | Inline content, parsed as a paragraph's would be. Empty array for an empty or padded cell. A backslash-escaped pipe becomes a literal `\|` *before* inline parsing, so it does not split the cell and a code span containing it holds a real pipe. |

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('| a | b |\n| :- | -: |\n| 1 | 2 |\n').children[0] // => { type: 'table', align: ['left', 'right'], children: [ { type: 'tableRow', children: [ { type: 'tableCell', children: [ { type: 'text', value: 'a' } ] }, { type: 'tableCell', children: [ { type: 'text', value: 'b' } ] } ] }, { type: 'tableRow', children: [ { type: 'tableCell', children: [ { type: 'text', value: '1' } ] }, { type: 'tableCell', children: [ { type: 'text', value: '2' } ] } ] } ] }
```

Recognition rules:

| Rule | Detail |
|---|---|
| Start | A delimiter row on the line directly after an open paragraph whose **last line** splits into the same number of cells. A paragraph with earlier lines is split: those lines stay a paragraph and the table begins after them. |
| Delimiter cell | `-+` with an optional leading and/or trailing `:`, and nothing else. |
| Pipes | A leading and a trailing pipe are both optional on every row, and may differ between rows. A backslash-escaped pipe is content, not a delimiter. |
| End | A blank line, or the start of any other block. |
| Indentation | A delimiter row indented four or more columns does not open a table; it continues the paragraph. |
| `gfm: false` | No `table` node is produced; the lines remain a paragraph. |

### Inline nodes

**`text`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'text'` | |
| `value` | `string` | Backslash escapes and entity references already resolved. Adjacent text runs are merged into one node. |

**`emphasis`** (`*x*`, `_x_`)

| Field | Type | Notes |
|---|---|---|
| `type` | `'emphasis'` | |
| `children` | `Inline[]` | |

**`strong`** (`**x**`, `__x__`)

| Field | Type | Notes |
|---|---|---|
| `type` | `'strong'` | |
| `children` | `Inline[]` | |

**`inlineCode`** (code span)

| Field | Type | Notes |
|---|---|---|
| `type` | `'inlineCode'` | |
| `value` | `string` | Line endings converted to spaces; one leading and one trailing space stripped when the content is not all spaces. Escapes and entities are *not* resolved inside a code span. |

**`link`**. Inline links and autolinks produce this node; a full, collapsed, or shortcut
reference is a `linkReference`, and `resolveReferences` makes it a `link`.

| Field | Type | Notes |
|---|---|---|
| `type` | `'link'` | |
| `url` | `string` | Destination, backslash-unescaped and entity-decoded. Not percent-encoded: that happens at render time. An email autolink gets a `mailto:` prefix. |
| `title` | `string \| null` | `null` when absent. |
| `children` | `Inline[]` | |

**`image`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'image'` | |
| `url` | `string` | As for `link`. |
| `title` | `string \| null` | |
| `alt` | `string` | The description flattened to plain text: text, code-span and raw-HTML literals kept, markup wrappers dropped, line breaks as `\n`. There are no `children`. |

**`linkReference`** (`[text][label]`, `[label][]`, `[label]`). Whether or not the document
defines the label.

| Field | Type | Notes |
|---|---|---|
| `type` | `'linkReference'` | |
| `identifier` | `string` | As for `definition`. |
| `label` | `string` | The label's text: the second bracket's for a full reference, and the link text's for the others. |
| `referenceType` | `'full' \| 'collapsed' \| 'shortcut'` | |
| `children` | `Inline[]` | |

**`imageReference`** (`![alt][label]`, `![label][]`, `![label]`)

| Field | Type | Notes |
|---|---|---|
| `type` | `'imageReference'` | |
| `identifier` | `string` | As for `definition`. |
| `label` | `string` | As for `linkReference`. |
| `referenceType` | `'full' \| 'collapsed' \| 'shortcut'` | |
| `alt` | `string` | As for `image`. |

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('![a *b*][Foo  Bar] [x][]').children[0].children[0] // => { type: 'imageReference', identifier: 'foo bar', label: 'Foo  Bar', referenceType: 'full', alt: 'a b' }
parseDocument('![a *b*][Foo  Bar] [x][]').children[0].children[2] // => { type: 'linkReference', identifier: 'x', label: 'x', referenceType: 'collapsed', children: [ { type: 'text', value: 'x' } ] }
```

**`break`**. A hard line break, or a soft one when `breaks: true`.

| Field | Type | Notes |
|---|---|---|
| `type` | `'break'` | No other fields. |

**`delete`** (`~~x~~`, only when `gfm: true`)

| Field | Type | Notes |
|---|---|---|
| `type` | `'delete'` | |
| `children` | `Inline[]` | |

**`html`**. A raw inline tag, comment, processing instruction, declaration, or CDATA
section. One node per tag, not per element.

| Field | Type | Notes |
|---|---|---|
| `type` | `'html'` | |
| `value` | `string` | Raw source, verbatim. |

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('Hello <b>bold</b>') // => { type: 'document', children: [ { type: 'paragraph', children: [ { type: 'text', value: 'Hello ' }, { type: 'html', value: '<b>' }, { type: 'text', value: 'bold' }, { type: 'html', value: '</b>' } ] } ] }
```

### References

The AST keeps references unresolved, as mdast does, so a block's tree depends on nothing
after it: a bracketed label that could be a reference is a `linkReference` or an
`imageReference`, and a definition is a `definition` node. `resolveReferences` gives the
tree CommonMark reads for every example of both corpora but five CommonMark examples
(512, 523, 528, 569, and 571), where a label with no definition decided how the text
around it nests: the label reads as a reference, so emphasis cannot cross it, a link's
text that holds it is no link, and `[foo][bar][baz]` reads as `[foo][bar]` and then
`[baz]`. Under `gfm: true` a task list marker is read before the paragraph is parsed, so
`- [x] done` is a task item whether or not `x` is defined. The HTML output and the native
tree from `parseTree` resolve as CommonMark does.

### What the AST does not carry

| Dropped | Where it survives |
|---|---|
| Source positions | `MdNode.sourcepos` on the native tree's block nodes. |
| Soft line breaks as nodes (with `breaks: false`) | `softbreak` nodes on the native tree. In the AST a soft break becomes a single space inside the surrounding text run. |
| The block/inline distinction for `html` | `html_block` vs `html_inline` on the native tree. |
| The table header-row flag | `MdNode.isHeaderRow` on the native tree. In the AST the first `tableRow` is the header row, by mdast convention. |
| Container open/closed state, `stringContent` | Block-phase bookkeeping on the native tree; not meaningful after the parse finishes. |

## Native tree

`parseTree(src, opts?)` and `parse(src, opts?)` return the root `MdNode`. It is a linked
tree: children are reached with `firstChild`/`next`, not an array.

### `NodeType`

| Group | Values |
|---|---|
| Containers (block) | `document`, `block_quote`, `list`, `item` |
| Leaf blocks | `paragraph`, `heading`, `thematic_break`, `code_block`, `html_block` |
| GFM tables | `table`, `table_row`, `table_cell` |
| Inlines | `text`, `softbreak`, `linebreak`, `code`, `html_inline`, `emph`, `strong`, `link`, `image`, `del` |

`isContainer()` returns `true` for `document`, `block_quote`, `list`, `item`, `paragraph`,
`heading`, `table`, `table_row`, `table_cell`, `emph`, `strong`, `link`, `image`, `del`.

A `table` accepts lines like a leaf block (no block-level element can be placed inside
one) but is a container in the tree: its children are `table_row` nodes, whose children
are `table_cell` nodes, whose children are inlines. `table` nodes appear only when `gfm`
is `true`.

### `MdNode` fields

| Field | Type | Default | Set for |
|---|---|---|---|
| `type` | `NodeType` | (none) | all |
| `parent` | `MdNode \| null` | `null` | all |
| `firstChild` | `MdNode \| null` | `null` | containers |
| `lastChild` | `MdNode \| null` | `null` | containers |
| `prev` | `MdNode \| null` | `null` | all |
| `next` | `MdNode \| null` | `null` | all |
| `sourcepos` | `SourcePos` | `[[0,0],[0,0]]` | block nodes; inline nodes keep the default |
| `literal` | `string \| null` | `null` | `text`, `code`, `code_block`, `html_block`, `html_inline` |
| `level` | `number` | `0` | `heading` (1–6) |
| `destination` | `string \| null` | `null` | `link`, `image`, entity-decoded and backslash-unescaped |
| `title` | `string \| null` | `null` | `link`, `image` |
| `info` | `string \| null` | `null` | fenced `code_block`; `null` for indented |
| `isFenced` | `boolean` | `false` | `code_block` |
| `fenceChar` | `string` | `''` | fenced `code_block` |
| `fenceLength` | `number` | `0` | fenced `code_block` |
| `fenceOffset` | `number` | `0` | fenced `code_block` |
| `listData` | `ListData \| null` | `null` | `list`, `item` |
| `tableAlign` | `TableAlign[] \| null` | `null` | `table`: one entry per column, from the delimiter row; `null` on every other type |
| `isHeaderRow` | `boolean` | `false` | `table_row`: `true` on the table's first child only |
| `checked` | `boolean \| null` | `null` | `item`: GFM task list state; `null` on an ordinary item and on every item when `gfm` is off |
| `gfm` | `boolean` | `true` | `document`: the `gfm` option the tree was parsed with, read by `renderHTML` when it is called with no options |
| `open` | `boolean` | `true` | block-phase bookkeeping |
| `stringContent` | `string` | `''` | block-phase bookkeeping; consumed and cleared by phase 2 |
| `lastLineBlank` | `boolean` | `false` | block-phase bookkeeping |
| `lastLineChecked` | `boolean` | `false` | block-phase bookkeeping |

### `MdNode` methods

| Method | Signature | Effect |
|---|---|---|
| `isContainer` | `() => boolean` | Whether the node may hold children. |
| `appendChild` | `(child: MdNode) => void` | Unlinks `child`, then adds it last. |
| `prependChild` | `(child: MdNode) => void` | Unlinks `child`, then adds it first. |
| `insertAfter` | `(sibling: MdNode) => void` | Unlinks `sibling`, then places it after `this`. |
| `insertBefore` | `(sibling: MdNode) => void` | Unlinks `sibling`, then places it before `this`. |
| `unlink` | `() => void` | Detaches from parent and siblings; the node itself stays intact. |
| `walker` | `() => NodeWalker` | A depth-first walker rooted at this node. |

### `SourcePos`, `ListData` and `TableAlign`

```ts
type SourcePos = [[number, number], [number, number]] // [[startLine, startCol], [endLine, endCol]], 1-based
type ListType = 'bullet' | 'ordered'
type TableAlign = 'left' | 'right' | 'center' | null
```

`TableAlign` is declared in `src/node.ts` and is not re-exported from the package entry.

| `ListData` field | Type | Meaning |
|---|---|---|
| `type` | `ListType` | |
| `tight` | `boolean` | Decided at list finalize. Tight lists omit the `<p>` wrapper around item paragraphs. |
| `start` | `number` | Ordered-list start number; ignored for bullet lists. |
| `delimiter` | `string` | `'.'` or `')'`. A change starts a new list. |
| `bulletChar` | `string` | `'-'`, `'+'` or `'*'`. A change starts a new list. |
| `padding` | `number` | Columns between the marker and the item content. |
| `markerOffset` | `number` | Column at which the marker starts. |

### `NodeWalker`

| Member | Signature | Behaviour |
|---|---|---|
| `next` | `() => WalkEvent \| null` | Returns the next event, or `null` when the walk is exhausted. |
| `resumeAt` | `(node: MdNode, entering: boolean) => void` | Repositions the walk. |

```ts
type WalkEvent = { entering: boolean; node: MdNode }
```

Containers produce two events: `entering: true` on the way down, `entering: false` on
the way up. Leaves produce one event, with `entering: true`. For `- a` the event sequence
is: `+document +list +item +paragraph +text -paragraph -item -list -document`.

## HTML output

`toHtml(src, opts?)` and `renderHTML(tree, opts?)` produce the same string for the same
tree.

| Node | Output |
|---|---|
| `document` | nothing |
| `paragraph` | `<p>` … `</p>`, unless the tight-list rule applies |
| `heading` | `<hN>` … `</hN>`, `N` = `level` |
| `thematic_break` | `<hr />` |
| `block_quote` | `<blockquote>` … `</blockquote>` |
| `list` | `<ul>` … `</ul>`, or `<ol>` … `</ol>`; `start="N"` is added when the list is ordered and `start !== 1` (including `start="0"`) |
| `item` | `<li>` … `</li>` |
| `code_block` | `<pre><code>` … `</code></pre>`; `class="language-X"` is added when the info string has a first word, `X` |
| `html_block` | `literal`, verbatim and unescaped |
| `table` | `<table>` … `</table>` |
| `table_row` | `<tr>` … `</tr>`, wrapped in `<thead>` … `</thead>` when `isHeaderRow`, and in `<tbody>` … `</tbody>` for the run of rows after it. `<tbody>` is written by the first body row, so a header-only table emits no `<tbody>` at all |
| `table_cell` | `<th>` … `</th>` in the header row, `<td>` … `</td>` elsewhere; `align="left"`, `align="right"` or `align="center"` is added when the column's `tableAlign` entry is not `null` |
| `text` | `literal`, XML-escaped |
| `softbreak` | `\n`, or `<br />\n` when `breaks: true` |
| `linebreak` | `<br />\n` |
| `code` | `<code>` + XML-escaped `literal` + `</code>` |
| `html_inline` | `literal`, verbatim and unescaped |
| `emph` | `<em>` … `</em>` |
| `strong` | `<strong>` … `</strong>` |
| `del` | `<del>` … `</del>` |
| `link` | `<a href="…">` … `</a>`; `title="…"` is added when the title is present and non-empty |
| `image` | `<img src="…" alt="…" />`; `title="…"` is added on the same condition. Children are flattened into `alt` and are not rendered as markup |

### Newlines

Every block writes a newline before its opening tag and after its closing tag, where
"writes a newline" is a no-op if the output is already at the start of a line. No block
emits a newline on behalf of a neighbour or a child. `<li>` deliberately does not end a
line.

### The tight-list paragraph rule

A `paragraph` whose grandparent is a `list` with `listData.tight === true` emits no `<p>`
tags; its inline content is written directly. Such a paragraph is necessarily an item's
direct child.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('- a\n- b') // => '<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n'
toHtml('- a\n\n- b') // => '<ul>\n<li>\n<p>a</p>\n</li>\n<li>\n<p>b</p>\n</li>\n</ul>\n'
```

### Table output

```js
import { toHtml } from '@tabnas/markdown'

toHtml('| a | b |\n| :- | -: |\n| 1 | 2 |\n') // => '<table>\n<thead>\n<tr>\n<th align="left">a</th>\n<th align="right">b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td align="left">1</td>\n<td align="right">2</td>\n</tr>\n</tbody>\n</table>\n'
toHtml('| a | b |\n| - | - |\n') // => '<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n'
```

### Escaping

Applied to text content and to attribute values identically.

| Character | Replacement |
|---|---|
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |

`'` is not escaped.

### URL encoding

`href` and `src` values are percent-encoded before escaping. Unreserved characters are
`0-9`, `A-Z`, `a-z` and `;/?:@&=+$,-_.!~*'()#`. An existing `%XX` triplet with two hex
digits is preserved as-is, so a pre-encoded URL is not double-encoded. Everything else is
encoded as UTF-8 by code point, so an astral character yields one four-byte sequence. An
unpaired surrogate yields `%EF%BF%BD`.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('[l](/a%20b?c=d&e)') // => '<p><a href="/a%20b?c=d&amp;e">l</a></p>\n'
toHtml('[l](/ä)') // => '<p><a href="/%C3%A4">l</a></p>\n'
```

### Sanitization

None is performed. `html_block` and `html_inline` literals are written verbatim, apart
from GFM's disallowed-raw-HTML filter: with `gfm` on, the leading `<` of `title`,
`textarea`, `style`, `xmp`, `iframe`, `noembed`, `noframes`, `script` and `plaintext`,
opening or closing, any case, followed by whitespace, `/`, `>` or the end of the text, is
written as `&lt;`. That is nine tag names; every other tag, every attribute, and every link
destination is untouched. Untrusted Markdown requires a sanitizer downstream of this
renderer.

## Conformance

**Conformant to CommonMark 0.31.2.**

| | |
|---|---|
| Spec | CommonMark 0.31.2 |
| Result | **652/652**, all 26 sections |
| Suite | `test/commonmark/spec.json`, vendored |
| Command | `npm run conformance` (no build step, no engine required) |
| Options used | `{ gfm: false, breaks: false }`, so the suite is pure CommonMark |
| Go port | `cd go && go test -run TestCommonMarkSpec -v ./...`, also 652/652 |

All 26 sections pass.

| Section | Examples | Section | Examples |
|---|---|---|---|
| Tabs | 11 | Block quotes | 25 |
| Backslash escapes | 13 | List items | 48 |
| Entity and numeric character references | 17 | Lists | 26 |
| Precedence | 1 | Inlines | 1 |
| Thematic breaks | 19 | Code spans | 22 |
| ATX headings | 18 | Emphasis and strong emphasis | 132 |
| Setext headings | 27 | Links | 90 |
| Indented code blocks | 12 | Images | 22 |
| Fenced code blocks | 29 | Autolinks | 19 |
| HTML blocks | 44 | Raw HTML | 20 |
| Link reference definitions | 27 | Hard line breaks | 15 |
| Paragraphs | 8 | Soft line breaks | 2 |
| Blank lines | 1 | Textual content | 3 |

Runtime parity is checked separately: 676 examples (652 CommonMark + 24 GFM) across 4
option combinations (`gfm` × `breaks`) is 2704 records, with 0 differing ASTs and 0
differing HTML outputs between TypeScript and Go. The 75 shared AST fixtures in
`test/spec/*.tsv` pass in every runtime.

### GFM

The extension set is complete: **24/24** on the vendored GFM extension corpus.

| | |
|---|---|
| Result | **24/24**, all 5 extension sections |
| Suite | `test/gfm/spec.json`, vendored: the extension sections of the GFM spec only |
| Command | `npm run conformance-gfm` |
| Options used | `{ gfm: true, breaks: false }` |
| Go port | `cd go && go test -run TestGFMSpec -v ./...`, also 24/24 |

| Section | Examples |
|---|---|
| Tables (extension) | 8 |
| Task list items (extension) | 2 |
| Strikethrough (extension) | 2 |
| Autolinks (extension) | 11 |
| Disallowed Raw HTML (extension) | 1 |

| Extension | Status | Phase |
|---|---|---|
| Tables (`table`, `tableRow`, `tableCell`) | Implemented, gated on `gfm` | Block phase |
| Task list items (`- [x] done`) | Implemented, gated on `gfm` | Block phase |
| Strikethrough (`~~x~~`, `~x~`) | Implemented, gated on `gfm` | Inline scanner |
| Autolink literals (bare `www.` / `http://` / `https://` / `ftp://` / `a@b.co`) | Implemented, gated on `gfm` | Post-pass over the inline tree |
| Disallowed raw HTML filtering | Implemented, gated on `gfm` | Renderer |
| Footnotes | Not implemented: a GitHub product feature, not a section of the GFM spec suite | (none) |

`gfm: false` disables all five together, and the output is then plain CommonMark,
byte-identical to a pure-CommonMark parse over 1430 checked records. All three runtimes
implement all five.

**Tables.** A delimiter row on the line directly after an open paragraph whose last line
splits into the same number of cells opens a table; earlier lines of that paragraph are
split off and stay a paragraph. A delimiter cell is hyphens with an optional leading
and/or trailing colon. Leading and trailing pipes are optional and may differ between
rows. A backslash-escaped pipe is resolved to a literal `|` before inline parsing, so it
does not split a cell and a code span sees a raw pipe. Every row has exactly as many cells
as the delimiter row had columns: short rows are padded, long ones truncated. The table
ends at a blank line or at the start of another block. See
[Table nodes](#table-nodes) and [Table output](#table-output).

**Task list items.** A list item whose first block is a paragraph starting with a task
list item marker (optional spaces, `[`, a space/tab or `x`/`X`, `]`, then at least one
space or tab) is a task list item. The marker is consumed, `listItem.checked` becomes
`true`/`false`, and the renderer writes `<input disabled="" type="checkbox"> ` (plus
`checked=""` when checked) at the head of that paragraph.

**Autolink literals.** Recognised at the start of a text run, after whitespace, or after
`*`, `_`, `~` or `(`. `www.` gets `http://` prepended and an address gets `mailto:`. A
domain is segments of alphanumerics, `_` and `-` separated by `.`, with at least one `.`
and no `_` in either of the last two segments; trailing `?`, `!`, `.`, `,`, `:`, `*`,
`_`, `~`, unbalanced `)` and a trailing entity-like `&…;` are excluded from the link.
Never produced inside a link, a code span, raw HTML, or an image description.

**Disallowed raw HTML.** In `html` block and inline output the leading `<` of `title`,
`textarea`, `style`, `xmp`, `iframe`, `noembed`, `noframes`, `script` and `plaintext`,
opening or closing, any case, is written as `&lt;`. The node's `value` keeps the
original text; only the rendered HTML changes. It escapes those nine tag names and nothing
else: it is not a sanitizer. See [Sanitization](#sanitization).

### Not implemented

**Footnotes.** A GitHub product feature, not part of the GFM specification suite. There is
no option that enables them. `[^1]` is a valid CommonMark link label, so a
GitHub-authored footnote does not error. It renders as a broken link. With no matching
definition the reference stays literal text and the definition line becomes a paragraph;
if the definition body happens to parse as a link destination, the reference becomes a
real link.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('Text[^1]\n\n[^1]: Note text here.\n') // => '<p>Text[^1]</p>\n<p>[^1]: Note text here.</p>\n'
toHtml('Text[^1]\n\n[^1]: /note\n') // => '<p>Text<a href="/note">^1</a></p>\n'
```

**Single-tilde subscript.** GFM strikethrough accepts a single `~`, which collides with
the `H~2~O` subscript syntax of other dialects. Under `gfm: true`, `H~2~O` renders as
`H<del>2</del>O`.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('H~2~O\n') // => '<p>H<del>2</del>O</p>\n'
toHtml('H~2~O\n', { gfm: false }) // => '<p>H~2~O</p>\n'
```

**Outside CommonMark and GFM.** Not implemented, and not gated on `gfm`: math, front
matter, definition lists, heading attributes, admonitions, wiki links, emoji shortcodes,
highlight, sub/superscript. Each would need its own opt-in option.

## Grammar

The grammar is registered in code, not loaded from a file. The plugin instance
carries the `markdown` rule (pushes `children`; closes on `#ZZ`), the `children` rule
(one node or one `#LB` line an iteration, a replace loop; the GFM nodes' alternates
tagged `g:'gfm'`) and a rule for each node type. See [`Markdown`](#markdown-plugin).
The nested inline instance carries the `inline` rule over a twelve-token
alphabet produced by its matcher set (`engine-inline.ts`). The railroad
diagrams `ts/doc/grammar.svg` and `ts/doc/grammar-inline.svg` are generated
from live instances by `ts/tools/gen-railroad.mjs`.

## Errors

The parser raises no errors for any input: per CommonMark every string is a valid
document, and malformed syntax becomes literal text. Lex errors from a misconfigured
engine surface as `TabnasError`; the plugin disables the `string`, `comment`, `number` and
`value` lexers, which removes that class for Markdown's own syntax.

## Compatibility

The 0.4 `csv`-family API (`field`, `record`, `header`, `object`, and related options) is
not present in this package. Record parsing is available from `@tabnas/csv`. The
RFC-4180 leftover `BuildMarkdownStringMatcher` and the `test/fixtures/` CSV corpus have
been removed.
