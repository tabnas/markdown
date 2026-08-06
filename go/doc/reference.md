# Reference: @tabnas/markdown (Go)

Complete, dry reference for the public API, every option, the map-based AST, the native
tree and the HTML output rules. For a guided introduction start with the
[tutorial](tutorial.md); for task recipes see the [how-to guide](guide.md); for design
rationale see [concepts](concepts.md).

## Outputs

The package has two documented outputs. The AST is the primary one; HTML is opt-in.

**AST.** `ParseDocument(src, opts)` returns a `map[string]any`. The HTML renderer does not
run.

```go
opts := tabnasmarkdown.DefaultOptions

doc := tabnasmarkdown.ParseDocument("# Hello\n\nHello *world*", opts)

fmt.Println(doc["type"])
// document
fmt.Println(doc["children"].([]any)[0])
// map[children:[map[type:text value:Hello]] depth:1 type:heading]
```

**HTML.** `ToHTML(src, opts)` returns a `string`. The CommonMark suite scores HTML output,
so this renderer is the instrument the 652/652 conformance figure is measured with.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("# Hello\n\nHello *world*", opts))
// "<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n"
```

**The HTML is not sanitized.** Raw HTML blocks and inline tags pass through verbatim, as
CommonMark specifies. GFM's disallowed-raw-HTML filter is not implemented.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("<script>alert(1)</script>", opts))
// "<script>alert(1)</script>\n"
```

A third output, `ParseTree(src, opts)`, returns the native CommonMark node tree that both
of the above are derived from.

## Module

```bash
go get github.com/tabnas/markdown/go@latest
```

```go
import tabnasmarkdown "github.com/tabnas/markdown/go"
```

| | |
|---|---|
| Module | `github.com/tabnas/markdown/go` |
| Package | `tabnasmarkdown` |
| `Version` | `"0.4.2"` |
| `go` directive | `1.24.7` |
| Requirements | `github.com/tabnas/parser/go v0.4.1` — the bare engine — and nothing else |
| Indirect requirements | none |
| License | MIT |

`go.mod` in full:

```
module github.com/tabnas/markdown/go

go 1.24.7

require github.com/tabnas/parser/go v0.4.1
```

## Files

| File | Contents | Engine |
|---|---|---|
| `markdown.go` | Plugin wiring, `Make`, `Defaults`, `Version`, the public parse entry points, the embedded grammar text. | Imports `github.com/tabnas/parser/go`. |
| `commonmark.go` | `Parse` — both phases. | None. |
| `block.go` | Phase 1: block structure. | None. |
| `inline.go` | Phase 2: inline structure. | None. |
| `ast.go` | `ToAST` — projection to the map-based AST. | None. |
| `html.go` | `RenderHTML`. | None. |
| `common.go` | Character classes, unescaping, label normalisation, URL and XML escaping, `IsEscapable`. | None. |
| `node.go` | `MdNode`, `NodeType`, `ListData`, `SourcePos`, `NodeWalker`. | None. |
| `options.go` | `Options`, `DefaultOptions`, `ResolveOptions`, `RefMap`, `RefDef`. | None. |

`markdown.go` is the only file that imports the engine; nothing reachable from
`commonmark.go` does. All files are one package, so the engine module remains a
compile-time requirement of the package as a whole. No engine instance is constructed on
the `Parse`, `ParseDocument`, `ParseInline`, `ToHTML`, `ParseTree`, `ToAST` or
`RenderHTML` paths.

## Exported symbols

| Symbol | Kind | Signature / value |
|---|---|---|
| `Markdown` | func | `func(j *parser.Tabnas, options map[string]any) error` — satisfies `parser.Plugin` |
| `Make` | func | `func(options ...map[string]any) *parser.Tabnas` |
| `Defaults` | var | `map[string]any{"gfm": true, "breaks": false}` |
| `Version` | const | `"0.4.2"` — untyped string |
| `Options` | type | `struct{ GFM bool; Breaks bool }` |
| `DefaultOptions` | var | `Options{GFM: true, Breaks: false}` |
| `ResolveOptions` | func | `func(opts map[string]any) Options` |
| `ParseDocument` | func | `func(src string, opts Options) map[string]any` |
| `ParseInline` | func | `func(text string, opts Options) []any` |
| `ToHTML` | func | `func(src string, opts Options) string` |
| `ParseTree` | func | `func(src string, opts Options) *MdNode` |
| `Parse` | func | `func(input string, opts Options) *MdNode` |
| `ToAST` | func | `func(doc *MdNode, opts Options) map[string]any` |
| `RenderHTML` | func | `func(doc *MdNode, opts Options) string` |
| `MdNode` | type | The native tree node, a struct. See [Native tree](#native-tree). |
| `NewNode` | func | `func(t NodeType) *MdNode` |
| `NodeType` | type | `string`. See [`NodeType`](#nodetype). |
| `ListType` | type | `string`, with constants `ListBullet` (`"bullet"`) and `ListOrdered` (`"ordered"`) |
| `ListData` | type | struct. See [`ListData`](#listdata). |
| `SourcePos` | type | `[2][2]int`. See [`SourcePos`](#sourcepos). |
| `NodeWalker` | type | struct with methods `Next`, `ResumeAt`. |
| `WalkEvent` | type | `struct{ Entering bool; Node *MdNode }` |
| `RefMap` | type | `map[string]RefDef`. See [Reference map types](#reference-map-types). |
| `RefDef` | type | `struct{ Destination string; Title string; HasTitle bool }` |
| `IsEscapable` | func | `func(b byte) bool` |

`parser` in the signatures above is `github.com/tabnas/parser/go`, whose package name is
`tabnas`.

The embedded grammar text is the unexported constant `grammarText`. The TypeScript package
exports its equivalent; this one does not.

### `ParseDocument(src, opts)`

Runs both parse phases and projects the result to the map-based AST. Returns
`map[string]any{"type": "document", "children": []any{…}}`. For `""` the `children` slice
is empty and non-nil. Never panics on Markdown input.

### `ParseInline(text, opts)`

Parses `text` as a document and returns the `children` of the first block if that block is
a `paragraph`, otherwise an empty, non-nil `[]any`.

```go
fmt.Println(tabnasmarkdown.ParseInline("a *b*", opts))
// [map[type:text value:a ] map[children:[map[type:text value:b]] type:emphasis]]
fmt.Println(tabnasmarkdown.ParseInline("# not a paragraph", opts))
// []
```

### `ToHTML(src, opts)`

Runs both parse phases and renders the native tree. Output ends with a newline whenever
the document has at least one block; for `""` the result is `""`. See
[HTML output](#html-output).

### `ParseTree(src, opts)`

Runs both parse phases and returns the root `*MdNode` (`Type` is `NodeDocument`) without
projecting or rendering. Identical to `Parse`. See [Native tree](#native-tree).

### `Parse(input, opts)`

The engine-free entry point. Runs `parseBlocks` then `parseInlines` and returns the root
`*MdNode`.

### `ToAST(doc, opts)`

Projects a tree that has been through both phases into the map-based AST.
`ToAST(Parse(src, opts), opts)` is exactly what `ParseDocument(src, opts)` does.

### `RenderHTML(doc, opts)`

Renders a tree that has been through both phases. `RenderHTML(Parse(src, opts), opts)` is
exactly what `ToHTML(src, opts)` does. Reads only `opts.Breaks`.

### `IsEscapable(b)`

Reports whether `b` is one of the 32 ASCII punctuation characters that may follow a
backslash (§2.1): `` !"#$%&'()*+,-./:;<=>?@[\]^_`{|}~ ``.

### `Markdown` (plugin)

`j.Use(Markdown, opts)` or `j.UseDefaults(Markdown, Defaults, opts)` applies the following
to the engine instance:

| Setting | Value |
|---|---|
| `Rule.Start` | `"markdown"` |
| `String.Lex` | `false` |
| `Comment.Lex` | `false` |
| `Number.Lex` | `false` |
| `Value.Lex` | `false` |
| `Lex.EmptyResult` | `map[string]any{"type": "document", "children": []any{}}` |

It defines one rule, `markdown`. The rule spec is cleared with `rs.Clear()` first, so
inherited `val` alts do not try to match a leading `#`. Its `BO` action reads the whole
source from `ctx.Src` and sets `r.Node = ParseDocument(src, opts)`. Its open alts are
`[{S: {{TinAA}}, R: "markdown"}, {}]` and its close alts are `[{S: {{TinZZ}}}, {}]`; these
consume the token stream so the engine's trailing-content check passes. The token stream
itself is not read by the parser.

The options map is passed through `ResolveOptions`, so `Use` and `UseDefaults` both fill
omitted keys from `DefaultOptions`. The return value is always `nil`.

`j.Parse(src)` returns `(any, error)`. For Markdown the value is always a `map[string]any`
document node, identical to `ParseDocument(src, opts)`.

### `Make(options...)`

Returns `parser.Make()` with `Markdown` installed, the counterpart of
`new Tabnas().use(Markdown)` in TypeScript. At most the first `options` map is used.

```go
j := tabnasmarkdown.Make()

result, err := j.Parse("# Hello")
fmt.Println(result, err)
// map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document] <nil>
```

## Options

`Options` is a struct, not a map. The zero value is `{GFM: false, Breaks: false}` — pure
CommonMark with strikethrough off.

```go
type Options struct {
	GFM    bool
	Breaks bool
}

var DefaultOptions = Options{GFM: true, Breaks: false}
```

| Field | Map key | Type | Default | Effect |
|---|---|---|---|---|
| `GFM` | `gfm` | `bool` | `true` | Enables GFM strikethrough (`~~text~~` → a `delete` node / `<del>`). Opening and closing runs must be the same length. Parse-time only. It gates nothing else. |
| `Breaks` | `breaks` | `bool` | `false` | When `true`, a soft line break becomes a `break` node in the AST and `<br />\n` in HTML. When `false`, a soft line break becomes a single space in the AST and `\n` in HTML. Hard line breaks (two or more trailing spaces, or a trailing backslash) are `break` nodes and `<br />` either way. |

`ResolveOptions(opts)` converts the plugin's option map to an `Options`. A `nil` map,
an unknown key, and a key whose value is not a `bool` all leave the corresponding default
in place.

```go
tabnasmarkdown.ResolveOptions(nil)                              // {GFM:true Breaks:false}
tabnasmarkdown.ResolveOptions(map[string]any{"gfm": false})     // {GFM:false Breaks:false}
tabnasmarkdown.ResolveOptions(map[string]any{"gfm": "no"})      // {GFM:true Breaks:false}
```

`RenderHTML` reads only `Breaks`; by the time it runs, `GFM` has already decided whether
`del` nodes exist.

### Reference map types

| Type | Definition |
|---|---|
| `RefDef` | `struct{ Destination string; Title string; HasTitle bool }` — `HasTitle` stands in for the TypeScript's `string \| null` |
| `RefMap` | `map[string]RefDef` — keys are link labels normalised by trimming, collapsing internal whitespace to one space, and case folding |

No exported function returns a `RefMap`; the block phase builds one and hands it to the
inline phase internally.

## AST

Every node is a `map[string]any`. Values are `string`, `int`, `bool`, `[]any`,
`map[string]any` or `nil`. There are no structs, no cycles, no parent pointers and no
positions. `document` is the root.

Two properties matter for `encoding/json` interop:

- **Optional values are present-and-`nil`, not absent.** `title`, `lang`, `meta` and
  `start` are always present as keys; when the value is absent the map holds an untyped
  `nil`, which marshals to JSON `null` and matches the TypeScript `string | null` /
  `number | null`.
- **Every `children` slice is non-nil.** An empty container holds `[]any{}`, which
  marshals to `[]` and never to `null`.

```go
doc := tabnasmarkdown.ParseDocument("", opts)
b, _ := json.Marshal(doc)
fmt.Println(string(b))
// {"children":[],"type":"document"}

doc = tabnasmarkdown.ParseDocument("    indented\n", opts)
b, _ = json.Marshal(doc)
fmt.Println(string(b))
// {"children":[{"lang":null,"meta":null,"type":"code","value":"indented"}],"type":"document"}
```

`encoding/json` emits map keys in sorted order. Map iteration order in Go is unspecified;
neither carries meaning.

### Document

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"document"` |
| `children` | `[]any` | Block nodes, in source order. Non-nil. |

### Block nodes

**`heading`** — ATX (`#`…`######`) and Setext (`===`, `---`).

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"heading"` |
| `depth` | `int` | 1–6. Setext produces 1 or 2 only. |
| `children` | `[]any` | Inline nodes. Non-nil. |

**`paragraph`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"paragraph"` |
| `children` | `[]any` | Inline nodes. Non-nil. |

**`blockquote`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"blockquote"` |
| `children` | `[]any` | Block nodes. Non-nil. |

**`list`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"list"` |
| `ordered` | `bool` | |
| `start` | `int` or `nil` | The start number for ordered lists; `nil` for bullet lists, marshalling to `null`. |
| `spread` | `bool` | `true` when the list is loose, i.e. any item is followed by a blank line or directly contains two blocks separated by one. mdast semantics. |
| `children` | `[]any` | `listItem` nodes only. Non-nil. |

**`listItem`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"listItem"` |
| `spread` | `bool` | `true` when two of the item's own children are separated by a blank line. Derived from the block phase's `SourcePos` line ranges. |
| `children` | `[]any` | Block nodes. Non-nil. |

**`code`** — fenced and indented code blocks.

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"code"` |
| `lang` | `string` or `nil` | First word of the info string; `nil` for indented code and for an empty info string. |
| `meta` | `string` or `nil` | Remainder of the info string after the first word, trimmed; `nil` when empty. |
| `value` | `string` | Content with exactly one trailing newline removed, if present. |

**`html`** — an HTML block. The same node type is also an inline node; see below.

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"html"` |
| `value` | `string` | Raw source, verbatim. |

**`thematicBreak`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"thematicBreak"`. No other keys. |

### Inline nodes

**`text`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"text"` |
| `value` | `string` | Backslash escapes and entity references already resolved. Adjacent text runs are merged into one node. |

**`emphasis`** (`*x*`, `_x_`)

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"emphasis"` |
| `children` | `[]any` | Non-nil. |

**`strong`** (`**x**`, `__x__`)

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"strong"` |
| `children` | `[]any` | Non-nil. |

**`inlineCode`** (code span)

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"inlineCode"` |
| `value` | `string` | Line endings converted to spaces; one leading and one trailing space stripped when the content is not all spaces. Escapes and entities are *not* resolved inside a code span. |

**`link`** — inline, reference, collapsed, shortcut and autolink forms all produce this
node.

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"link"` |
| `url` | `string` | Destination, backslash-unescaped and entity-decoded. Not percent-encoded — that happens at render time. An email autolink gets a `mailto:` prefix. |
| `title` | `string` or `nil` | `nil` when absent. |
| `children` | `[]any` | Non-nil. |

**`image`**

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"image"` |
| `url` | `string` | As for `link`. |
| `title` | `string` or `nil` | |
| `alt` | `string` | The description flattened to plain text: text, code-span and raw-HTML literals kept, markup wrappers dropped, line breaks as `\n`. There is no `children` key. |

**`break`** — a hard line break, or a soft one when `Breaks: true`.

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"break"`. No other keys. |

**`delete`** (`~~x~~`, only when `GFM: true`)

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"delete"` |
| `children` | `[]any` | Non-nil. |

**`html`** — a raw inline tag, comment, processing instruction, declaration or CDATA
section. One node per tag, not per element.

| Key | Go type | Notes |
|---|---|---|
| `type` | `string` | `"html"` |
| `value` | `string` | Raw source, verbatim. |

```go
fmt.Println(tabnasmarkdown.ParseDocument("Hello <b>bold</b>", opts))
// map[children:[map[children:[map[type:text value:Hello ] map[type:html value:<b>] map[type:text value:bold] map[type:html value:</b>]] type:paragraph]] type:document]
```

### What the AST does not carry

| Dropped | Where it survives |
|---|---|
| Source positions | `MdNode.SourcePos` on the native tree's block nodes. |
| Soft line breaks as nodes (with `Breaks: false`) | `NodeSoftbreak` nodes on the native tree. In the AST a soft break becomes a single space inside the surrounding text run. |
| The block/inline distinction for `html` | `NodeHTMLBlock` vs `NodeHTMLInline` on the native tree. |
| Container open/closed state, `StringContent` | Block-phase bookkeeping on the native tree; not meaningful after the parse finishes. |

## Native tree

`ParseTree(src, opts)` and `Parse(src, opts)` return the root `*MdNode`. It is a linked
tree: children are reached with `FirstChild`/`Next`, not a slice.

### `NodeType`

| Group | Constants | Values |
|---|---|---|
| Containers (block) | `NodeDocument`, `NodeBlockQuote`, `NodeList`, `NodeItem` | `document`, `block_quote`, `list`, `item` |
| Leaf blocks | `NodeParagraph`, `NodeHeading`, `NodeThematicBreak`, `NodeCodeBlock`, `NodeHTMLBlock` | `paragraph`, `heading`, `thematic_break`, `code_block`, `html_block` |
| Inlines | `NodeText`, `NodeSoftbreak`, `NodeLinebreak`, `NodeCode`, `NodeHTMLInline`, `NodeEmph`, `NodeStrong`, `NodeLink`, `NodeImage`, `NodeDel` | `text`, `softbreak`, `linebreak`, `code`, `html_inline`, `emph`, `strong`, `link`, `image`, `del` |

`IsContainer()` returns `true` for `document`, `block_quote`, `list`, `item`, `paragraph`,
`heading`, `emph`, `strong`, `link`, `image`, `del`.

### `MdNode` fields

| Field | Go type | Zero value | Set for |
|---|---|---|---|
| `Type` | `NodeType` | — | all |
| `Parent` | `*MdNode` | `nil` | all |
| `FirstChild` | `*MdNode` | `nil` | containers |
| `LastChild` | `*MdNode` | `nil` | containers |
| `Prev` | `*MdNode` | `nil` | all |
| `Next` | `*MdNode` | `nil` | all |
| `SourcePos` | `SourcePos` | `[[0,0],[0,0]]` | block nodes; inline nodes keep the zero value |
| `Literal` | `string` | `""` | `text`, `code`, `code_block`, `html_block`, `html_inline` |
| `Level` | `int` | `0` | `heading` (1–6) |
| `Destination` | `string` | `""` | `link`, `image` — entity-decoded and backslash-unescaped |
| `Title` | `string` | `""` | `link`, `image` |
| `HasTitle` | `bool` | `false` | `link`, `image` — distinguishes an absent title from an empty one |
| `Info` | `string` | `""` | fenced `code_block` — raw, entity-decoded |
| `HasInfo` | `bool` | `false` | `code_block` — `false` for indented code |
| `IsFenced` | `bool` | `false` | `code_block` |
| `FenceChar` | `byte` | `0` | fenced `code_block` — `` '`' `` or `'~'` |
| `FenceLength` | `int` | `0` | fenced `code_block` |
| `FenceOffset` | `int` | `0` | fenced `code_block` |
| `ListData` | `*ListData` | `nil` | `list`, `item` |
| `Open` | `bool` | `true` from `NewNode` | block-phase bookkeeping |
| `StringContent` | `[]byte` | `nil` | block-phase bookkeeping; consumed and cleared by phase 2 |
| `LastLineBlank` | `bool` | `false` | block-phase bookkeeping |
| `LastLineChecked` | `bool` | `false` | block-phase bookkeeping |

`HasTitle` and `HasInfo` are the Go stand-ins for the TypeScript's nullable `title` and
`info`; there is no other difference in field set.

### `MdNode` methods

| Method | Signature | Effect |
|---|---|---|
| `IsContainer` | `() bool` | Whether the node may hold children. |
| `AppendChild` | `(child *MdNode)` | Unlinks `child`, then adds it last. |
| `PrependChild` | `(child *MdNode)` | Unlinks `child`, then adds it first. |
| `InsertAfter` | `(sibling *MdNode)` | Unlinks `sibling`, then places it after the receiver. |
| `InsertBefore` | `(sibling *MdNode)` | Unlinks `sibling`, then places it before the receiver. |
| `Unlink` | `()` | Detaches from parent and siblings; the node itself stays intact. |
| `Walker` | `() *NodeWalker` | A depth-first walker rooted at this node. |

`NewNode(t NodeType) *MdNode` returns an open node of type `t` with every other field at
its zero value.

### `SourcePos`

```go
type SourcePos [2][2]int // [[startLine, startCol], [endLine, endCol]], 1-based
```

Columns count **characters**, not bytes: `é *x*` and `😀 *x*` both end at column 5.

### `ListData`

```go
type ListType string

const (
	ListBullet  ListType = "bullet"
	ListOrdered ListType = "ordered"
)
```

| Field | Go type | Meaning |
|---|---|---|
| `Type` | `ListType` | |
| `Tight` | `bool` | Decided at list finalize. Tight lists omit the `<p>` wrapper around item paragraphs. |
| `Start` | `int` | Ordered-list start number; ignored for bullet lists. |
| `Delimiter` | `string` | `"."` or `")"`. A change starts a new list. |
| `BulletChar` | `string` | `"-"`, `"+"` or `"*"`. A change starts a new list. |
| `Padding` | `int` | Columns between the marker and the item content. |
| `MarkerOffset` | `int` | Column at which the marker starts. |

A `list` and each of its `item` children point at the same `*ListData`.

### `NodeWalker`

| Member | Signature | Behaviour |
|---|---|---|
| `Next` | `() *WalkEvent` | Returns the next event, or `nil` when the walk is exhausted. |
| `ResumeAt` | `(node *MdNode, entering bool)` | Repositions the walk. |

```go
type WalkEvent struct {
	Entering bool
	Node     *MdNode
}
```

Containers produce two events — `Entering: true` on the way down, `Entering: false` on the
way up. Leaves produce one event, with `Entering: true`. For `- a` the event sequence is:
`+document +list +item +paragraph +text -paragraph -item -list -document`.

## HTML output

`ToHTML(src, opts)` and `RenderHTML(tree, opts)` produce the same string for the same
tree.

| Node | Output |
|---|---|
| `document` | nothing |
| `paragraph` | `<p>` … `</p>`, unless the tight-list rule applies |
| `heading` | `<hN>` … `</hN>`, `N` = `Level` |
| `thematic_break` | `<hr />` |
| `block_quote` | `<blockquote>` … `</blockquote>` |
| `list` | `<ul>` … `</ul>`, or `<ol>` … `</ol>`; `start="N"` is added when the list is ordered and `Start != 1` (including `start="0"`) |
| `item` | `<li>` … `</li>` |
| `code_block` | `<pre><code>` … `</code></pre>`; `class="language-X"` is added when the info string has a first word, `X` |
| `html_block` | `Literal`, verbatim and unescaped |
| `text` | `Literal`, XML-escaped |
| `softbreak` | `\n`, or `<br />\n` when `Breaks: true` |
| `linebreak` | `<br />\n` |
| `code` | `<code>` + XML-escaped `Literal` + `</code>` |
| `html_inline` | `Literal`, verbatim and unescaped |
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

A `paragraph` whose grandparent is a `list` with `ListData.Tight == true` emits no `<p>`
tags; its inline content is written directly. Such a paragraph is necessarily an item's
direct child.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("- a\n- b", opts))
// "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("- a\n\n- b", opts))
// "<ul>\n<li>\n<p>a</p>\n</li>\n<li>\n<p>b</p>\n</li>\n</ul>\n"
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
`0-9`, `A-Z`, `a-z` and `` ;/?:@&=+$,-_.!~*'()# ``. An existing `%XX` triplet with two hex
digits is preserved as-is, so a pre-encoded URL is not double-encoded. Everything else is
encoded as UTF-8, byte by byte. A byte that is not part of a valid UTF-8 sequence yields
`%EF%BF%BD`.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("[l](/a%20b?c=d&e)", opts))
// "<p><a href=\"/a%20b?c=d&amp;e\">l</a></p>\n"
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("[l](/ä)", opts))
// "<p><a href=\"/%C3%A4\">l</a></p>\n"
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
| Command | `go test -run TestCommonMarkSpec -v ./...` from `go/` |
| Options used | `Options{GFM: false, Breaks: false}` — the suite is pure CommonMark |
| TypeScript package | `npm run conformance` from `ts/`, also 652/652 |

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
between TypeScript and Go. The 36 shared AST fixtures in `test/spec/*.tsv` pass in both;
`go test -run TestSpec ./...` runs the Go half.

### GFM

| Extension | Status |
|---|---|
| Strikethrough (`~~x~~`) | Implemented, gated on `GFM` |
| Tables | Not implemented |
| Task list items | Not implemented |
| Autolink literals (bare `www.` / `https://` without angle brackets) | Not implemented |
| Footnotes | Not implemented |
| Disallowed raw HTML filtering | Not implemented |

`GFM: false` disables strikethrough and nothing else.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("~~x~~", tabnasmarkdown.Options{GFM: true}))
// "<p><del>x</del></p>\n"
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("~~x~~", tabnasmarkdown.Options{GFM: false}))
// "<p>~~x~~</p>\n"
```

## Grammar file

`markdown-grammar.jsonic` at the repository root is embedded verbatim into `go/markdown.go`
(as the unexported `grammarText`) and `ts/src/markdown.ts` by `ts/embed-grammar.js`,
between `BEGIN EMBEDDED` / `END EMBEDDED` markers.

It is inert. It declares one rule, `markdown`, with `open` and `close` alts of
`[{ s: '#ZZ' }]`. Block structure is decided by the line algorithm in `block.go`, not by
these alts. `grammarText` is a string constant that no code path parses — `markdown.go`
contains a `_ = grammarText` statement and nothing else reads it. The file is kept because
`embed-grammar.js` embeds it. The railroad diagram generated from it
(`ts/doc/grammar.svg`) is empty: a bare track with no boxes on it.

## Errors

The parser raises no errors for any input: per CommonMark every string is a valid
document, and malformed syntax becomes literal text. `ParseDocument`, `ParseInline`,
`ToHTML`, `ParseTree`, `Parse`, `ToAST` and `RenderHTML` have no error return and do not
panic; adversarial input is covered by `robust_test.go`.

`Markdown(j, options)` returns `error` to satisfy `parser.Plugin`; the value is always
`nil`. `j.Parse(src)` returns `(any, error)`; lex errors from a misconfigured engine
surface as the engine's error type, and the plugin disables the `string`, `comment`,
`number` and `value` lexers, which removes that class for Markdown's own syntax.

## Compatibility

The 0.4 `csv`-family API (`field`, `record`, `header`, `object`, and related options) is
not present in this package. Record parsing is available from
[`@tabnas/csv`](https://github.com/tabnas/csv). The RFC-4180 leftover
`BuildMarkdownStringMatcher` and the `test/fixtures/` CSV corpus have been removed.
