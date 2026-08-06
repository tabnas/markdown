# How-to guide: the markdown plugin (Go)

Task-oriented recipes. Each is self-contained. For the full option list see the
[reference](reference.md); for *why* the plugin behaves this way see the
[concepts](concepts.md).

Each recipe assumes these imports:

```go
import (
    tabnasjsonic "github.com/tabnas/jsonic/go"
    tabnasmarkdown "github.com/tabnas/markdown/go"
)
```

## Register the plugin

`UseDefaults` merges your options over `tabnasmarkdown.Defaults` and applies the
plugin. The returned instance is reusable across `Parse` calls:

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("# Hello")
fmt.Println(result)
// map[children:[map[children:[map[type:text value:Hello]] depth:1 type:heading]] type:document]
```

Use `j.Use(tabnasmarkdown.Markdown, opts)` if you want to pass raw options *without*
merging the defaults — but `UseDefaults` is the normal path.

## Parse headings

ATX (`#`…`######`) and Setext (`===`/`---` underline):

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("# H1")
// document -> heading{depth:1, children:[text "H1"]}

result, _ = j.Parse("Foo\n===")
// document -> heading{depth:1, children:[text "Foo"]}
```

## Parse lists

Unordered (`-` `*` `+`) and ordered (`1.` `1)`). Start number is preserved:

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("- a\n- b")
// document -> list{ordered:false, start:<nil>} -> [listItem paragraph "a", ...]

result, _ = j.Parse("5. a\n6. b")
// document -> list{ordered:true, start:5}
```

## Parse code blocks

Fenced (``` ``` / `~~~`) and indented (4 spaces):

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("```js\nconsole.log(\"hi\")\n```")
// document -> code{lang:"js", meta:<nil>, value:"console.log(\"hi\")"}

result, _ = j.Parse("    indented\n    code")
// document -> code{lang:<nil>, value:"indented\ncode"}
```

## Parse blockquotes

```go
result, _ := j.Parse("> hello\n> world")
// document -> blockquote -> [paragraph "hello world"]
```

## Parse thematic breaks

```go
result, _ := j.Parse("---")
// document -> thematicBreak
// Also: "***", "___", "- - -", "* * *"
```

## Inline: emphasis, strong, code

```go
result, _ := j.Parse("Hello *world*")
// paragraph -> [text "Hello ", emphasis [text "world"]]

result, _ = j.Parse("Hello **world**")
// paragraph -> [text "Hello ", strong [text "world"]]

result, _ = j.Parse("`code`")
// paragraph -> [inlineCode value:"code"]

result, _ = j.Parse("foo_bar_baz")
// paragraph -> [text "foo_bar_baz"]  (_ inside word is not emphasis per CommonMark)
```

## Links, images, autolinks

```go
result, _ := j.Parse("[link](https://example.com)")
// paragraph -> [link url:"https://example.com" title:<nil> children:[text "link"]]

result, _ = j.Parse("![alt](https://example.com/img.png)")
// paragraph -> [image url:"https://example.com/img.png" alt:"alt"]

result, _ = j.Parse("<https://example.com>")
// paragraph -> [link url:"https://example.com" ...]  (autolink, gated on gfm:true)
```

## GFM strikethrough

Enabled by default (`gfm:true`). Disable with `gfm:false`:

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults, map[string]any{"gfm": true})
result, _ := j.Parse("~~delete~~")
// paragraph -> [delete [text "delete"]]

j2 := tabnasjsonic.Make()
j2.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults, map[string]any{"gfm": false})
result, _ = j2.Parse("~~delete~~")
// paragraph -> [text "~~delete~~"]
```

## Soft and hard breaks

A single `\n` inside a paragraph is a soft break. By default it becomes a space:

```go
result, _ := j.Parse("a\nb")
// paragraph -> [text "a b"]  (soft break collapsed)
```

With `breaks:true`, soft breaks become `break` nodes. Hard breaks (two trailing spaces) are always `break`:

```go
j3 := tabnasjsonic.Make()
j3.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults, map[string]any{"breaks": true})
result, _ = j3.Parse("a\nb")
// paragraph -> [text "a", break, text "b"]

result, _ = j.Parse("a  \nb")
// paragraph -> [text "a", break, text "b"]  (even with breaks:false)
```

## HTML blocks

Lines starting with `<div>` etc. are emitted as raw `html` nodes:

```go
result, _ := j.Parse("<div>hi</div>")
// document -> html{value:"<div>hi</div>"}
```

## Apply engine-level options

The markdown plugin consumes only `gfm`/`breaks`. Other jsonic engine options (e.g. custom comment) are set via `SetOptions` *before* `UseDefaults`:

```go
j := tabnasjsonic.Make()
// (markdown disables string/comment/number/value lexers internally, so most
// engine lex gates do not affect Markdown prose parsing)
```
