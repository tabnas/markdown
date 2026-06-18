# How-to guide: the markdown plugin (Go)

Task-oriented recipes. Each is self-contained. The `// ...` comments show the
real `fmt.Println` output. For the full option list see the
[reference](reference.md); for *why* the plugin behaves this way see the
[concepts](concepts.md).

Object records print as `{[key order] map[...]}` because they are an
order-preserving map; array records print as `[...]`.

Each recipe assumes these imports:

```go
import (
    jsonic "github.com/tabnas/jsonic/go"
    markdown "github.com/tabnas/markdown/go"
)
```


## Register the plugin

`UseDefaults` merges your options over `markdown.Defaults` and applies the
plugin. The returned instance is reusable across `Parse` calls:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults)

r, _ := j.Parse("a,b\n1,2\n3,4")
fmt.Println(r)
// [{[a b] map[a:1 b:2]} {[a b] map[a:3 b:4]}]
```

Use `j.Use(markdown.Markdown, opts)` if you want to pass raw options *without*
merging the defaults — but `UseDefaults` is the normal path.


## Return slices instead of maps

Set `object: false` for one `[]any` per row, and `header: false` so the first
row is data:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "header": false,
    "object": false,
})

r, _ := j.Parse("a,b,c\n1,2,3")
fmt.Println(r)
// [[a b c] [1 2 3]]
```

Slice output is the easiest to read programmatically — each cell is a plain
value you can index and type-assert.


## Name fields when there is no header row

With `header: false` and `field.names`, records are keyed by your names:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "header": false,
    "field":  map[string]any{"names": []string{"x", "y", "z"}},
})

r, _ := j.Parse("1,2,3\n4,5,6")
fmt.Println(r)
// [{[x y z] map[x:1 y:2 z:3]} {[x y z] map[x:4 y:5 z:6]}]
```

Note `names` is a `[]string` inside the `field` map.


## Use a custom field delimiter

Set `field.separation` (single- or multi-character):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "field": map[string]any{"separation": "|"},
})

r, _ := j.Parse("a|b\n1|2")
fmt.Println(r)
// [{[a b] map[a:1 b:2]}]
```


## Use a custom record separator

Set `record.separators` to end rows on a character other than newline:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "record": map[string]any{"separators": "%"},
})

r, _ := j.Parse("a,b%1,2%3,4")
fmt.Println(r)
// [{[a b] map[a:1 b:2]} {[a b] map[a:3 b:4]}]
```


## Parse numbers and keywords

Enable `number` for numeric literals and `value` for `true`/`false`/`null`
(`null` becomes Go `nil`, printed `<nil>`):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "number": true,
    "value":  true,
})

r, _ := j.Parse("a,b,c\n1,true,null")
fmt.Println(r)
// [{[a b c] map[a:1 b:true c:<nil>]}]
```


## Trim whitespace from fields

Enable `trim` to strip leading/trailing whitespace (internal spacing kept):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "trim": true,
})

r, _ := j.Parse("a , b \n 1 , 2 ")
fmt.Println(r)
// [{[a b] map[a:1 b:2]}]
```


## Skip comment lines

Enable `comment` to ignore `#` lines (and strip trailing `#...` from a field):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "comment": true,
})

r, _ := j.Parse("a,b\n# skip\n1,2")
fmt.Println(r)
// [{[a b] map[a:1 b:2]}]
```


## Handle quoted fields

Double-quoted fields span commas, line breaks, and doubled quotes (`""` →
`"`):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults)

r, _ := j.Parse("a\n\"b\"\"c\"")
fmt.Println(r)
// [{[a] map[a:b"c]}]
```

For a different quote character, set `string.quote`.


## Handle ragged rows

Extra fields beyond the header get keys formed from `field.nonameprefix`
(default `field~`) plus the index; missing fields take `field.empty` (default
`""`):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults)

r, _ := j.Parse("a,b\n1,2,3")
fmt.Println(r)
// [{[a b field~2] map[a:1 b:2 field~2:3]}]
```


## Reject ragged rows

Set `field.exact` so a row whose field count differs from the header errors:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "field": map[string]any{"exact": true},
})

_, err := j.Parse("a,b\n1,2,3")
fmt.Println(err != nil) // true
```

`Parse` returns a non-nil `error`. The error is a `*jsonic.JsonicError`; its
`.Code` is `"unexpected"` and its message text names the specific failure
(`markdown_extra_field` for too many fields, `markdown_missing_field` for too
few):

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "field": map[string]any{"exact": true},
})

_, err := j.Parse("a,b\n1")
if je, ok := err.(*jsonic.JsonicError); ok {
    fmt.Println(je.Code)                                   // unexpected
    fmt.Println(strings.Contains(je.Error(), "markdown_missing_field")) // true
}
```

(This differs from the TypeScript version, where the thrown error's `.code` is
the specific `markdown_missing_field` / `markdown_extra_field`. See
[concepts: Differences from the TS version](concepts.md#differences-from-the-ts-version).)


## Keep blank lines as empty records

Set `record.empty` so blank lines become empty records rather than being
skipped:

```go
j := jsonic.Make()
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{
    "record": map[string]any{"empty": true},
})

r, _ := j.Parse("a\n1\n\n2")
fmt.Println(r)
// [{[a] map[a:1]} {[a] map[a:]} {[a] map[a:2]}]
```


## Stream records as they are parsed

Pass a `stream` callback of type `func(what string, record any)`. It receives
`"start"`, a `"record"` per row, then `"end"` (`"error"` on failure). While
streaming, the result is not collected:

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
fmt.Println(records)
// [{[a b] map[a:1 b:2]} {[a b] map[a:3 b:4]}]
```


## Apply engine-level options

Some behaviours (a custom comment string, custom value keywords) are jsonic
engine options, not markdown options. Set them with `SetOptions` *before*
`UseDefaults`, because the markdown plugin re-applies the base grammar when it
loads:

```go
j := jsonic.Make()
j.SetOptions(jsonic.Options{Comment: &jsonic.CommentOptions{
    Def: map[string]*jsonic.CommentDef{
        "bang": {Start: "!", Line: true},
    },
}})
j.UseDefaults(markdown.Markdown, markdown.Defaults, map[string]any{"comment": true})

r, _ := j.Parse("a\n! a comment\n1")
fmt.Println(r)
// [{[a] map[a:1]}]
```
