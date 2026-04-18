# CSV plugin for Jsonic (Go)

A Jsonic syntax plugin that parses CSV text into Go slices of maps
or slices, with support for headers, quoted fields, custom
delimiters, streaming, and strict/non-strict modes.

```bash
go get github.com/jsonicjs/csv/go@latest
```


## Tutorials

### Parse a basic CSV file

Parse CSV text with a header row into a slice of ordered maps:

```go
package main

import (
    "fmt"
    csv "github.com/jsonicjs/csv/go"
)

func main() {
    result, _ := csv.Parse("name,age\nAlice,30\nBob,25")
    fmt.Println(result)
    // [{name:Alice age:30} {name:Bob age:25}]
}
```

### Parse CSV without headers

Return rows as slices instead of maps, with no header row:

```go
result, _ := csv.Parse("a,b,c\n1,2,3", csv.CsvOptions{
    Header: boolPtr(false),
    Object: boolPtr(false),
})
// [[a b c] [1 2 3]]
```

### Parse CSV with quoted fields

Double-quoted fields handle commas, newlines, and escaped quotes:

```go
result, _ := csv.Parse(`name,bio
Alice,"Likes ""cats"" and dogs"
Bob,"Line1
Line2"`)
// [{name:Alice bio:Likes "cats" and dogs} {name:Bob bio:Line1\nLine2}]
```


## How-to guides

### Use a custom field delimiter

Set `Field.Separation` to use a delimiter other than comma:

```go
result, _ := csv.Parse("name\tage\nAlice\t30", csv.CsvOptions{
    Field: &csv.FieldOptions{Separation: "\t"},
})
// [{name:Alice age:30}]
```

### Enable number and value parsing

By default in strict mode, all values are strings. Enable `Number`
and `Value` to parse numeric and boolean values:

```go
result, _ := csv.Parse("a,b,c\n1,true,null", csv.CsvOptions{
    Number: boolPtr(true),
    Value:  boolPtr(true),
})
// [{a:1 b:true c:<nil>}]
```

### Trim whitespace from fields

Enable `Trim` to remove leading and trailing whitespace from field
values:

```go
result, _ := csv.Parse("a , b \n 1 , 2 ", csv.CsvOptions{
    Trim: boolPtr(true),
})
// [{a:1 b:2}]
```

### Stream records as they are parsed

Use the `Stream` callback to receive records one at a time:

```go
var records []any

result, _ := csv.Parse("a,b\n1,2\n3,4", csv.CsvOptions{
    Stream: func(what string, record any) {
        if what == "record" {
            records = append(records, record)
        }
    },
})
// result is [] (empty, records were streamed)
// records contains [{a:1 b:2}, {a:3 b:4}]
```

### Provide explicit field names

Set `Field.Names` when the CSV has no header row but you want
map output with named fields:

```go
result, _ := csv.Parse("1,2,3\n4,5,6", csv.CsvOptions{
    Header: boolPtr(false),
    Field:  &csv.FieldOptions{Names: []string{"x", "y", "z"}},
})
// [{x:1 y:2 z:3} {x:4 y:5 z:6}]
```

### Enforce exact field counts

Set `Field.Exact` to error when a row has more or fewer fields
than the header:

```go
_, err := csv.Parse("a,b\n1,2,3", csv.CsvOptions{
    Field: &csv.FieldOptions{Exact: true},
})
// err: unexpected extra field value
```

### Create a reusable parser

Use `MakeJsonic` to create a configured Jsonic instance you can
call repeatedly:

```go
j := csv.MakeJsonic(csv.CsvOptions{
    Number: boolPtr(true),
})

r1, _ := j.Parse("a,b\n1,2")
r2, _ := j.Parse("x,y\n3,4")
```

### Enable comment lines

Enable `Comment` to skip lines starting with `#`:

```go
result, _ := csv.Parse("a,b\n# skip\n1,2", csv.CsvOptions{
    Comment: boolPtr(true),
})
// [{a:1 b:2}]
```


## Explanation

### Strict vs non-strict mode

In **strict mode** (default), the CSV plugin disables Jsonic's
built-in JSON parsing. All field values are treated as raw strings
unless `Number` or `Value` options are enabled. This matches the
behaviour of standard CSV parsers.

In **non-strict mode** (`Strict: boolPtr(false)`), the plugin
preserves Jsonic's ability to parse JSON values. Fields can contain
objects, arrays, booleans, numbers, and quoted strings using Jsonic
syntax. Non-strict mode enables `Trim`, `Comment`, and `Number` by
default.

### How quoted fields work

The plugin includes a custom CSV string matcher that handles the
RFC 4180 double-quote escaping convention:

- A field wrapped in double quotes can contain commas, newlines,
  and quotes.
- A literal quote inside a quoted field is represented as `""`.
- For example: `"a""b"` parses to `a"b`.


## Reference

### `Parse` (Function)

```go
func Parse(src string, opts ...CsvOptions) ([]any, error)
```

Parse CSV text with the given options. Returns a slice of records.

### `MakeJsonic` (Function)

```go
func MakeJsonic(opts ...CsvOptions) *jsonic.Jsonic
```

Create a reusable Jsonic instance configured for CSV parsing.

### `CsvOptions`

```go
type CsvOptions struct {
    Object  *bool          // Return maps (true) or slices (false). Default: true
    Header  *bool          // First row is header. Default: true
    Trim    *bool          // Trim whitespace. Default: nil (false strict, true non-strict)
    Comment *bool          // Enable # comments. Default: nil (false strict, true non-strict)
    Number  *bool          // Parse numbers. Default: nil (false strict, true non-strict)
    Value   *bool          // Parse true/false/null. Default: nil
    Strict  *bool          // Strict CSV mode. Default: true
    Field   *FieldOptions
    Record  *RecordOptions
    String  *StringOptions
    Stream  StreamFunc
}
```

### `FieldOptions`

```go
type FieldOptions struct {
    Separation   string   // Field separator. Default: ","
    NonamePrefix string   // Prefix for unnamed extra fields. Default: "field~"
    Empty        string   // Value for empty fields. Default: ""
    Names        []string // Explicit field names.
    Exact        bool     // Error on field count mismatch. Default: false
}
```

### `RecordOptions`

```go
type RecordOptions struct {
    Separators string // Custom record separator characters.
    Empty      bool   // Preserve empty lines as records. Default: false
}
```

### `StringOptions`

```go
type StringOptions struct {
    Quote string // Quote character. Default: `"`
    Csv   *bool  // Force CSV string mode (nil=auto).
}
```

### `StreamFunc`

```go
type StreamFunc func(what string, record any)
```

Callback for streaming CSV parsing. Called with `"start"`, `"record"`,
`"end"`, or `"error"`.
