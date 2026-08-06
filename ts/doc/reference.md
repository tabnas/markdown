# Reference: @tabnas/markdown (TypeScript)

Complete, dry reference for the public API, every option, the AST, the native tree and the
HTML output rules. For a guided introduction start with the [tutorial](tutorial.md); for
task recipes see the [how-to guide](guide.md); for design rationale see
[concepts](concepts.md).

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
CommonMark specifies. GFM's disallowed-raw-HTML filter is not implemented.

```js
import { toHtml } from '@tabnas/markdown'

toHtml('<script>alert(1)</script>') // => '<script>alert(1)</script>\n'
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
| Version | 0.4.2 |
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

The parser modules — `block.ts`, `inline.ts`, `html.ts`, `ast.ts`, `common.ts`,
`node.ts`, `options.ts`, `entities.ts` — are all reachable from `commonmark.ts` and are
therefore all engine-free.

## Exports of `@tabnas/markdown`

```ts
import { Markdown, parseDocument, parseInline, toHtml, parseTree, MdNode, grammarText } from '@tabnas/markdown'
import type { MarkdownOptions, ParserOptions, DocumentNode, Block, Inline } from '@tabnas/markdown'
```

| Export | Kind | Signature / value |
|---|---|---|
| `Markdown` | `Plugin` | `(tn: Tabnas, options?: MarkdownOptions) => void` |
| `Markdown.defaults` | `MarkdownOptions` | `{ gfm: true, breaks: false }` |
| `parseDocument` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => DocumentNode` |
| `parseInline` | function | `(text: string, opts?: MarkdownOptions \| ParserOptions) => Inline[]` |
| `toHtml` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => string` |
| `parseTree` | function | `(src: string, opts?: MarkdownOptions \| ParserOptions) => MdNode` |
| `MdNode` | class | Re-export of `src/node.ts`. The native tree node. |
| `grammarText` | `string` | The embedded text of `markdown-grammar.jsonic`. |
| `MarkdownOptions` | type | See [Options](#options). |
| `ParserOptions` | type | Re-export of `src/options.ts`. See [Options](#options). |
| `DocumentNode`, `Block`, `HeadingNode`, `ParagraphNode`, `BlockquoteNode`, `ListNode`, `ListItemNode`, `CodeNode`, `HtmlNode`, `ThematicBreakNode` | types | See [Block nodes](#block-nodes). |
| `Inline`, `TextNode`, `EmphasisNode`, `StrongNode`, `InlineCodeNode`, `LinkNode`, `ImageNode`, `BreakNode`, `DeleteNode` | types | See [Inline nodes](#inline-nodes). |

### `parseDocument(src, opts?)`

Runs both parse phases and projects the result to the public AST. Returns
`{ type: 'document', children: Block[] }`. For `''` the `children` array is empty. Never
throws on Markdown input.

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
| `string.lex` | `false` |
| `comment.lex` | `false` |
| `number.lex` | `false` |
| `value.lex` | `false` |
| `lex.emptyResult` | `{ type: 'document', children: [] }` |

It defines one rule, `markdown`, whose `bo` action reads the whole source with
`ctx.src()` and sets `r.node = parseDocument(src, opts)`. Its `open` alts are
`[{ s: '#AA', r: 'markdown' }, {}]` and its `close` alts are `[{ s: '#ZZ' }, {}]`; these
consume the token stream so the engine's trailing-content check passes. The token stream
itself is not read by the parser.

`tn.parse(src)` therefore returns the same `DocumentNode` as `parseDocument(src, opts)`.

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
| `parse` | function | `(input: string, opts?: Partial<ParserOptions>) => MdNode` — both phases, returns the native tree. |
| `renderHTML` | function | `(doc: MdNode, options?: Partial<ParserOptions>) => string` |
| `parseBlocks` | function | `(input: string, options: ParserOptions) => { doc: MdNode, refmap: RefMap }` — phase 1 only. Paragraph and heading text is left raw in `stringContent`. |
| `parseInlines` | function | `(doc: MdNode, refmap: RefMap, options: ParserOptions) => void` — phase 2, in place. |
| `resolveOptions` | function | `(opts?: Partial<ParserOptions>) => ParserOptions` — fills omitted keys from `DEFAULT_OPTIONS`. |
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
| `gfm` | `boolean` | `true` | Enables GFM strikethrough (`~~text~~` → a `delete` node / `<del>`). Opening and closing runs must be the same length. Parse-time only. It gates nothing else. |
| `breaks` | `boolean` | `false` | When `true`, a soft line break becomes a `break` node in the AST and `<br />\n` in HTML. When `false`, a soft line break becomes a single space in the AST and `\n` in HTML. Hard line breaks (two or more trailing spaces, or a trailing backslash) are `break` nodes and `<br />` either way. |

`renderHTML` reads only `breaks`; by the time it runs, `gfm` has already decided whether
`del` nodes exist.

### Reference map types

| Type | Definition |
|---|---|
| `RefDef` | `{ destination: string; title: string \| null }` |
| `RefMap` | `Record<string, RefDef>` — keys are link labels normalised by trimming, collapsing internal whitespace to one space, and case folding (`toLowerCase().toUpperCase()`). |

## AST

All nodes are plain JSON: no classes, no cycles, no positions. `document` is the root.

```ts
type DocumentNode = { type: 'document'; children: Block[] }

type Block =
  | HeadingNode | ParagraphNode | BlockquoteNode | ListNode
  | ListItemNode | CodeNode | HtmlNode | ThematicBreakNode

type Inline =
  | TextNode | EmphasisNode | StrongNode | InlineCodeNode
  | LinkNode | ImageNode | BreakNode | DeleteNode | HtmlNode
```

### Block nodes

**`heading`** — ATX (`#`…`######`) and Setext (`===`, `---`).

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
| `spread` | `boolean` | `true` when the list is loose, i.e. any item is followed by a blank line or directly contains two blocks separated by one. mdast semantics. |
| `children` | `ListItemNode[]` | |

**`listItem`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'listItem'` | |
| `spread` | `boolean` | `true` when two of the item's own children are separated by a blank line. Derived from the block phase's `sourcepos` line ranges. |
| `children` | `Block[]` | |

**`code`** — fenced and indented code blocks.

| Field | Type | Notes |
|---|---|---|
| `type` | `'code'` | |
| `lang` | `string \| null` | First word of the info string; `null` for indented code and for an empty info string. |
| `meta` | `string \| null` | Remainder of the info string after the first word, trimmed; `null` when empty. |
| `value` | `string` | Content with exactly one trailing newline removed, if present. |

**`html`** — an HTML block. The same node type is also an inline node; see below.

| Field | Type | Notes |
|---|---|---|
| `type` | `'html'` | |
| `value` | `string` | Raw source, verbatim. |

**`thematicBreak`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'thematicBreak'` | No other fields. |

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

**`link`** — inline, reference, collapsed, shortcut and autolink forms all produce this node.

| Field | Type | Notes |
|---|---|---|
| `type` | `'link'` | |
| `url` | `string` | Destination, backslash-unescaped and entity-decoded. Not percent-encoded — that happens at render time. An email autolink gets a `mailto:` prefix. |
| `title` | `string \| null` | `null` when absent. |
| `children` | `Inline[]` | |

**`image`**

| Field | Type | Notes |
|---|---|---|
| `type` | `'image'` | |
| `url` | `string` | As for `link`. |
| `title` | `string \| null` | |
| `alt` | `string` | The description flattened to plain text: text, code-span and raw-HTML literals kept, markup wrappers dropped, line breaks as `\n`. There are no `children`. |

**`break`** — a hard line break, or a soft one when `breaks: true`.

| Field | Type | Notes |
|---|---|---|
| `type` | `'break'` | No other fields. |

**`delete`** (`~~x~~`, only when `gfm: true`)

| Field | Type | Notes |
|---|---|---|
| `type` | `'delete'` | |
| `children` | `Inline[]` | |

**`html`** — a raw inline tag, comment, processing instruction, declaration or CDATA
section. One node per tag, not per element.

| Field | Type | Notes |
|---|---|---|
| `type` | `'html'` | |
| `value` | `string` | Raw source, verbatim. |

```js
import { parseDocument } from '@tabnas/markdown'

parseDocument('Hello <b>bold</b>') // => { type: 'document', children: [ { type: 'paragraph', children: [ { type: 'text', value: 'Hello ' }, { type: 'html', value: '<b>' }, { type: 'text', value: 'bold' }, { type: 'html', value: '</b>' } ] } ] }
```

### What the AST does not carry

| Dropped | Where it survives |
|---|---|
| Source positions | `MdNode.sourcepos` on the native tree's block nodes. |
| Soft line breaks as nodes (with `breaks: false`) | `softbreak` nodes on the native tree. In the AST a soft break becomes a single space inside the surrounding text run. |
| The block/inline distinction for `html` | `html_block` vs `html_inline` on the native tree. |
| Container open/closed state, `stringContent` | Block-phase bookkeeping on the native tree; not meaningful after the parse finishes. |

## Native tree

`parseTree(src, opts?)` and `parse(src, opts?)` return the root `MdNode`. It is a linked
tree: children are reached with `firstChild`/`next`, not an array.

### `NodeType`

| Group | Values |
|---|---|
| Containers (block) | `document`, `block_quote`, `list`, `item` |
| Leaf blocks | `paragraph`, `heading`, `thematic_break`, `code_block`, `html_block` |
| Inlines | `text`, `softbreak`, `linebreak`, `code`, `html_inline`, `emph`, `strong`, `link`, `image`, `del` |

`isContainer()` returns `true` for `document`, `block_quote`, `list`, `item`, `paragraph`,
`heading`, `emph`, `strong`, `link`, `image`, `del`.

### `MdNode` fields

| Field | Type | Default | Set for |
|---|---|---|---|
| `type` | `NodeType` | — | all |
| `parent` | `MdNode \| null` | `null` | all |
| `firstChild` | `MdNode \| null` | `null` | containers |
| `lastChild` | `MdNode \| null` | `null` | containers |
| `prev` | `MdNode \| null` | `null` | all |
| `next` | `MdNode \| null` | `null` | all |
| `sourcepos` | `SourcePos` | `[[0,0],[0,0]]` | block nodes; inline nodes keep the default |
| `literal` | `string \| null` | `null` | `text`, `code`, `code_block`, `html_block`, `html_inline` |
| `level` | `number` | `0` | `heading` (1–6) |
| `destination` | `string \| null` | `null` | `link`, `image` — entity-decoded and backslash-unescaped |
| `title` | `string \| null` | `null` | `link`, `image` |
| `info` | `string \| null` | `null` | fenced `code_block`; `null` for indented |
| `isFenced` | `boolean` | `false` | `code_block` |
| `fenceChar` | `string` | `''` | fenced `code_block` |
| `fenceLength` | `number` | `0` | fenced `code_block` |
| `fenceOffset` | `number` | `0` | fenced `code_block` |
| `listData` | `ListData \| null` | `null` | `list`, `item` |
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

### `SourcePos` and `ListData`

```ts
type SourcePos = [[number, number], [number, number]] // [[startLine, startCol], [endLine, endCol]], 1-based
type ListType = 'bullet' | 'ordered'
```

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

Containers produce two events — `entering: true` on the way down, `entering: false` on
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

None is performed. `html_block` and `html_inline` literals are written verbatim. GFM's
disallowed-raw-HTML filter is not implemented. Untrusted Markdown requires a sanitizer
downstream of this renderer.

## Conformance

| | |
|---|---|
| Spec | CommonMark 0.31.2 |
| Result | **652/652** |
| Suite | `test/commonmark/spec.json`, vendored |
| Command | `npm run conformance` (no build step, no engine required) |
| Options used | `{ gfm: false, breaks: false }` — the suite is pure CommonMark |
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

Runtime parity is checked separately: 652 examples across 4 option combinations
(`gfm` × `breaks`) is 2608 records, with 0 differing ASTs and 0 differing HTML outputs
between TypeScript and Go. The 36 shared AST fixtures in `test/spec/*.tsv` pass in both.

### GFM

| Extension | Status |
|---|---|
| Strikethrough (`~~x~~`) | Implemented, gated on `gfm` |
| Tables | Not implemented |
| Task list items | Not implemented |
| Autolink literals (bare `www.` / `https://` without angle brackets) | Not implemented |
| Footnotes | Not implemented |
| Disallowed raw HTML filtering | Not implemented |

`gfm: false` disables strikethrough and nothing else.

## Grammar file

`markdown-grammar.jsonic` at the repository root is embedded verbatim into
`ts/src/markdown.ts` (as `grammarText`) and `go/markdown.go` by `ts/embed-grammar.js`,
between `BEGIN EMBEDDED` / `END EMBEDDED` markers.

It is inert. It declares one rule, `markdown`, with `open` and `close` alts of
`[{ s: '#ZZ' }]`. Block structure is decided by the line algorithm in `block.ts`, not by
these alts. `grammarText` is a string constant that no code path parses; the file is kept
because `embed-grammar.js` embeds it. The railroad diagram generated from it
(`ts/doc/grammar.svg`) is empty: a bare track with no boxes on it.

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
