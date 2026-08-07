# markdown plugin (Go)

A CommonMark parser for the [Tabnas](https://github.com/tabnas/parser) engine.

**This parser is conformant to CommonMark 0.31.2** — all 652 examples, across all 26
sections of the spec suite, in both runtimes. The suite is vendored in this repository, so
the claim is checkable: `go test -run TestCommonMarkSpec ./...` reports 652/652. It also
implements **all five GFM extensions** — tables, task list items, autolink literals,
strikethrough and disallowed raw HTML — 24/24 on the vendored GFM corpus, via
`go test -run TestGFMSpec ./...`.

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
CommonMark specifies. Put a sanitizer downstream of any untrusted Markdown.

```go
fmt.Print(tabnasmarkdown.ToHTML(`<img onerror="alert(1)">`, opts))
// <img onerror="alert(1)">
```

GFM's disallowed-raw-HTML filter (on with `GFM`, the default) rewrites the leading `<` of
nine tag names — `title`, `textarea`, `style`, `xmp`, `iframe`, `noembed`, `noframes`,
`script`, `plaintext` — and touches nothing else:

```go
fmt.Print(tabnasmarkdown.ToHTML("<script>alert(1)</script>", opts))
// &lt;script>alert(1)&lt;/script>
```

`ParseTree` returns the native CommonMark node tree instead — it keeps `SourcePos` on
block nodes, and `RenderHTML` renders it, so you can parse, walk or mutate, then render.
See the [reference](doc/reference.md).

GFM tables add three node types, in mdast's shape — `table`, `tableRow` and `tableCell`.
`align` has one entry per column, `nil` where the delimiter cell had no colon; there is no
header flag, so the first row is the header row by convention, and every row has exactly
as many cells as `align` has entries.

```go
doc := tabnasmarkdown.ParseDocument("| a | b |\n| :- | -: |\n| 1 | 2 |", opts)
table := doc["children"].([]any)[0].(map[string]any)

fmt.Println(table["type"])  // table
fmt.Println(table["align"]) // [left right]
fmt.Println(table["children"].([]any)[0])
// map[children:[map[children:[map[type:text value:a]] type:tableCell] map[children:[map[type:text value:b]] type:tableCell]] type:tableRow]
```

The native tree names the same three nodes `table`, `table_row` and `table_cell`.

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
option map form. `GFM` gates five extensions together — tables, strikethrough, task list
items, autolink literals (bare `www.` / `https://` / `a@b.co`) and the
disallowed-raw-HTML filter. Footnotes are not implemented. With `GFM: false` the output
is plain CommonMark, byte for byte.

Footnotes are a GitHub product feature rather than part of the GFM spec suite, and their
absence is quiet: `[^1]` is a valid CommonMark link label, so a footnote authored on
GitHub renders as a broken link instead of raising an error.

```go
fmt.Print(tabnasmarkdown.ToHTML("Text[^1]\n\n[^1]: note", opts))
// <p>Text<a href="note">^1</a></p>
```

Nothing outside CommonMark and GFM is implemented — no math, front matter, definition
lists, heading attributes, admonitions, wiki links, emoji shortcodes, highlight or
sub/superscript. Each would need its own opt-in flag; `GFM` is not going to grow to mean
"everything". Note one collision: GFM's single-tilde strikethrough takes the syntax other
dialects use for subscript, so `H~2~O` is a deletion under the default `GFM: true`.

```go
fmt.Print(tabnasmarkdown.ToHTML("H~2~O", opts))
// <p>H<del>2</del>O</p>
```

The parser is engine-free — nothing under `commonmark.go` imports the engine — so the
conformance suite runs on its own:

```bash
go test -run TestCommonMarkSpec -v ./...   # 652/652, all 26 sections
go test -run TestGFMSpec -v ./...          # 24/24, all five extensions
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
