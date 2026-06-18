# Reference: @tabnas/markdown (TypeScript)

Complete, dry reference for the public API, every option, and the grammar the
plugin accepts. For a guided introduction start with the
[tutorial](tutorial.md); for task recipes see the [how-to guide](guide.md).


## Package

```bash
npm install @tabnas/markdown @tabnas/parser @tabnas/jsonic
```

| | |
|---|---|
| Package | `@tabnas/markdown` |
| Module type | CommonJS (`main: dist/markdown.js`, `types: dist/markdown.d.ts`) |
| Peer dependencies | `@tabnas/parser` (>=2), `@tabnas/jsonic` |
| Node | >=24 |
| License | MIT |


## Exports

```ts
import { Markdown, buildMarkdownStringMatcher } from '@tabnas/markdown'
import type { MarkdownOptions } from '@tabnas/markdown'
```

| Export | Kind | Purpose |
|---|---|---|
| `Markdown` | `Plugin` | The plugin. Register with `tn.use(Markdown, options?)`. |
| `Markdown.defaults` | `MarkdownOptions` | The default option values (see below). |
| `buildMarkdownStringMatcher` | function | Factory for the custom RFC-4180 double-quote string lexer. Exported for advanced/embedding use; the plugin wires it in itself. |
| `MarkdownOptions` | type | The option object type. |


## Entry point

There is no standalone parse function. The plugin installs itself into a
tabnas instance; you call that instance's `.parse()`:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown /* , options */)

j.parse('a,b\n1,2') // => [{ a: '1', b: '2' }]
```

- `jsonic` **must** be loaded before `Markdown` — the plugin rewrites the
  jsonic grammar's record/field rules and overrides the start rule.
- `options` is an optional partial `MarkdownOptions`; omitted keys take their
  defaults. Nested groups (`field`, `record`, `string`) merge per-key.
- `.parse(src)` returns `any[]`:
  - an **array of objects** when `object: true` (default), or
  - an **array of arrays** when `object: false`,
  - `[]` for empty input, all-blank input, or when a `stream` callback is set.
- Parse errors throw; the thrown error carries a `.code` (see
  [Error codes](#error-codes)).


## Options

`Markdown.defaults`:

```ts
type MarkdownOptions = {
  trim: boolean | null
  comment: boolean | null
  number: boolean | null
  value: boolean | null
  header: boolean
  object: boolean
  stream: null | ((what: string, record?: Record<string, any> | Error) => void)
  strict: boolean
  field: {
    separation: null | string
    nonameprefix: string
    empty: any
    names: undefined | string[]
    exact: boolean
  }
  record: {
    separators: null | string
    empty: boolean
  }
  string: {
    quote: string
    markdown: null | boolean
  }
}
```

### Top-level options

| Option | Type | Default | Effect |
|---|---|---|---|
| `trim` | `boolean \| null` | `null` | Strip leading/trailing whitespace from each field. `null` resolves to `false` in strict mode, `true` in non-strict mode. Internal whitespace is always kept. |
| `comment` | `boolean \| null` | `null` | Treat `#` lines as comments (skipped) and strip trailing `#...` from a field. `null` resolves to `false` in strict mode, `true` in non-strict. |
| `number` | `boolean \| null` | `null` | Parse numeric literals (`1`, `2.5`, `1e2`) as numbers. `null` resolves to `false` in strict, `true` in non-strict. |
| `value` | `boolean \| null` | `null` | Parse the keywords `true` / `false` / `null` as their JSON values. `null` resolves to `false` in strict, `true` in non-strict. |
| `header` | `boolean` | `true` | Use the first non-blank record as field names (keys). With `false`, the first record is data. |
| `object` | `boolean` | `true` | Emit each record as an object (`true`) or an array (`false`). |
| `stream` | `null \| (what, record?) => void` | `null` | Streaming callback. When set, records are delivered to the callback and `parse()` returns `[]`. See [Streaming](#streaming). |
| `strict` | `boolean` | `true` | Strict mode disables jsonic value parsing — every field is raw text. With `false`, fields may hold embedded jsonic. See [Strict vs non-strict](#strict-vs-non-strict). |

### `field`

| Option | Type | Default | Effect |
|---|---|---|---|
| `field.separation` | `null \| string` | `null` (comma) | Field delimiter. A custom value replaces the `#CA` token; may be multi-character (e.g. `'~~'`, `', '`). |
| `field.nonameprefix` | `string` | `'field~'` | Prefix for keys of extra fields beyond the header/`names` (object output only). Key is `nonameprefix + index`, e.g. `field~2`. |
| `field.empty` | `any` | `''` | Value used for a missing or empty field. |
| `field.names` | `undefined \| string[]` | `undefined` | Explicit field names. With `header: false` they key the objects; extra fields fall back to `nonameprefix`. |
| `field.exact` | `boolean` | `false` | If `true`, a record whose field count differs from the header/`names` length throws `markdown_extra_field` or `markdown_missing_field`. |

### `record`

| Option | Type | Default | Effect |
|---|---|---|---|
| `record.separators` | `null \| string` | `null` (newline) | Characters that end a record, replacing the default newline handling (engine `line.chars` / `line.rowChars`). |
| `record.empty` | `boolean` | `false` | If `true`, blank lines become empty records instead of being skipped. |

### `string`

| Option | Type | Default | Effect |
|---|---|---|---|
| `string.quote` | `string` | `'"'` | The field quote character used by the markdown string matcher. |
| `string.markdown` | `null \| boolean` | `null` | Controls the custom RFC-4180 (`""`-escaped) string matcher. In strict mode the matcher is on unless this is `false`. In non-strict mode it is off unless this is `true` (non-strict fields otherwise use jsonic's own string lexer with `\`-escapes). |


## Output shapes

| `header` | `object` | `field.names` | Result |
|---|---|---|---|
| `true` | `true` | — | Array of objects keyed by the header row. (default) |
| `true` | `false` | — | Array of arrays; the header row is consumed (not emitted). |
| `false` | `true` | unset | Array of objects keyed by `field~0`, `field~1`, … |
| `false` | `true` | set | Array of objects keyed by `names`; extras under `nonameprefix`. |
| `false` | `false` | — | Array of arrays; every row (including the first) is data. |


## Streaming

When `stream` is set, `parse()` returns `[]` and the callback is invoked as the
parse proceeds:

| `what` | `record` argument |
|---|---|
| `'start'` | — |
| `'record'` | the parsed record (object or array, per `object`) |
| `'end'` | — |
| `'error'` | the thrown `Error` |

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const out = []
const j = new Tabnas().use(jsonic).use(Markdown, {
  stream: (what, record) => { if (what === 'record') out.push(record) },
})

j.parse('a,b\n1,2\n3,4') // => []
out // => [{ a: '1', b: '2' }, { a: '3', b: '4' }]
```


## Error codes

Thrown on `parser.parse(...)`; read `error.code`.

| `code` | Cause |
|---|---|
| `markdown_extra_field` | `field.exact` is set and a row has more fields than the header. |
| `markdown_missing_field` | `field.exact` is set and a row has fewer fields than the header. |
| `unterminated_string` | A quoted field has no closing quote. |
| `unexpected` | Unexpected token — e.g. trailing junk after an embedded container in non-strict mode. |


## Grammar / syntax accepted

The plugin defines a record grammar on top of jsonic. The start rule is
`markdown`.

```
markdown  = ( newline / record )*           ; zero or more rows
newline   = LN+ record? / ZZ                 ; blank line(s) between records
record    = list                             ; one row of fields
list      = elem ( CA elem )*                ; comma-separated fields
elem      = val?                             ; a field (may be empty)
val       = text / VAL                       ; field value (strict)
          / text / map / list / VAL          ; field value (non-strict)
text      = ( VAL / SP )+                     ; raw text with significant spaces
```

Tokens (the railroad diagram legend describes these):

| Token | Meaning |
|---|---|
| `LN` | newline — ends a record / row |
| `SP` | whitespace — significant inside a field |
| `CA` | comma — field separator (replaceable via `field.separation`) |
| `VAL` | a field value: text, number, string, or keyword |
| `ZZ` | end of input |
| `OB` `CB` `OS` `CS` `CL` `KEY` | embedded-JSON structure: `{` `}` `[` `]` `:` and a map key — only active in non-strict mode |

Structural rules:

- A **record** is one or more fields separated by the field delimiter, ending
  at a record separator (newline by default) or end of input.
- A **field** may be empty (two adjacent delimiters, a leading/trailing
  delimiter); empty fields yield `field.empty`.
- A **blank line** is skipped (default) or yields an empty record
  (`record.empty: true`). Leading and trailing blank lines are always skipped.
- A **quoted field** (`string.quote`, default `"`) may contain the delimiter,
  newlines, and doubled quotes (`""` → `"`).
- In **strict mode**, the embedded-JSON structural tokens are disabled and
  every field is text (plus optional `number`/`value` lexing).
- In **non-strict mode**, the jsonic `val` / `list` / `map` alternatives are
  preserved, so a field can be a JSON container or backslash-escaped string.

The live grammar as a railroad diagram is in
[`grammar.svg`](grammar.svg); a vertical ASCII rendering is in
[`grammar.txt`](grammar.txt). Both are generated from the embedded grammar by
[`@tabnas/railroad`](https://github.com/tabnas/railroad).


## `buildMarkdownStringMatcher`

```ts
function buildMarkdownStringMatcher(
  options: MarkdownOptions,
): (cfg: Config, opts: TabnasOptions) => (lex: Lex) => Token | undefined
```

A factory returning a lexer-matcher factory for the RFC-4180 double-quote
string format (where `""` is an escaped quote). The plugin installs it
automatically based on `string.markdown` and `strict`; it is exported for
callers building their own lexer stacks.
