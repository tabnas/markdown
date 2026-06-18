# Tutorial: parsing records with the markdown plugin (Go)

A guided first run. By the end you will have the plugin installed, parsed a
small record file, and understood the result. About five minutes; assumes Go
1.24+ and a module to work in.

> The package is named *markdown*, but it parses **delimited records** —
> comma-separated fields, one record per line, with a header row. Treat it as a
> configurable CSV/TSV reader built on the tabnas engine.


## 1. Install

The plugin layers on the Go port of jsonic, which re-exports the tabnas engine
API. Add it to your module:

```bash
go get github.com/tabnas/markdown/go@latest
```

Then import both the jsonic package (the engine + base grammar) and the
markdown package:

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    markdown "github.com/tabnas/markdown/go"
)
```


## 2. Parse your first record file

Make a jsonic instance, register the markdown plugin with `UseDefaults` (which
merges your options over `markdown.Defaults`), then `Parse`:

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

What happened:

- The **first line** (`name,age`) is the header; its fields become the keys.
- Each later line is one **record**, split on commas into **fields**.
- You get back `[]any`, one record per data row.

Each record is an **ordered map** (`orderedMap`) — it keeps insertion order. In
`fmt` output it prints as `{[key order] map[...]}`; the `[name age]` part is the
key order and `map[...]` is the data.

Every value is a **string** (`age:30`, not `30`). In the default *strict* mode
the plugin does not interpret field contents — you opt into typed values next.


## 3. Read the result

`Parse` returns `any`. Assert it to `[]any` to get the records. Each record's
own type depends on the `object` option.

The object records are an internal ordered-map type that callers cannot type
assert to, so the most convenient way to *read* typed fields is **array
output** (`object: false`): each record is a plain `[]any` you can index
directly. Add `header: false` so the first row is data too:

```go
package main

import (
    "fmt"

    jsonic "github.com/tabnas/jsonic/go"
    markdown "github.com/tabnas/markdown/go"
)

func main() {
    j := jsonic.Make()
    j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
        "header": false,
        "object": false,
    })

    result, _ := j.Parse("Alice,30\nBob,25")
    rows := result.([]any)

    fmt.Println(len(rows)) // 2

    first := rows[0].([]any)
    fmt.Println(first[0], first[1]) // Alice 30
}
```

With the default `object: true`, records keep their key order but you read them
through `fmt`/printing rather than a type assertion — see the
[reference](reference.md#output-types) for the full type table.


## 4. Get numbers instead of strings

Turn on `number` (and `value` for `true`/`false`/`null`) to type the fields:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "number": true,
    "value":  true,
})

result, _ := j.Parse("a,b,c\n1,true,null")
fmt.Println(result)
// [{[a b c] map[a:1 b:true c:<nil>]}]
```

`1` is now a number, `true` a boolean, and `null` becomes Go `nil` (printed as
`<nil>`).


## 5. Quoted fields

A field wrapped in double quotes may contain commas, line breaks, or quotes. A
literal quote is doubled (`""`), per RFC 4180:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults)

result, _ := j.Parse("a\n\"b\"\"c\"")
fmt.Println(result)
// [{[a] map[a:b"c]}]
```

The two quotes collapse to one, so the field value is `b"c`.


## You have arrived

You can now:

- install the plugin and register it on a jsonic instance,
- parse a header-and-records file into `[]any` of ordered maps,
- get numbers and booleans with `number` / `value`,
- handle quoted fields.

Next:

- **[How-to guide](guide.md)** — recipes: custom delimiters, no-header mode,
  streaming, error handling.
- **[Reference](reference.md)** — the full option list, output types, and
  accepted grammar.
- **[Concepts](concepts.md)** — how it works on the engine, plus *Differences
  from the TS version*.
