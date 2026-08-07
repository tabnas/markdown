# Tutorial: parsing Markdown with @tabnas/markdown (Go)

This is a guided first run. By the end you will have added the module to a
program, parsed a document into an AST, rendered the same document to HTML, and
changed one option. Takes about five minutes and assumes Go 1.24+.

**This parser is conformant to CommonMark 0.31.2.** All 652 examples of the
specification's own test suite pass, in all 26 sections. The suite is vendored
in this repository, so the claim is one you can check rather than one you have
to take on trust — `cd go && go test -run TestCommonMarkSpec -v ./...` runs it
and prints the score. The Markdown you learn here is the whole language, not a
subset of it.

On top of that the package implements the complete set of five GFM extensions —
tables, task list items, autolink literals, strikethrough and disallowed raw
HTML — and they are on by default. The vendored GFM extension suite is 24
examples, and `go test -run TestGFMSpec -v ./...` runs that one.

Two answers before you start, because they shape everything below:

- **The AST is the primary output.** `ParseDocument` returns it, as a
  `map[string]any`. Nothing else runs — the HTML renderer is never touched.
- **Yes, there is an HTML emitter**: `ToHTML`. The CommonMark test suite scores
  HTML output, so the renderer is the instrument that makes the conformance
  claim measurable. You get to use it too.

You will meet both in this lesson, in that order.

## 1. Install

Make a directory for this lesson, start a module in it, and add the package:

```bash
mkdir mdnotes && cd mdnotes
go mod init mdnotes
go get github.com/tabnas/markdown/go@latest
```

That adds this package and the bare tabnas engine it requires, and nothing else.

Now make a file called `main.go` in that directory. Every code block below is
the whole of that file, and you run it with `go run .`.

## 2. Parse a document

Put this in `main.go`:

```go
package main

import (
	"encoding/json"
	"fmt"

	tabnasmarkdown "github.com/tabnas/markdown/go"
)

func main() {
	src := `# Notes

A *short* note with a [link](https://example.com).
`

	doc := tabnasmarkdown.ParseDocument(src, tabnasmarkdown.DefaultOptions)

	out, _ := json.MarshalIndent(doc, "", "  ")
	fmt.Println(string(out))
}
```

Run it:

```bash
go run .
```

You will see the AST. It is built from plain `map[string]any`, `[]any`, `string`
and `int` — no structs to learn, no cycles, and `encoding/json` marshals it
directly:

```json
{
  "children": [
    {
      "children": [
        {
          "type": "text",
          "value": "Notes"
        }
      ],
      "depth": 1,
      "type": "heading"
    },
    {
      "children": [
        {
          "type": "text",
          "value": "A "
        },
        {
          "children": [
            {
              "type": "text",
              "value": "short"
            }
          ],
          "type": "emphasis"
        },
        {
          "type": "text",
          "value": " note with a "
        },
        {
          "children": [
            {
              "type": "text",
              "value": "link"
            }
          ],
          "title": null,
          "type": "link",
          "url": "https://example.com"
        },
        {
          "type": "text",
          "value": "."
        }
      ],
      "type": "paragraph"
    }
  ],
  "type": "document"
}
```

The keys come out alphabetically because that is how `encoding/json` marshals a
map; the order carries no meaning. What does carry meaning is `type`, which
tells you what each node is, and `children`, which holds the nodes underneath
it.

The top of the tree is always a `document` node, and its `children` are the
blocks of the file in source order: the heading, then the paragraph.

## 3. Read the AST

Blocks that hold prose — headings, paragraphs, list items — have their own
`children`, holding the inline nodes. Because every node is a `map[string]any`,
reading the tree means asserting the type at each step. Replace the body of
`main()` with:

```go
	doc := tabnasmarkdown.ParseDocument("# Notes\n\nA *short* note.\n", tabnasmarkdown.DefaultOptions)

	fmt.Println(doc["type"]) // document

	children := doc["children"].([]any)
	fmt.Println(len(children)) // 2

	heading := children[0].(map[string]any)
	fmt.Println(heading["type"], heading["depth"]) // heading 1

	para := children[1].(map[string]any)
	inlines := para["children"].([]any)
	fmt.Println(inlines[1]) // map[children:[map[type:text value:short]] type:emphasis]
```

You can drop the `encoding/json` import now; `go run .` will tell you if you
forget.

Notice the shape of that last line: `*short*` did not stay as text with
asterisks in it. It became an `emphasis` node with a `text` child. That is the
whole point of the AST — you read structure, not punctuation.

## 4. Render the same document to HTML

The same input, one different function:

```go
	src := "# Notes\n\nA *short* note.\n"

	fmt.Printf("%q\n", tabnasmarkdown.ToHTML(src, tabnasmarkdown.DefaultOptions))
	// "<h1>Notes</h1>\n<p>A <em>short</em> note.</p>\n"
```

`%q` rather than `%s` so you can see where the newlines fall. That string is
byte-for-byte what the CommonMark 0.31.2 test suite expects, trailing newline
included.

One caution, from your very first render: **the HTML is not sanitized.**
CommonMark passes raw HTML through by specification, so `<script>alert(1)</script>`
in the Markdown comes out as `<script>alert(1)</script>` in the HTML. That is
correct behaviour, not a bug. When you get to rendering Markdown that other
people wrote, run a sanitizer over the output; the [how-to guide](guide.md) has
a recipe.

## 5. Change one option: breaks

Options are a struct, `tabnasmarkdown.Options`. To change one setting, copy
`DefaultOptions` and assign the field — that way the other settings stay at
their defaults.

A single newline inside a paragraph is a *soft* break. By default it reads as a
space in the AST, and as a newline in the HTML:

```go
	src := "line one\nline two\n"

	doc := tabnasmarkdown.ParseDocument(src, tabnasmarkdown.DefaultOptions)
	para := doc["children"].([]any)[0].(map[string]any)
	fmt.Println(para["children"]) // [map[type:text value:line one line two]]

	fmt.Printf("%q\n", tabnasmarkdown.ToHTML(src, tabnasmarkdown.DefaultOptions))
	// "<p>line one\nline two</p>\n"
```

Set `Breaks` and every soft break becomes a hard one — a `break` node in the
AST, a `<br />` in the HTML:

```go
	src := "line one\nline two\n"

	opts := tabnasmarkdown.DefaultOptions
	opts.Breaks = true

	doc := tabnasmarkdown.ParseDocument(src, opts)
	para := doc["children"].([]any)[0].(map[string]any)
	fmt.Println(para["children"])
	// [map[type:text value:line one] map[type:break] map[type:text value:line two]]

	fmt.Printf("%q\n", tabnasmarkdown.ToHTML(src, opts))
	// "<p>line one<br />\nline two</p>\n"
```

Both functions take the same `Options` value, and both were parsed by the same
parser. That is the pattern for everything else in this package.

## 6. Where to go next

You have the two outputs and one option. That is enough to be useful.

- [How-to guide](guide.md) — recipes: the plugin, walking the AST, walking the
  native tree, reading a table's alignment and cells, rewriting a document and
  re-rendering it, rendering untrusted input safely, source positions, running
  the conformance suite.
- [Reference](reference.md) — every exported symbol, every option, every node
  type.
- [Concepts](concepts.md) — why the parser is built the way it is, plus
  *Differences from the TS version*.
