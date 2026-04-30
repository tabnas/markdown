# Markdown plugin for Jsonic (Go)

A Jsonic syntax plugin that parses Markdown text into Go slices of maps
or slices, with support for headers, quoted fields, custom
delimiters, streaming, and strict/non-strict modes.

```bash
go get github.com/jsonicjs/markdown/go@latest
```

This guide follows the [Diataxis](https://diataxis.fr) framework:
[tutorials](#tutorials) for first-time users, [how-to guides](#how-to-guides)
for specific tasks, [explanation](#explanation) for background, and a
[reference](#reference) for the API.

For the TypeScript version, see [markdown-ts.md](markdown-ts.md).


## Tutorials

### Parse a basic Markdown file

Register the `Markdown` plugin on a `jsonic` instance with `UseDefaults`,
then parse:

```go
package main

import (
    "fmt"

    jsonic "github.com/jsonicjs/jsonic/go"
    markdown "github.com/jsonicjs/markdown/go"
)

func main() {
    j := jsonic.Make()
    j.UseDefaults(markdown.Markdown, markdown.Defaults)

    result, _ := j.Parse("name,age\nAlice,30\nBob,25")
    fmt.Println(result)
    // [{name:Alice age:30} {name:Bob age:25}]
}
```

### Parse without headers

Return rows as slices instead of maps, with no header row:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "header": false,
    "object": false,
})

result, _ := j.Parse("a,b,c\n1,2,3")
// [[a b c] [1 2 3]]
```

### Parse with quoted fields

Double-quoted fields handle commas, newlines, and escaped quotes:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults)

result, _ := j.Parse(`name,bio
Alice,"Likes ""cats"" and dogs"
Bob,"Line1
Line2"`)
// [{name:Alice bio:Likes "cats" and dogs} {name:Bob bio:Line1\nLine2}]
```


## How-to guides

### Use a custom field delimiter

Set `field.separation` to use a delimiter other than comma:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "field": map[string]any{"separation": "\t"},
})

result, _ := j.Parse("name\tage\nAlice\t30")
// [{name:Alice age:30}]
```

### Enable number and value parsing

By default in strict mode, all values are strings. Enable `number`
and `value` to parse numeric and boolean values:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "number": true,
    "value":  true,
})

result, _ := j.Parse("a,b,c\n1,true,null")
// [{a:1 b:true c:<nil>}]
```

### Trim whitespace from fields

Enable `trim` to remove leading and trailing whitespace from field
values:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "trim": true,
})

result, _ := j.Parse("a , b \n 1 , 2 ")
// [{a:1 b:2}]
```

### Stream records as they are parsed

Use the `stream` callback to receive records one at a time:

```go
var records []any

j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "stream": func(what string, record any) {
        if what == "record" {
            records = append(records, record)
        }
    },
})

j.Parse("a,b\n1,2\n3,4")
// records contains [{a:1 b:2}, {a:3 b:4}]
```

### Provide explicit field names

Set `field.names` when the input has no header row but you want
map output with named fields:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "header": false,
    "field":  map[string]any{"names": []string{"x", "y", "z"}},
})

result, _ := j.Parse("1,2,3\n4,5,6")
// [{x:1 y:2 z:3} {x:4 y:5 z:6}]
```

### Enforce exact field counts

Set `field.exact` to error when a row has more or fewer fields
than the header:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "field": map[string]any{"exact": true},
})

_, err := j.Parse("a,b\n1,2,3")
// err code: markdown_extra_field
```

### Reuse a configured parser

`UseDefaults` applies the plugin to a `jsonic.Jsonic` instance, which
you can keep and call repeatedly:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "number": true,
})

r1, _ := j.Parse("a,b\n1,2")
r2, _ := j.Parse("x,y\n3,4")
```

### Enable comment lines

Enable `comment` to skip lines starting with `#`:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "comment": true,
})

result, _ := j.Parse("a,b\n# skip\n1,2")
// [{a:1 b:2}]
```


## Explanation

### Strict vs non-strict mode

In **strict mode** (default), the Markdown plugin disables Jsonic's
built-in JSON parsing. All field values are treated as raw strings
unless `number` or `value` options are enabled. This matches the
behaviour of standard tabular text parsers.

In **non-strict mode** (`"strict": false`), the plugin preserves
Jsonic's ability to parse JSON values. Fields can contain objects,
arrays, booleans, numbers, and quoted strings using Jsonic syntax.
Non-strict mode enables `trim`, `comment`, and `number` by default.

### How quoted fields work

The plugin includes a custom Markdown string matcher that handles the
RFC 4180 double-quote escaping convention:

- A field wrapped in double quotes can contain commas, newlines,
  and quotes.
- A literal quote inside a quoted field is represented as `""`.
- For example: `"a""b"` parses to `a"b`.

### How options are passed

Options are supplied as `map[string]any` and merged with `Defaults`
via `jsonic.UseDefaults`. Nested groups (`field`, `record`, `string`)
are themselves `map[string]any`. This mirrors the TypeScript option
shape.


## Reference

### `Markdown` (Plugin)

```go
func Markdown(j *jsonic.Jsonic, options map[string]any) error
```

The plugin function. Register on a `jsonic.Jsonic` instance via
`j.UseDefaults(Markdown, Defaults, opts...)` (preferred) or
`j.Use(Markdown, opts)` for raw options without defaults.

### `Defaults`

```go
var Defaults map[string]any
```

Default option values, merged with caller options by `UseDefaults`.

```go
var Defaults = map[string]any{
    "trim":    nil,    // false in strict, true in non-strict
    "comment": nil,    // false in strict, true in non-strict
    "number":  nil,    // false in strict, true in non-strict
    "value":   nil,    // parse true/false/null
    "header":  true,   // first row is header
    "object":  true,   // records as maps (true) or slices (false)
    "stream":  nil,    // streaming callback
    "strict":  true,   // strict mode disables Jsonic syntax
    "field": map[string]any{
        "separation":   nil,        // field separator (default: ",")
        "nonameprefix": "field~",   // prefix for unnamed extra fields
        "empty":        "",         // value for empty fields
        "names":        nil,        // explicit field names
        "exact":        false,      // error on field count mismatch
    },
    "record": map[string]any{
        "separators": nil,    // custom record separators
        "empty":      false,  // preserve empty lines as records
    },
    "string": map[string]any{
        "quote":    `"`,  // quote character
        "markdown": nil,  // force markdown string mode (nil=auto)
    },
}
```

### `Version`

```go
const Version
```

The current Go module version.

### Stream callback signature

```go
func(what string, record any)
```

Called with `"start"`, `"record"`, `"end"`, or `"error"`. For
`"record"`, `record` is the parsed row (an `orderedMap` for object
output, or `[]any` for array output).

### Error codes

| code                     | meaning                                  |
| ------------------------ | ---------------------------------------- |
| `markdown_missing_field` | Row has fewer fields than expected       |
| `markdown_extra_field`   | Row has more fields than expected        |
| `unterminated_string`    | Quoted field is missing its closing quote |
