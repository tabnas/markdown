# How-to guide: @tabnas/markdown (Go)

Task-oriented recipes. Each one is self-contained. If this is your first run,
start with the [tutorial](tutorial.md); for exact types and defaults, the
[reference](reference.md).

Two answers first, because most of these recipes depend on them:

- **The AST is the primary output.** `ParseDocument(src, opts)` returns it,
  without running the renderer and without loading the engine.
- **HTML output is available**: `ToHTML(src, opts)`. The CommonMark suite scores
  HTML, so the renderer is what makes the conformance result measurable; it is a
  first-class output, not a side utility. It is also **not sanitized** — see
  [Render untrusted Markdown safely](#render-untrusted-markdown-safely).

**The parser is conformant to CommonMark 0.31.2**: 652/652 examples of the
specification's own suite, all 26 sections, in both the Go and the TypeScript
runtime. The suite is vendored in this repository, so the claim is checkable —
`go test -run TestCommonMarkSpec -v ./...` runs it; see
[Check conformance yourself](#check-conformance-yourself).

On top of CommonMark it implements the complete set of five GFM extensions —
tables, task list items, autolink literals, strikethrough and the
disallowed-raw-HTML filter. That is 24/24 on the vendored GFM extension suite,
which `go test -run TestGFMSpec -v ./...` runs. All five are gated on the `GFM`
option, default `true`.

Every recipe assumes this import:

```go
import tabnasmarkdown "github.com/tabnas/markdown/go"
```

## Get just the AST, without the engine

Call `ParseDocument`. It takes a source string and an `Options` value, and
returns the JSON AST as a `map[string]any`.

```go
doc := tabnasmarkdown.ParseDocument("# Hello", tabnasmarkdown.DefaultOptions)

fmt.Println(doc)
// map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document]
```

Nothing on this path constructs an engine instance, and the HTML renderer is not
touched. `ParseInline(text, opts)` does the same for a single run of inline
Markdown, returning the `[]any` of inline nodes:

```go
inlines := tabnasmarkdown.ParseInline("a *b* c", tabnasmarkdown.DefaultOptions)

fmt.Println(inlines)
// [map[type:text value:a ] map[children:[map[type:text value:b]] type:emphasis] map[type:text value: c]]
```

The map is `encoding/json`-ready as it stands: `json.Marshal(doc)` produces the
same JSON the TypeScript package produces for the same input.

## Render HTML

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("# Hello\n\nHello *world*\n", tabnasmarkdown.DefaultOptions))
// "<h1>Hello</h1>\n<p>Hello <em>world</em></p>\n"
```

The output is byte-exact against the CommonMark 0.31.2 expected HTML — including
where the newlines fall, which is a correctness contract, not formatting.
`ToHTML` parses from source; it does not take an AST. To render something you
have already parsed and changed, see
[Transform a document, then render it](#transform-a-document-then-render-it).

The HTML is not sanitized. If any of the input came from someone else, read
[Render untrusted Markdown safely](#render-untrusted-markdown-safely) before you
ship it.

## Use the plugin on a tabnas engine

`Make` returns an engine with the plugin already installed:

```go
j := tabnasmarkdown.Make()

result, err := j.Parse("# Hello")
fmt.Println(result, err)
// map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document] <nil>
```

`Parse` returns `(any, error)`. For Markdown the value is always a
`map[string]any` document node, and it is exactly what `ParseDocument` returns
for the same input — verified by `reflect.DeepEqual`, not by eye. Errors come
from the engine, not from the Markdown, which has no syntax errors.

`Make` takes an optional plugin option map:

```go
j := tabnasmarkdown.Make(map[string]any{"breaks": true})

result, _ := j.Parse("a\nb")
fmt.Println(result)
// map[children:[map[children:[map[type:text value:a] map[type:break] map[type:text value:b]] type:paragraph]] type:document]
```

To install the plugin on an engine you already have, use `Use` for raw options
or `UseDefaults` to merge over `Defaults`:

```go
import parser "github.com/tabnas/parser/go"

j := parser.Make()
err := j.Use(tabnasmarkdown.Markdown, nil)

j2 := parser.Make()
err2 := j2.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults, map[string]any{"gfm": false})

result, _ := j2.Parse("~~keep~~")
fmt.Println(result)
// map[children:[map[children:[map[type:text value:~~keep~~]] type:paragraph]] type:document]
```

The instance is reusable — call `Parse` as often as you like. If you load other
plugins, install `Markdown` last, because it claims the `markdown` start rule.

## Walk the AST

The AST is nested `map[string]any` and `[]any`, so a recursive walk needs a type
assertion at every step. There is no way to make this pretty in Go; write the
walk once and reuse it.

```go
func walk(node map[string]any, visit func(map[string]any)) {
	visit(node)
	children, _ := node["children"].([]any)
	for _, child := range children {
		if m, ok := child.(map[string]any); ok {
			walk(m, visit)
		}
	}
}
```

Container nodes have `children`; leaf nodes such as `text`, `inlineCode`,
`code`, `html` and `image` do not, and the comma-ok assertion handles their
absence. To collect every link URL:

```go
doc := tabnasmarkdown.ParseDocument(
	"See [docs](https://example.com/docs) and [spec](https://spec.commonmark.org).\n\n- [home](/)\n",
	tabnasmarkdown.DefaultOptions)

var urls []string
walk(doc, func(n map[string]any) {
	if "link" == n["type"] {
		urls = append(urls, n["url"].(string))
	}
})

fmt.Println(urls)
// [https://example.com/docs https://spec.commonmark.org /]
```

Images carry their destination on `url` too, and their text on `alt` rather than
in children. Switch the type test to `"image"` to collect those instead.

Two things to know before you assert a numeric field:

- `depth` and `start` are `int` as returned. After a round trip through
  `encoding/json` they are `float64`, because that is what `json.Unmarshal`
  produces for a bare `any`. Assert against whichever side of the round trip you
  are on.
- `title` and `start` can be `nil`. `n["title"].(string)` panics on a link
  without a title; use the comma-ok form.

## Walk the native tree instead

For most traversal work in Go, this is the better tool. `ParseTree` returns
`*MdNode`, the native CommonMark tree the renderer reads: a typed struct, with
typed fields, and no assertions anywhere.

`MdNode` is a linked tree (`FirstChild`, `Next`, `Parent`, …). `node.Walker()`
gives you a depth-first cursor returning `*WalkEvent` with `Entering` and
`Node`, and `nil` when the walk is done.

```go
tree := tabnasmarkdown.ParseTree(
	"See [docs](https://example.com/docs) and [spec](https://spec.commonmark.org).\n\n- [home](/)\n",
	tabnasmarkdown.DefaultOptions)

var urls []string
w := tree.Walker()
for ev := w.Next(); ev != nil; ev = w.Next() {
	if ev.Entering && tabnasmarkdown.NodeLink == ev.Node.Type {
		urls = append(urls, ev.Node.Destination)
	}
}

fmt.Println(urls)
// [https://example.com/docs https://spec.commonmark.org /]
```

Native node types are the CommonMark ones, exported as `NodeType` constants —
`NodeHeading`, `NodeParagraph`, `NodeBlockQuote`, `NodeList`, `NodeItem`,
`NodeCodeBlock`, `NodeHTMLBlock`, `NodeEmph`, `NodeStrong`, `NodeLink`, … — not
the AST's mdast-adjacent names. A table is `NodeTable`, `NodeTableRow` and
`NodeTableCell` there. A heading's level is `Level`, a link's URL is
`Destination`, text is `Literal`.

Which to walk:

- **The native tree** for traversal, mutation, source positions, and anything
  you intend to render afterwards. It is typed and it is lossless.
- **The AST** when the map shape *is* the point: marshalling to JSON, feeding
  something that expects mdast-shaped data, or matching the TypeScript
  package's output field for field.

## Transform a document, then render it

`ParseDocument` projects the parse to maps and drops what the maps have no room
for. When you need to change a document and still render it, work on the native
tree: `ParseTree` gives you the tree, and `RenderHTML` turns it back into HTML.

```go
tree := tabnasmarkdown.ParseTree("# Title\n\nSee [docs](/docs).\n", tabnasmarkdown.DefaultOptions)

w := tree.Walker()
for ev := w.Next(); ev != nil; ev = w.Next() {
	n := ev.Node
	if ev.Entering && tabnasmarkdown.NodeLink == n.Type && strings.HasPrefix(n.Destination, "/") {
		n.Destination = "https://example.com" + n.Destination
	}
}

fmt.Printf("%q\n", tabnasmarkdown.RenderHTML(tree, tabnasmarkdown.DefaultOptions))
// "<h1>Title</h1>\n<p>See <a href=\"https://example.com/docs\">docs</a>.</p>\n"
```

The same shape works for block-level edits — demoting every heading one level,
for instance:

```go
tree := tabnasmarkdown.ParseTree("# Title\n\n## Section\n\ntext\n", tabnasmarkdown.DefaultOptions)

w := tree.Walker()
for ev := w.Next(); ev != nil; ev = w.Next() {
	if ev.Entering && tabnasmarkdown.NodeHeading == ev.Node.Type && ev.Node.Level < 6 {
		ev.Node.Level++
	}
}

fmt.Printf("%q\n", tabnasmarkdown.RenderHTML(tree, tabnasmarkdown.DefaultOptions))
// "<h2>Title</h2>\n<h3>Section</h3>\n<p>text</p>\n"
```

`MdNode` also has `AppendChild`, `PrependChild`, `InsertBefore`, `InsertAfter`
and `Unlink` if you need to restructure rather than retag. `RenderHTML` takes
the same `Options` value as everything else. `Breaks` affects it, and `GFM`
selects one thing only — the disallowed-raw-HTML filter, which the extension
defines at render time. The tree records the flavour it was parsed with, so
`RenderHTML(tree, tabnasmarkdown.Options{GFM: tree.GFM})` renders it as parsed.

## Render untrusted Markdown safely

Sanitize the HTML downstream. This package does not, and cannot: CommonMark
specifies that raw HTML passes through verbatim, and passing it through is part
of scoring 652/652.

```go
opts := tabnasmarkdown.DefaultOptions

fmt.Printf("%q\n", tabnasmarkdown.ToHTML(`<img onerror="alert(1)">`+"\n", opts))
// "<img onerror=\"alert(1)\">\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("[click](javascript:alert(1))\n", opts))
// "<p><a href=\"javascript:alert(1)\">click</a></p>\n"
```

What reaches the output untouched:

- HTML blocks (§4.6) — whole `<div>`, `<form>`, `<table>` blocks and anything
  else that starts a block-level tag.
- Inline raw HTML (§6.6) — `<b onclick="...">` and friends, attributes intact.
- Link and image destinations, including `javascript:` URLs. They are
  percent-encoded and entity-decoded, not filtered by scheme.

The one thing `GFM` changes here is the disallowed-raw-HTML filter, which
rewrites the leading `<` of nine tag names — `title`, `textarea`, `style`,
`xmp`, `iframe`, `noembed`, `noframes`, `script`, `plaintext` — and leaves every
other tag, and every attribute, alone:

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("<script>alert(1)</script>", tabnasmarkdown.DefaultOptions))
// "&lt;script>alert(1)&lt;/script>\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("<script>alert(1)</script>", tabnasmarkdown.Options{GFM: false}))
// "<script>alert(1)</script>\n"
```

That is nine tag names out of the whole of HTML. It is not a sanitizer.

Run the output through an HTML sanitizer (bluemonday, or whatever your stack
already uses) before it reaches a browser. If you would rather detect raw HTML
than strip it, the AST surfaces it as `html` nodes — one per tag inline, one per
block:

```go
doc := tabnasmarkdown.ParseDocument("Hi <b>there</b>", tabnasmarkdown.DefaultOptions)
para := doc["children"].([]any)[0].(map[string]any)

fmt.Println(para["children"])
// [map[type:text value:Hi ] map[type:html value:<b>] map[type:text value:there] map[type:html value:</b>]]
```

On the native tree the same nodes are `NodeHTMLInline` and `NodeHTMLBlock`. Either
way that tells you the input contains HTML; it does not make the HTML safe, and
it says nothing about link destinations. The sanitizer is still the fix.

## Turn the GFM extensions off, or hard breaks on

There are two options, and they behave the same on `ParseDocument`,
`ParseInline`, `ToHTML`, `ParseTree` and `RenderHTML`:

```go
type Options struct {
	GFM    bool // default true
	Breaks bool // default false
}
```

`Options` is a plain struct, so its zero value is `{GFM: false, Breaks: false}`
— **not** the defaults. `Options{Breaks: true}` therefore turns strikethrough
off as a side effect. To change one setting, copy `DefaultOptions`:

```go
opts := tabnasmarkdown.DefaultOptions
opts.Breaks = true
```

Write `Options{GFM: false}` only when you mean pure CommonMark with no
extensions, which is what the conformance suite runs.

`GFM` gates five extensions as one switch — tables, strikethrough, task list
items, autolink literals and the disallowed-raw-HTML filter. Footnotes are not
implemented, with `GFM: true` or without it. With `GFM: false` the output is
plain CommonMark, byte for byte.

```go
doc := tabnasmarkdown.ParseDocument("~~gone~~", tabnasmarkdown.DefaultOptions)
fmt.Println(doc["children"].([]any)[0].(map[string]any)["children"])
// [map[children:[map[type:text value:gone]] type:delete]]

off := tabnasmarkdown.ParseDocument("~~gone~~", tabnasmarkdown.Options{GFM: false})
fmt.Println(off["children"].([]any)[0].(map[string]any)["children"])
// [map[type:text value:~~gone~~]]

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("~~gone~~", tabnasmarkdown.Options{GFM: false}))
// "<p>~~gone~~</p>\n"
```

## Read a table

Use `ParseTree`, not `ParseDocument`. A table is three levels of nesting, and on
the map AST that is three levels of type assertion per cell; on the native tree
the alignment is a typed `[]TableAlign` field and the header row is a `bool`.
The map form is at the end of this recipe for when you need the maps anyway.

The three node types are `NodeTable`, `NodeTableRow` and `NodeTableCell`. The
table carries the column alignments:

```go
tree := tabnasmarkdown.ParseTree(
	"| Item | Qty |\n| :--- | ---: |\n| pen | 3 |\n| ink | 12 |\n",
	tabnasmarkdown.DefaultOptions)

table := tree.FirstChild

fmt.Println(table.Type, table.TableAlign)
// table [left right]
```

`TableAlign` has one entry per column, read off the colons in the delimiter row:
`AlignLeft` for `:--`, `AlignRight` for `--:`, `AlignCenter` for `:-:`, and
`AlignNone` — the empty string — for a cell with no colon.

Rows and cells are ordinary linked children, so walk them with `FirstChild` and
`Next`. A cell's children are inline nodes, the same ones a paragraph holds, so
flattening one to text is a `Walker` over the cell:

```go
func cellText(cell *tabnasmarkdown.MdNode) string {
	var b strings.Builder
	w := cell.Walker()
	for ev := w.Next(); ev != nil; ev = w.Next() {
		t := ev.Node.Type
		if ev.Entering && (tabnasmarkdown.NodeText == t || tabnasmarkdown.NodeCode == t) {
			b.WriteString(ev.Node.Literal)
		}
	}
	return b.String()
}
```

```go
for row := table.FirstChild; row != nil; row = row.Next {
	cells := make([]string, 0, len(table.TableAlign))
	for cell := row.FirstChild; cell != nil; cell = cell.Next {
		cells = append(cells, cellText(cell))
	}
	fmt.Println(row.IsHeaderRow, cells)
}
// true [Item Qty]
// false [pen 3]
// false [ink 12]
```

Two things make that loop safe to index into. Every row has exactly as many
cells as `TableAlign` has entries — the block phase pads short rows with empty
cells and truncates long ones — so `table.TableAlign[i]` is the alignment of
cell `i` of every row. And the header row is always the table's first child;
`IsHeaderRow` is there so the renderer can pick `<th>` without walking back up,
and you can use it the same way or just take the first child.

To put a pipe inside a cell, escape it. `\|` never splits a cell, and it becomes
a literal `|` before the inline phase runs, so a code span in that cell holds a
real pipe:

```go
src := "| a |\n| - |\n| b `\\|` az |\n"

body := tabnasmarkdown.ParseTree(src, tabnasmarkdown.DefaultOptions).FirstChild.FirstChild.Next
for c := body.FirstChild.FirstChild; c != nil; c = c.Next {
	fmt.Printf("%s=%q ", c.Type, c.Literal)
}
// text="b " code="|" text=" az"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML(src, tabnasmarkdown.DefaultOptions))
// "<table>\n<thead>\n<tr>\n<th>a</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>b <code>|</code> az</td>\n</tr>\n</tbody>\n</table>\n"
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
- With `GFM: false` there is no table at all: the lines stay one paragraph.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("intro line\n| a | b |\n| - | - |\n| 1 | 2 |\n", tabnasmarkdown.DefaultOptions))
// "<p>intro line</p>\n<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n</table>\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("| a | b |\n| - | - |\n| 1 | 2 |\n", tabnasmarkdown.Options{GFM: false}))
// "<p>| a | b |\n| - | - |\n| 1 | 2 |</p>\n"
```

Rendering is `ToHTML` as usual. A table with no body rows emits no `<tbody>`:

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("| a | b |\n| - | - |\n", tabnasmarkdown.DefaultOptions))
// "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n"
```

**The map form**, when you want the maps rather than the tree. The node types are
`table`, `tableRow` and `tableCell`, and the alignment is `align`:

```go
doc := tabnasmarkdown.ParseDocument(
	"| Item | Qty |\n| :--- | ---: |\n| pen | 3 |\n| ink | 12 |\n",
	tabnasmarkdown.DefaultOptions)
table := doc["children"].([]any)[0].(map[string]any)

fmt.Println(table["type"], table["align"], len(table["children"].([]any)))
// table [left right] 3

header := table["children"].([]any)[0].(map[string]any)
cell := header["children"].([]any)[0].(map[string]any)
fmt.Println(cell["children"].([]any)[0].(map[string]any)["value"])
// Item
```

`align` is an `[]any`, not a `[]string`, because a column with no colon is an
untyped `nil` there rather than the native tree's `AlignNone`. That is what makes
it marshal the way the TypeScript package's does:

```go
doc := tabnasmarkdown.ParseDocument("| a | b |\n| --- | :-: |\n", tabnasmarkdown.DefaultOptions)
table := doc["children"].([]any)[0].(map[string]any)

b, _ := json.Marshal(table["align"])
fmt.Println(string(b))
// [null,"center"]
```

There is no header flag on the map AST — that is mdast's shape, and mdast's
convention is that the first row is the header row.

## Work with task lists and bare URLs

A list item whose first paragraph starts with `[ ]` or `[x]` and at least one
space is a task list item: the marker is consumed and the item's `checked` field
records its state (`nil` on an ordinary item).

```go
doc := tabnasmarkdown.ParseDocument("- [x] done\n- [ ] todo\n- plain\n", tabnasmarkdown.DefaultOptions)
list := doc["children"].([]any)[0].(map[string]any)
for _, item := range list["children"].([]any) {
	fmt.Print(item.(map[string]any)["checked"], " ")
}
// true false <nil>

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("- [x] done\n", tabnasmarkdown.DefaultOptions))
// "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> done</li>\n</ul>\n"
```

On the native tree the same state is `MdNode.Checked` plus `MdNode.HasChecked`,
which is Go's spelling of the AST's `boolean | null`.

Bare URLs and addresses become links without angle brackets, at the start of a
line, after whitespace, or after `*`, `_`, `~` or `(`. Trailing sentence
punctuation is left out of the link.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("Visit www.commonmark.org.", tabnasmarkdown.DefaultOptions))
// "<p>Visit <a href=\"http://www.commonmark.org\">www.commonmark.org</a>.</p>\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("mail me at a@b.co", tabnasmarkdown.DefaultOptions))
// "<p>mail me at <a href=\"mailto:a@b.co\">a@b.co</a></p>\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("www.commonmark.org", tabnasmarkdown.Options{GFM: false}))
// "<p>www.commonmark.org</p>\n"
```

`Breaks` promotes soft line breaks to hard ones:

```go
opts := tabnasmarkdown.DefaultOptions
opts.Breaks = true

doc := tabnasmarkdown.ParseDocument("a\nb", opts)
fmt.Println(doc["children"].([]any)[0].(map[string]any)["children"])
// [map[type:text value:a] map[type:break] map[type:text value:b]]

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("a\nb", opts))
// "<p>a<br />\nb</p>\n"
```

On the plugin the same two settings are map keys, `"gfm"` and `"breaks"`, and
`ResolveOptions(map[string]any)` converts a map into an `Options` value with the
defaults filled in.

## Handle Markdown written for GitHub, or for another dialect

The five GFM extensions are the whole of what `GFM` turns on. Two things that
authors write anyway will parse without complaint and render as something else,
so check for them rather than waiting for a bug report.

**Footnotes are not implemented.** They are a GitHub product feature, not part
of the GFM specification suite. Nothing errors, because `[^1]` is a valid
CommonMark link label: the reference falls through to literal text, and the
definition line becomes an ordinary paragraph.

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("Text[^1]\n\n[^1]: Note text here.\n", tabnasmarkdown.DefaultOptions))
// "<p>Text[^1]</p>\n<p>[^1]: Note text here.</p>\n"
```

Worse, a definition whose body happens to look like a destination *is* a valid
link reference definition, so the footnote quietly becomes a link:

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("Text[^1]\n\n[^1]: /note\n", tabnasmarkdown.DefaultOptions))
// "<p>Text<a href=\"/note\">^1</a></p>\n"
```

Scan for `[^` before you convert a corpus, and strip or rewrite footnotes in the
source. There is no option that changes this.

**Single-tilde subscript collides with strikethrough.** GFM's strikethrough
accepts a single `~` as well as `~~`, so the `H~2~O` subscript syntax that
Pandoc and several other dialects use comes out as `<del>`:

```go
fmt.Printf("%q\n", tabnasmarkdown.ToHTML("H~2~O\n", tabnasmarkdown.DefaultOptions))
// "<p>H<del>2</del>O</p>\n"

fmt.Printf("%q\n", tabnasmarkdown.ToHTML("H~2~O\n", tabnasmarkdown.Options{GFM: false}))
// "<p>H~2~O</p>\n"
```

If your input is really Pandoc-flavoured, escape the tildes (`H\~2\~O`) or parse
with `GFM: false`; there is no way to keep strikethrough and lose the collision.

Everything else outside CommonMark and the five GFM extensions is also absent —
math, front matter, definition lists, heading attributes, admonitions, wiki
links, emoji shortcodes, highlight, and sub/superscript. Each would need its own
opt-in flag rather than joining `GFM`, so none of them appear under `GFM: true`.

## Report errors with line numbers

Use `ParseTree`. The map AST has no source positions — they are one of the
things the projection drops — but every block node on the native tree carries
`SourcePos`, a `[2][2]int` of `[[startLine, startCol], [endLine, endCol]]`,
1-based, as in the spec. Inline nodes carry `[[0, 0], [0, 0]]`.

```go
tree := tabnasmarkdown.ParseTree(
	"# Title\n\n## Section\n\n#### Too deep\n\npara\n",
	tabnasmarkdown.DefaultOptions)

var problems []string
w := tree.Walker()
for ev := w.Next(); ev != nil; ev = w.Next() {
	n := ev.Node
	if ev.Entering && tabnasmarkdown.NodeHeading == n.Type && 3 < n.Level {
		problems = append(problems, fmt.Sprintf("line %d: heading level %d", n.SourcePos[0][0], n.Level))
	}
}

fmt.Println(problems)
// [line 5: heading level 4]
```

Columns count characters, not bytes, so a position lines up with what an editor
shows rather than with a byte offset into the source.

## Check conformance yourself

The whole CommonMark 0.31.2 suite, with a per-section table and a total:

```bash
cd go
go test -run TestCommonMarkSpec -v ./...
```

The last line is `TOTAL 652/652  100.00%`, across all 26 sections — that is the
substantiation for "conformant to CommonMark 0.31.2", and the suite it reads,
`test/commonmark/spec.json`, is vendored in the repository. The suite is pure
CommonMark, so it runs with `Options{GFM: false, Breaks: false}`;
`TestCommonMarkOptionMatrix` re-runs all 652 examples across all four `GFM` ×
`Breaks` combinations to check that no combination panics.

The GFM extension corpus, `test/gfm/spec.json`, is vendored the same way and has
its own test, with the same shape of table:

```bash
cd go
go test -run TestGFMSpec -v ./...
```

The last line is `TOTAL 24/24`, over the five extension sections — Tables 8,
Autolinks 11, Task list items 2, Strikethrough 2, Disallowed Raw HTML 1. Every
vendored section passes, and every one is asserted. The TypeScript twin is
`npm run conformance-gfm`.

Everything, including the shared `test/spec/*.tsv` fixtures that both runtimes
assert:

```bash
cd go
go test ./...
```

The TypeScript package runs the same 652 examples straight from source, with no
build step and no engine installed:

```bash
cd ts
npm run conformance
```

The 36 fixtures pin the AST through the plugin path in both runtimes; they are a
regression net, not a proof that the runtimes agree. The claim that they agree
rests on a wider comparison: all 652 spec inputs under all four `GFM` × `Breaks`
combinations — 2608 records — with both the AST and the HTML compared on each,
and 0 differences in either. Neither check covers `SourcePos`, which the AST
drops and the HTML does not encode.
