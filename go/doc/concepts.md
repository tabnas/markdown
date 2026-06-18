# Concepts: how the markdown plugin works (Go)

Background and design rationale, plus the differences between the Go port and
the canonical TypeScript implementation. For the API see the
[reference](reference.md).


## It is a grammar plugin, not a parser

The package contains no parser. It is a **plugin for the tabnas engine** that,
with the jsonic base grammar, configures the engine to read delimited records:

```
jsonic.Make()                                  // engine + jsonic base grammar (val/map/list/pair/elem rules, tokens)
  .UseDefaults(markdown.Markdown, Defaults)    // layer the record grammar on top
  .Parse(src)
```

The Go jsonic package re-exports the engine API (`jsonic.Jsonic`,
`jsonic.Options`, `jsonic.Rule`, `jsonic.LexOptions`, …), so the plugin imports
`jsonic`, not a separate engine package.

> The package is named *markdown* but installs a record/field (CSV-family)
> grammar — header rows, `,`-separated fields, one record per line, RFC-4180
> quoting. It was ported from the TS package, which was built from the
> `@tabnas/csv` template.


## The record model

A document is a sequence of **records** (rows); a record is a **list** of
**fields** separated by a delimiter (`,` by default), ending at a record
separator (newline) or end of input. The grammar rules:

- `markdown` — entry rule; loops rows, dispatching blank lines to `newline`
  and content rows to `record`.
- `newline` — absorbs blank lines (so they don't become records, unless
  `record.empty`).
- `record` — one row; pushes `list`, closes at newline / end of input. Its
  close action (`@record-bc`) turns the row into a map or slice, applies the
  header, runs `field.exact`, and appends to the result.
- `list` / `elem` / `val` — the field machinery.
- `text` — concatenates `VAL`/`SP` tokens into one field string so in-field
  whitespace survives.

`markdown`, `newline`, `record`, and `text` come from the embedded grammar
text; `list`, `elem`, and `val` are built in code (`j.Rule(...)`) because the
two modes need different versions of them.


## Significant whitespace and newlines

In plain jsonic, `#SP` (space), `#LN` (newline) and `#CM` (comment) are
*ignored* tokens. A record grammar can't allow that, so the plugin rewrites the
`IGNORE` token set:

- `#LN` is removed from `IGNORE` in every mode — row breaks are structural.
- `#SP` is removed in strict mode so in-field spaces reach the `text` rule; in
  non-strict mode `#SP` stays ignored (which is why non-strict defaults `trim`
  on).
- `#CM` stays ignored, so comment tokens (when comment lexing is enabled) drop
  out.


## Strict vs non-strict

**Strict mode (default).** jsonic value parsing is off
(`rule.exclude: 'jsonic,imp'`), the JSON structural tokens `#OB #CB #OS #CS
#CL` are disabled, and a field is raw text. Numbers and keywords need the
`number` / `value` opt-ins. Quoting uses the plugin's RFC-4180 string matcher
(doubled quotes).

**Non-strict mode (`strict: false`).** jsonic's `val` / `list` / `map`
alternatives are preserved so a field *can* contain jsonic content, and
`trim` / `comment` / `number` / `value` all default on. (See the port gap on
embedded containers in [Differences](#differences-from-the-ts-version).)


## How a row becomes a record

The `@record-bc` action does the per-row work:

1. The first non-blank record, when `header: true`, is captured as the field
   names (`ctx.Meta["fields"]`) and not emitted.
2. Each later record becomes the output value: an ordered map (when
   `object: true`) keyed by header/`names` with extras under
   `field.nonameprefix + index`, or a `[]any` (when `object: false`).
3. Missing fields are replaced with `field.empty`.
4. With `field.exact`, a width mismatch sets `ctx.ParseErr` to a `#BD` token
   carrying `markdown_extra_field` / `markdown_missing_field`, which the engine
   then reports.
5. The record is appended to the result — or, when `stream` is set, handed to
   the callback and not stored.

The Go object record is an `orderedMap{keys, m}` struct that keeps insertion
order so the parity fixtures (which compare against ordered JSON) line up with
the TS objects.


## The embedded grammar and `@`-refs

The four record rules are authored once in the top-level
`markdown-grammar.jsonic` and embedded verbatim into `go/markdown.go` (and
`ts/src/markdown.ts`) by `embed-grammar.js` during the TS build. Never
hand-edit the embedded block.

At init, the grammar text is parsed by a standalone jsonic instance
(`jsonic.Make().Parse(grammarText)`), converted to a `*jsonic.GrammarSpec` by
`parseGrammarText` / `buildGrammarAlts`, given the `refs` map, and applied with
`j.Grammar(...)`. `@`-prefixed names resolve against `refs`: state actions by
the `@rulename-{bo,ao,bc,ac}` convention, alt actions / conditions / dynamic
rule names explicitly. This is the same declarative pattern as the TS plugin,
which is what keeps the two ports aligned from one grammar source.


## Re-invocation guard

`Use` re-runs a plugin on later `SetOptions` calls, so the Go plugin guards
against double-application with a decoration (`j.Decoration("markdown-init")`).
That keeps the in-code rule rewrites idempotent.


## Differences from the TS version

The TypeScript implementation is canonical and the Go package is a port. The
parity fixtures in `test/fixtures/` keep them aligned for the cases they cover,
but a few shape and behaviour differences remain:

### API shape

| | TypeScript | Go |
|---|---|---|
| Construct | `new Tabnas().use(jsonic).use(Markdown, opts)` | `jsonic.Make()` then `j.UseDefaults(Markdown, Defaults, opts)` |
| Options | a partial `MarkdownOptions` object | `map[string]any` (nested groups are `map[string]any` too) |
| Defaults merge | the plugin fills defaults itself | you pass `Defaults` to `UseDefaults` |
| Parse | `j.parse(src)` returns the value, throws on error | `j.Parse(src)` returns `(any, error)` |
| String matcher | `buildMarkdownStringMatcher` is **exported** | unexported (internal only) |

### Value types

| Value | TypeScript | Go |
|---|---|---|
| Object record | plain JS object — read fields directly | an unexported `orderedMap` struct — read by printing or use `object: false`; it is **not** type-assertable by callers and does not implement `MarshalJSON` |
| Array record | JS array | `[]any` |
| number | JS `number` | `float64` |
| `null` keyword | JS `null` | Go `nil` |
| empty / blank result | `[]` | empty `[]any` |

### Behaviour

- **`field.exact` error code.** TS throws an error whose `.code` is the
  specific `markdown_extra_field` / `markdown_missing_field`. Go returns a
  `*jsonic.JsonicError` whose `.Code` is `"unexpected"`; the specific code
  appears only in the message text. Both fail on the same inputs; only the
  surfaced code differs.

- **Non-strict embedded containers.** In TS, non-strict mode parses embedded
  JSON containers in a field — `a,b\ntrue,[1,2]` → `{ a: true, b: [1, 2] }`,
  and `{x:1}` works too. In the Go port these container forms currently
  **error** (`unexpected`); non-strict scalar handling (`true`, numbers, `trim`,
  `comment`) matches TS. The shared parity fixtures do not exercise embedded
  containers, so this gap is not caught by the fixture suite. If you need
  embedded JSON in fields today, use the TypeScript package.

Everything else — strict-mode text fields, headers, custom delimiters and
record separators, RFC-4180 quoting, `number` / `value` / `trim` / `comment`,
`record.empty`, `field.names` / `field.empty` / `field.nonameprefix`, and
streaming — behaves the same across both runtimes, as enforced by
`test/fixtures/`.


## Accepted vs rejected — worked edge cases

| Input | Options | Result | Why |
|---|---|---|---|
| `a\n"b""c"` | default | `[{[a] map[a:b"c]}]` | `""` is one escaped quote (RFC 4180). |
| `a\n b ` | default | `[{[a] map[a: b ]}]` | strict keeps significant whitespace. |
| `a\n1` | default | `[{[a] map[a:1]}]` (string) | strict: a field is text. |
| `a\n1` | `number: true` | `[{[a] map[a:1]}]` (`float64`) | number lexing. |
| `a\n# b` | `comment: true` | `[]` | the line is a comment. |
| `a\n1\n\n2` | `record.empty: true` | `[{[a] map[a:1]} {[a] map[a:]} {[a] map[a:2]}]` | blank becomes empty record. |
| `a,b\n1,2,3` | `field.exact: true` | error (`.Code unexpected`) | width must match header. |
| `a,b\ntrue,[1,2]` | `strict: false` | error (`unexpected`) | embedded container — Go port gap. |
