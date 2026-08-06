# markdown plugin (Go)

A [Tabnas](https://github.com/tabnas/parser) grammar plugin that parses
**CommonMark / GFM Markdown** into a JSON AST — headings, paragraphs,
blockquotes, lists, code blocks, thematic breaks, HTML, and inline (emphasis,
strong, code, links, images, strikethrough). Despite the name's history as a
CSV-family reader, this package now parses **prose Markdown** (the Go port of
the canonical TypeScript package [`@tabnas/markdown`](../ts/README.md)).

This is the Go port of the canonical TypeScript package; the two share one
grammar source (`markdown-grammar.jsonic`) and a common fixture suite in
`test/spec/`.

## Install

```bash
go get github.com/tabnas/markdown/go@latest
```

Requires Go 1.24+. The plugin depends on `github.com/tabnas/jsonic/go`, which
re-exports the tabnas engine API.

## Example

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

    result, _ := j.Parse("# Hello\n\nHello *world*")
    fmt.Println(result)
    // map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading] map[children:[map[type:text value:Hello ] map[children:[map[type:text value:world]] type:emphasis]] type:paragraph]] type:document]
}
```

The result is always a single `document` node (`map[string]any{"type":"document",
"children":[]any{...}}`). Each block and inline node has a `type` field.

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md) — a guided first run.
- [How-to guide](doc/guide.md) — task recipes (headings, lists, code, links, GFM).
- [Reference](doc/reference.md) — the full API, every option, output types, and the grammar.
- [Concepts](doc/concepts.md) — how it works on the engine, plus *Differences from the TS version*.

For the TypeScript version, see [../ts/README.md](../ts/README.md).

## Grammar diagram

The railroad diagram of the live grammar lives in the TS package
([`ts/doc/grammar.svg`](../ts/doc/grammar.svg),
[`ts/doc/grammar.txt`](../ts/doc/grammar.txt)). The grammar source is the
top-level [`markdown-grammar.jsonic`](../markdown-grammar.jsonic), embedded into
[`markdown.go`](markdown.go).

## License

Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License.
