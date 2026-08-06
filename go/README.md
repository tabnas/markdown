# markdown plugin (Go)

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine, scoring
**652/652 on the CommonMark 0.31.2 spec suite**, with one GFM extension (strikethrough).
This is the Go port of the canonical TypeScript package
[`@tabnas/markdown`](../ts/README.md); the two are verified to agree on every example.

## Two outputs

**The AST is the primary output.** `ParseDocument` returns it directly, as a
`map[string]any`; the renderer does not run.

```go
opts := tabnasmarkdown.DefaultOptions

doc := tabnasmarkdown.ParseDocument("# Hello\n\nHello *world*", opts)
fmt.Println(doc["type"]) // document
fmt.Println(doc["children"])
// [map[children:[map[type:text value:Hello]] depth:1 type:heading] map[children:[map[type:text value:Hello ] map[children:[map[type:text value:world]] type:emphasis]] type:paragraph]]
```

**HTML is available, on request, via `ToHTML`.** The CommonMark suite scores HTML output,
so the renderer is what makes the 652/652 claim measurable; that it is useful to callers
is a consequence.

```go
fmt.Print(tabnasmarkdown.ToHTML("# Hello\n\nHello *world*", opts))
// <h1>Hello</h1>
// <p>Hello <em>world</em></p>
```

**The HTML is not sanitized.** Raw HTML blocks and inline tags pass through verbatim, as
CommonMark specifies, and GFM's disallowed-raw-HTML filter is not implemented. Put a
sanitizer downstream of any untrusted Markdown.

```go
fmt.Print(tabnasmarkdown.ToHTML("<script>alert(1)</script>", opts))
// <script>alert(1)</script>
```

`ParseTree` returns the native CommonMark node tree instead — it keeps `SourcePos` on
block nodes, and `RenderHTML` renders it, so you can parse, walk or mutate, then render.
See the [reference](doc/reference.md).

## Install

```bash
go get github.com/tabnas/markdown/go@latest
```

Requires Go 1.24+. `go.mod` requires `github.com/tabnas/parser/go` — the bare engine — and
nothing else. There is no jsonic dependency and there are no indirect dependencies.

## Specific to this runtime

`Make` returns an engine with the plugin already installed, the counterpart of
`new Tabnas().use(Markdown)` in TypeScript. Its `Parse` returns the same AST as
`ParseDocument`.

```go
package main

import (
	"fmt"

	tabnasmarkdown "github.com/tabnas/markdown/go"
)

func main() {
	j := tabnasmarkdown.Make()

	result, err := j.Parse("# Hello")
	fmt.Println(result, err)
	// map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document] <nil>
}
```

To install the plugin on an engine you already have, use `j.Use(tabnasmarkdown.Markdown, nil)`.

Options are a struct, not a map: `tabnasmarkdown.Options{GFM: bool, Breaks: bool}`, with
`DefaultOptions` being `{GFM: true, Breaks: false}`. `ResolveOptions` converts the plugin
option map form. The parser is engine-free — nothing under `commonmark.go` imports the
engine — so the conformance suite runs on its own:

```bash
go test -run TestCommonMarkSpec -v ./...   # 652/652
```

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md) — first parse, start to finish.
- [How-to guide](doc/guide.md) — task recipes.
- [Reference](doc/reference.md) — API, options, AST node types.
- [Concepts](doc/concepts.md) — how it works, and why, plus *Differences from the TS version*.

Top-level [README](../README.md) · TypeScript version: [ts/README.md](../ts/README.md).

## License

Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License.
