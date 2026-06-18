# markdown plugin (Go)

A [Tabnas](https://github.com/tabnas/parser) grammar plugin that parses
delimited record text — a header row, comma-separated fields, one record per
line, with RFC-4180 quoting — into Go slices of maps or slices. Despite the
name it is a configurable CSV/TSV-family reader, with support for headers,
quoted fields, custom delimiters, streaming, and strict / non-strict modes.

This is the Go port of the canonical TypeScript package
[`@tabnas/markdown`](../ts/README.md); the two share one grammar and a common
fixture suite.

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

    jsonic "github.com/tabnas/jsonic/go"
    markdown "github.com/tabnas/markdown/go"
)

func main() {
    j := jsonic.Make()
    j.UseDefaults(markdown.Markdown, markdown.Defaults)

    result, _ := j.Parse("name,age\nAlice,30\nBob,25")
    fmt.Println(result)
    // [{[name age] map[age:30 name:Alice]} {[name age] map[age:25 name:Bob]}]
}
```

Object records keep key order (they are an ordered map, printed as
`{[key order] map[...]}`). For directly indexable output, use
`object: false` to get one `[]any` per row.

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md) — a guided first run.
- [How-to guide](doc/guide.md) — task recipes (delimiters, no-header, streaming, errors).
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

Copyright (c) 2021-2025 Richard Rodger and other contributors, MIT License.
