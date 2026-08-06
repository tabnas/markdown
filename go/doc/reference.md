# Reference: the markdown plugin (Go)

Complete, dry reference for the public API, every option, and the AST the plugin produces. For a guided introduction see the [tutorial](tutorial.md); for task recipes see the [how-to guide](guide.md).

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
var Defaults = map[string]any{"gfm": true, "breaks": false}
const Version = "0.4.1"
```

## Registering and parsing

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults /*, opts... */)

result, err := j.Parse("# Hello\n")
```

- `tabnasjsonic.Make()` builds an engine instance with the jsonic base grammar already loaded.
- `UseDefaults(plugin, Defaults, opts...)` merges each `opts` map over `Defaults` and applies the plugin. Prefer this over `Use` so option defaults are filled in. `opts` is variadic; later maps win.
- `Use(plugin, opts)` applies the plugin with raw options (no defaults merge).
- `Parse(src string) (any, error)` returns the result and an error. For Markdown the result is always a `map[string]any` document node (never `[]any`); errors only occur for engine-level failures. On success `err` is `nil`.
- The instance is reusable — call `Parse` repeatedly.

## Options

`Defaults`:

```go
var Defaults = map[string]any{
    "gfm":    true,
    "breaks": false,
}
```

| Option | Go type | Default | Effect |
|---|---|---|---|
| `gfm` | `bool` | `true` | When `true`, enables `~~strikethrough~~` → `delete` and `<https://autolink>` handling. Tables/task lists are documented future extensions and are not parsed even when `gfm:true` (see `dx-report.md` §2.3). |
| `breaks` | `bool` | `false` | When `true`, a soft line break (`\n` inside a paragraph without trailing spaces) becomes a `break` node. When `false`, soft breaks become a single space. Hard breaks (two trailing spaces before `\n`) are always `break` nodes regardless of this option. |

## AST

All nodes are plain `map[string]any` (JSON-serialisable). `document` is the root.

### Document

```go
map[string]any{"type": "document", "children": []any{ /* Block */ }}
```

### Blocks

| Type | Shape |
|---|---|
| `heading` | `map[type:heading depth:1..6 children:Inline[]]` |
| `paragraph` | `map[type:paragraph children:Inline[]]` |
| `blockquote` | `map[type:blockquote children:Block[]]` |
| `list` | `map[type:list ordered:bool start:number\|nil spread:bool children:ListItem[]]` |
| `listItem` | `map[type:listItem spread:bool children:Block[]]` |
| `code` | `map[type:code lang:string\|nil meta:string\|nil value:string]` |
| `html` | `map[type:html value:string]` |
| `thematicBreak` | `map[type:thematicBreak]` |

### Inline

| Type | Shape |
|---|---|
| `text` | `map[type:text value:string]` |
| `emphasis` | `map[type:emphasis children:Inline[]]` |
| `strong` | `map[type:strong children:Inline[]]` |
| `inlineCode` | `map[type:inlineCode value:string]` |
| `link` | `map[type:link url:string title:string\|nil children:Inline[]]` |
| `image` | `map[type:image url:string title:string\|nil alt:string]` |
| `break` | `map[type:break]` |
| `delete` | `map[type:delete children:Inline[]]` (only when `gfm:true`) |

### Example

`"# Title\n\nHello *world*"` →

```json
{
  "type": "document",
  "children": [
    {"type": "heading", "depth": 1, "children": [{"type": "text", "value": "Title"}]},
    {"type": "paragraph", "children": [{"type": "text", "value": "Hello "}, {"type": "emphasis", "children": [{"type": "text", "value": "world"}]}]}
  ]
}
```

## Errors

Markdown is permissive — malformed syntax is treated as plain text, so `Parse` does not return Markdown-specific errors. Engine-level errors (e.g. `field.exact` in the legacy CSV path) would surface as `*tabnasjsonic.JsonicError` with `.Code`, but the prose parser does not use them.

## Grammar / syntax accepted

Start rule `markdown`. Prose syntax (CommonMark subset + `gfm` strikethrough):

```
document     = Block*
Block        = Heading | Paragraph | Blockquote | List | Code | Html | ThematicBreak
Heading      = ATX ("#"… "######") | Setext ("===" / "---" underline)
ThematicBreak= "---" | "***" | "___" (with optional spaces, e.g. "- - -")
Code         = Fenced ("```"/"~~~" with info string) | Indented (4 spaces)
Blockquote   = ">"-prefixed lines, inner content recursively parsed as Blocks
List         = consecutive "-"/"*"/"+" or "1."/"1)" items, each item's lines recursively parsed
Paragraph    = lines until blank or block boundary; trailing "===" / "---" reinterprets as Setext heading
Html         = "<...>" line until blank or block boundary
Inline (inside heading/paragraph/listItem) = escapes, code spans, links/images, autolinks, emphasis/strong, breaks, delete
```

The railroad diagram of the live grammar is in the TS package
([`ts/doc/grammar.svg`](../../ts/doc/grammar.svg),
[`ts/doc/grammar.txt`](../../ts/doc/grammar.txt)); the grammar source is the
top-level [`markdown-grammar.jsonic`](../../markdown-grammar.jsonic), embedded
into `go/markdown.go`. See `dx-report.md` §2.1 for why the prose grammar is a
single `markdown` rule whose block scanner lives in Go (mirroring `ts/src/markdown.ts`).
