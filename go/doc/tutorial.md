# Tutorial: parsing Markdown with @tabnas/markdown (Go)

This is a guided first run. By the end you will have installed the plugin,
parsed Markdown, and understood the AST. Takes about five minutes and assumes
Go 1.24+.

## 1. Install

The plugin layers on the jsonic engine (which re-exports the tabnas engine).
Add it to your module:

```bash
go get github.com/tabnas/markdown/go@latest
```

Then import both packages:

```go
import (
    tabnasjsonic "github.com/tabnas/jsonic/go"
    tabnasmarkdown "github.com/tabnas/markdown/go"
)
```

## 2. Parse your first document

Make a jsonic instance, register the markdown plugin, then `Parse`:

```go
package main

import (
    "fmt"

    tabnasjsonic "github.com/tabnas/jsonic/go"
    tabnasmarkdown "github.com/tabnas/markdown/go"
)

func main() {
    j := tabnasjsonic.Make()
    j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

    result, _ := j.Parse("# Hello\n")
    fmt.Println(result)
    // map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document]
}
```

What happened: `# Hello` became a `heading` node with `depth: 1` and a `text` child.
The result is always a single `document` node (a `map[string]any` with `type:"document"`).

## 3. Read the result

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("# Title\n\nHello *world*")
doc := result.(map[string]any)
fmt.Println(doc["type"]) // document
children := doc["children"].([]any)
fmt.Println(children[0].(map[string]any)["type"]) // heading
fmt.Println(children[1].(map[string]any)["type"]) // paragraph
```

Each node's `type` field discriminates the AST. `document` contains `children: Block[]`.
Blocks include `heading`, `paragraph`, `blockquote`, `list`, `code`, `html`, `thematicBreak`.

## 4. Inline markup

Paragraphs and headings contain inline children:

```go
result, _ := j.Parse("Hello **world**")
// document -> paragraph -> [text "Hello ", strong [text "world"]]

result, _ = j.Parse("[link](https://example.com)")
// document -> paragraph -> [link url:"https://example.com" title:<nil> children:[text "link"]]
```

## 5. Lists and code

```go
result, _ = j.Parse("- a\n- b\n- c")
// document -> list{ordered:false} -> [listItem -> paragraph "a", ...]

result, _ = j.Parse("```js\nconsole.log(\"hi\")\n```")
// document -> code{lang:"js", value:"console.log(\"hi\")"}
```

## You have arrived

You can now:

- install the plugin and parse a Markdown document to a `document` AST,
- read headings, paragraphs, lists, and code blocks from the result.

Next:

- **[How-to guide](guide.md)** — recipes for GFM, breaks, lists, etc.
- **[Reference](reference.md)** — the full API, every option, and the AST shape.
- **[Concepts](concepts.md)** — how it works on the engine, plus *Differences from the TS version*.
