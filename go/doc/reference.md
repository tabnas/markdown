# Reference: the markdown plugin (Go)

Complete, dry reference for the public API, every option, and the grammar the
plugin accepts. For a guided introduction see the [tutorial](tutorial.md); for
recipes see the [how-to guide](guide.md).


## Module

```bash
go get github.com/tabnas/markdown/go@latest
```

```go
import (
    tabnasjsonic "github.com/tabnas/jsonic/go"
    tabnasmarkdown "github.com/tabnas/markdown/go"
)
```

| | |
|---|---|
| Module | `github.com/tabnas/markdown/go` |
| Engine dependency | `github.com/tabnas/jsonic/go` (re-exports the tabnas engine API) |
| Go | 1.24+ |
| License | MIT |


## Public API

| Symbol | Kind | Purpose |
|---|---|---|
| `tabnasmarkdown.Markdown` | `func(j *tabnasjsonic.Jsonic, options map[string]any) error` | The plugin. Register with `j.UseDefaults(...)` or `j.Use(...)`. |
| `tabnasmarkdown.Defaults` | `map[string]any` | Default option values, merged by `UseDefaults`. |
| `tabnasmarkdown.Version` | `const string` | The Go module version (kept in sync with the TS package). |

```go
func Markdown(j *tabnasjsonic.Jsonic, options map[string]any) error
var Defaults map[string]any
const Version = "0.1.1"
```

The custom string matcher (`buildMarkdownStringMatcher`) is **unexported** in
the Go port; it is wired in by the plugin and not part of the public API
(unlike the TS package, which exports it).


## Registering and parsing

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults /*, opts... */)

result, err := j.Parse("a,b\n1,2")
```

- `tabnasjsonic.Make()` builds an engine instance with the jsonic base grammar
  already loaded.
- `UseDefaults(plugin, Defaults, opts...)` merges each `opts` map over
  `Defaults` and applies the plugin. Prefer this over `Use` so option defaults
  are filled in. `opts` is variadic; later maps win.
- `Use(plugin, opts)` applies the plugin with raw options (no defaults merge).
- `Parse(src string) (any, error)` returns the result and an error. On success
  the result is a `[]any` (see [Output types](#output-types)); on failure
  `err` is a `*tabnasjsonic.JsonicError`.
- The instance is reusable — call `Parse` repeatedly.


## Options

Options are a `map[string]any`. Nested groups (`field`, `record`, `string`) are
themselves `map[string]any`. `Defaults`:

```go
var Defaults = map[string]any{
    "trim":    nil,
    "comment": nil,
    "number":  nil,
    "value":   nil,
    "header":  true,
    "object":  true,
    "stream":  nil,
    "strict":  true,
    "field": map[string]any{
        "separation":   nil,
        "nonameprefix": "field~",
        "empty":        "",
        "names":        nil,
        "exact":        false,
    },
    "record": map[string]any{
        "separators": nil,
        "empty":      false,
    },
    "string": map[string]any{
        "quote":    `"`,
        "markdown": nil,
    },
}
```

### Top-level options

| Option | Go type | Default | Effect |
|---|---|---|---|
| `trim` | `bool` / `nil` | `nil` | Strip leading/trailing whitespace from each field. `nil` → `false` in strict mode, `true` in non-strict. Internal whitespace kept. |
| `comment` | `bool` / `nil` | `nil` | Treat `#` lines as comments and strip trailing `#...`. `nil` → `false` in strict, `true` in non-strict. |
| `number` | `bool` / `nil` | `nil` | Parse numeric literals as numbers (`float64`). `nil` → `false` in strict, `true` in non-strict. |
| `value` | `bool` / `nil` | `nil` | Parse `true` / `false` / `null` keywords (`null` → Go `nil`). `nil` → `false` in strict, `true` in non-strict. |
| `header` | `bool` | `true` | Use the first non-blank record as field names. With `false` it is data. |
| `object` | `bool` | `true` | Emit each record as an ordered map (`true`) or a `[]any` slice (`false`). |
| `stream` | `func(string, any)` / `nil` | `nil` | Streaming callback; when set, records go to the callback and the result is empty. See [Streaming](#streaming). |
| `strict` | `bool` | `true` | Strict mode disables jsonic value parsing; every field is text. See [concepts](concepts.md). |

### `field`

| Option | Go type | Default | Effect |
|---|---|---|---|
| `field.separation` | `string` / `nil` | `nil` (comma) | Field delimiter; replaces `#CA`. May be multi-character. |
| `field.nonameprefix` | `string` | `"field~"` | Prefix for keys of extra fields (object output). Key = prefix + index. |
| `field.empty` | `string` | `""` | Value for a missing/empty field. |
| `field.names` | `[]string` / `nil` | `nil` | Explicit field names; with `header: false` they key the maps. |
| `field.exact` | `bool` | `false` | If `true`, a row whose width differs from the header errors (see [Errors](#errors)). |

### `record`

| Option | Go type | Default | Effect |
|---|---|---|---|
| `record.separators` | `string` / `nil` | `nil` (newline) | Characters that end a record. |
| `record.empty` | `bool` | `false` | If `true`, blank lines become empty records. |

### `string`

| Option | Go type | Default | Effect |
|---|---|---|---|
| `string.quote` | `string` | `"\""` | Field quote character for the markdown string matcher. |
| `string.markdown` | `bool` / `nil` | `nil` | Controls the RFC-4180 (`""`-escaped) string matcher. Strict: on unless `false`. Non-strict: off unless `true`. |


## Output types

`Parse` returns `any`; assert to `[]any`. Each record's type depends on
`object`:

| `object` | Record type | Notes |
|---|---|---|
| `true` (default) | an internal ordered map | Preserves key order. Prints as `{[key order] map[...]}`. The type is unexported, so callers cannot type-assert to it — read object output by printing, or use `object: false` for programmatic access. |
| `false` | `[]any` | Plain slice of cells (`string`, `float64`, `bool`, or `nil`). Directly indexable and type-assertable. |

Empty input, all-blank input, and the streaming case yield an empty `[]any`.


## Streaming

`stream` has signature `func(what string, record any)`:

| `what` | `record` |
|---|---|
| `"start"` | `nil` |
| `"record"` | the parsed record (ordered map or `[]any`) |
| `"end"` | `nil` |
| `"error"` | not used by the Go port's plugin body (errors surface through `Parse`'s returned `error`) |

```go
var records []any
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults, map[string]any{
    "stream": func(what string, record any) {
        if what == "record" { records = append(records, record) }
    },
})
j.Parse("a,b\n1,2\n3,4")
// records: [{[a b] map[a:1 b:2]} {[a b] map[a:3 b:4]}]
```


## Errors

`Parse` returns a `*tabnasjsonic.JsonicError` on failure. It has a `.Code` field and
its `.Error()` message carries detail.

| Situation | `.Code` | Message contains |
|---|---|---|
| `field.exact`, too many fields | `unexpected` | `markdown_extra_field` |
| `field.exact`, too few fields | `unexpected` | `markdown_missing_field` |
| Quoted field with no closing quote | `unterminated_string` | — |
| Trailing junk after a value | `unexpected` | — |

> In the Go port the `field.exact` errors surface with `.Code == "unexpected"`;
> the specific code appears only in the message text. The TypeScript version
> exposes the specific code directly as `error.code`. See
> [concepts: Differences from the TS version](concepts.md#differences-from-the-ts-version).


## Grammar / syntax accepted

Start rule `markdown`. Same record grammar as the TS version:

```
markdown  = ( newline / record )*           ; zero or more rows
newline   = LN+ record? / ZZ                 ; blank line(s) between records
record    = list                             ; one row of fields
list      = elem ( CA elem )*                ; comma-separated fields
elem      = val?                             ; a field (may be empty)
val       = text / VAL                       ; field value (strict)
text      = ( VAL / SP )+                     ; raw text with significant spaces
```

| Token | Meaning |
|---|---|
| `LN` | newline — ends a record / row |
| `SP` | whitespace — significant inside a field |
| `CA` | comma — field separator (replaceable via `field.separation`) |
| `VAL` | a field value: text, number, string, or keyword |
| `ZZ` | end of input |
| `OB` `CB` `OS` `CS` `CL` `KEY` | embedded-JSON tokens (`{ } [ ] :`, map key) — strict mode disables them |

Structural rules: a record is delimiter-separated fields ending at a record
separator (newline) or end of input; empty fields yield `field.empty`; blank
lines are skipped unless `record.empty`; quoted fields use `""` escaping.

The railroad diagram of the live grammar is in the TS package
([`ts/doc/grammar.svg`](../../ts/doc/grammar.svg),
[`ts/doc/grammar.txt`](../../ts/doc/grammar.txt)); the grammar source is the
top-level [`markdown-grammar.jsonic`](../../markdown-grammar.jsonic), embedded
into `go/markdown.go`.
