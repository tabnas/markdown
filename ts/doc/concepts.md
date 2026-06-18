# Concepts: how @tabnas/markdown works (TypeScript)

Background and design rationale. This explains *why* the plugin is shaped the
way it is — useful when extending it or debugging surprising output. For the
API see the [reference](reference.md).


## It is a grammar plugin, not a parser

`@tabnas/markdown` does not contain a parser. It is a **plugin for the tabnas
engine** that, together with the `@tabnas/jsonic` base grammar, configures the
engine to read delimited records. The pipeline is:

```
new Tabnas()        // the engine: a generic table-driven lexer + Pratt-ish rule parser
  .use(jsonic)      // load the relaxed-JSON grammar (val/map/list/pair/elem rules, tokens)
  .use(Markdown)    // layer the record grammar on top, then call .parse()
```

Because it builds on jsonic, the plugin inherits jsonic's lexer (numbers,
strings, comments, value keywords, line tracking) and its rule machinery. The
markdown plugin's job is to (1) change which tokens are significant, (2) add the
`markdown` / `newline` / `record` / `text` rules, and (3) re-shape the
`list` / `elem` / `val` rules so a "list" means "a row of fields".

> The package is named *markdown* but the grammar it installs is a
> record/field (CSV-family) grammar — header rows, `,`-separated fields, one
> record per line, RFC-4180 quoting. It was built from the `@tabnas/csv`
> template and keeps that record model.


## The record model

A document is a sequence of **records** (rows). A record is a **list** of
**fields** separated by a delimiter (`,` by default). A record ends at a record
separator (a newline by default) or end of input. Mapped onto the grammar
rules:

- `markdown` — the entry rule. Loops over rows, dispatching blank lines to
  `newline` and content rows to `record`.
- `newline` — absorbs one or more blank lines (so leading, trailing, and
  interior blank lines don't become records, unless `record.empty` is set).
- `record` — one row: it pushes `list`, then closes at a newline or end of
  input. Its close action (`@record-bc`) is where a row becomes an object or
  array, applies the header, and pushes the result onto the output.
- `list` / `elem` / `val` — the field machinery: a list of comma-separated
  elements, each element a value.
- `text` — concatenates `VAL`/`SP` tokens into one field string, which is how
  in-field whitespace and multi-word values survive.

`markdown`, `newline`, `record`, and `text` are defined in the embedded grammar
text. `list`, `elem`, and `val` are configured **in code** — see [Why three
rules live in code](#why-three-rules-live-in-code).


## What makes whitespace and newlines significant

In ordinary jsonic, whitespace (`#SP`), newlines (`#LN`), and comments (`#CM`)
are all *ignored* tokens — the parser never sees them. A record grammar can't
work that way: a newline ends a row, and a space is part of a field.

So the plugin rewrites the engine's `IGNORE` token set:

- `#LN` is **removed** from `IGNORE` in every mode, so row breaks are
  structural. The grammar's `markdown` / `newline` / `record` rules then match
  on `#LN` explicitly.
- `#SP` is **removed** in strict mode, so spaces inside fields reach the parser
  and the `text` rule can keep them. In non-strict mode `#SP` stays ignored
  (jsonic's value lexer handles spacing), which is why non-strict defaults
  `trim` to on.
- `#CM` stays ignored, so comments are dropped — *if* comment lexing is enabled
  (the `comment` option toggles whether `#` even produces comment tokens).


## Strict vs non-strict

This is the central design choice.

**Strict mode (the default).** jsonic's value parsing is switched off:
`rule.exclude: 'jsonic,imp'` drops the relaxed alternatives, and the JSON
structural tokens `#OB` `#CB` `#OS` `#CS` `#CL` (`{ } [ ] :`) are disabled. A
field is therefore *raw text*. Numbers and keywords are only recognised if you
opt in with `number` / `value`. Quoting uses the plugin's own RFC-4180 string
matcher (doubled quotes), not jsonic's backslash strings. This matches how
standard tabular-text tools behave and keeps field contents predictable.

**Non-strict mode (`strict: false`).** jsonic's `val` / `list` / `map`
alternatives are preserved, so a field body can be a JSON object (`{x:1}`), an
array (`[1,2]`), or a backslash-escaped string. To make the common case useful,
non-strict also defaults `trim`, `comment`, `number`, and `value` to on. The
trade-off: a field is now parsed as an expression, so trailing junk after a
container is a syntax error (`{x:1}y` → `unexpected`).

The two modes are not just an option flag — they change which token set is
ignored, which rules are excluded, and how the `list`/`elem`/`val` rules are
built. That is why the plugin branches heavily on `strict`.


## Why three rules live in code

`list`, `elem`, and `val` are configured with `tn.rule(...)` in
`markdown.ts`, not in the grammar text, because the two modes need *different*
versions of them:

- In **strict mode** the rules are rebuilt from scratch (`.clear()`). Each rule
  owns its node: `list` allocates the field array, `elem` pushes one field,
  `val` coalesces a child value / token / implicit null. Reusing jsonic's
  defaults here would flatten records via jsonic's map/pair lifecycle and
  produce `[object Object]` keys.
- In **non-strict mode** the rules *keep* jsonic's default alternatives (so
  embedded `[...]` / `{...}` still parse) and the markdown record alternates
  are layered around them — prepended for the empty-field cases, appended after
  the jsonic structural alts so embedded containers win first. Only the
  corrupting jsonic `bo`/`bc` lifecycle actions are replaced with clean,
  record-aware ones.

The grammar file can't express "keep the existing alternatives and add mine",
so that logic is code.


## How a row becomes a record

The close action of the `record` rule (`@record-bc`) does the per-row work:

1. The first non-blank record, when `header: true`, is captured as the **field
   names** (`ctx.u.fields`) and *not* emitted.
2. Every later record's field array is turned into the output value:
   - with `object: true`, an object keyed by `field.names`/header, with extras
     under `field.nonameprefix + index`;
   - with `object: false`, the array as-is.
3. Missing/`undefined` fields are replaced with `field.empty`.
4. If `field.exact` is set and the field count differs from the header length,
   it raises `markdown_extra_field` / `markdown_missing_field`.
5. The record is pushed onto the result array — or, when `stream` is set,
   handed to the callback and *not* stored.

The header/object/empty/exact options all funnel through this one action, which
is why they only matter for the value shape, not for tokenising.


## The embedded grammar and `@`-refs

The four record rules are authored once in `markdown-grammar.jsonic` (a relaxed
JSON file) and embedded verbatim into `ts/src/markdown.ts` (and into
`go/markdown.go`) between `BEGIN/END EMBEDDED` markers by `embed-grammar.js`,
which runs as part of `npm run build`. Never hand-edit the embedded block.

At plugin-init the grammar text is parsed by a **separate** jsonic instance
(`new Tabnas().use(jsonic).parse(grammarText)`) into a grammar spec, its `ref`
map is set, and the spec is applied with `tn.grammar(...)`. Names prefixed with
`@` in the grammar resolve against that `refs` map:

- State actions are auto-wired by the `@rulename-{bo,ao,bc,ac}` convention
  (`@markdown-bo`, `@record-bc`, `@text-bc`).
- Alt actions, conditions, and dynamic rule names are referenced explicitly
  (`@text-follows`, `@not-record-empty`, `@record-close-next`,
  `@text-space-push`).

This declarative-grammar-plus-refs pattern is the standard tabnas plugin
style, and it is what lets the same grammar drive both the TypeScript and Go
ports.


## Token legends

The plugin registers human descriptions for its tokens via `config.modify` →
`cfg.tokenDesc` (the `'markdown-tokendesc'` modifier). `@tabnas/railroad` reads
those off the live config when it draws the railroad diagram, which is why the
legend in `grammar.svg` / `grammar.txt` explains `LN`, `SP`, `CA`, `VAL`, and
the embedded-JSON tokens.


## Accepted vs rejected — worked edge cases

| Input | Options | Result | Why |
|---|---|---|---|
| `a\n"b""c"` | default | `[{ a: 'b"c' }]` | `""` is one escaped quote (RFC 4180). |
| `a\n b ` | default | `[{ a: ' b ' }]` | strict mode keeps significant whitespace. |
| `a\n b ` | `trim: true` | `[{ a: 'b' }]` | trim strips the edges only. |
| `a\n1` | default | `[{ a: '1' }]` | strict: a field is text, `'1'` not `1`. |
| `a\n1` | `number: true` | `[{ a: 1 }]` | number lexing turns it numeric. |
| `a\n# b` | default | `[{ a: '# b' }]` | comments off in strict: `#` is text. |
| `a\n# b` | `comment: true` | `[]` | the line is a comment, skipped. |
| `a\n1\n\n2` | default | `[{a:'1'},{a:'2'}]` | the blank line is skipped. |
| `a\n1\n\n2` | `record.empty: true` | `[{a:'1'},{a:''},{a:'2'}]` | blank becomes an empty record. |
| `a,b\n1,2,3` | default | `[{a:'1',b:'2','field~2':'3'}]` | extra field gets a generated key. |
| `a,b\n1,2,3` | `field.exact: true` | throws `markdown_extra_field` | row width must match the header. |
| `a,b\ntrue,[1,2]` | `strict: false` | `[{a:true,b:[1,2]}]` | non-strict parses embedded JSON. |
| `a\n{x:1}y` | `strict: false` | throws `unexpected` | trailing junk after the container. |


## Where to look in the code

| Concern | Location in `ts/src/markdown.ts` |
|---|---|
| Defaults | `Markdown.defaults` |
| Mode setup (excludes, IGNORE, string matcher) | the `if (strict) … else …` blocks near the top |
| Engine option overrides | `jsonicOptions` |
| Record/field/header logic | the `refs` map, esp. `@record-bc` |
| In-code rules | the `tn.rule('list' | 'elem' | 'val', …)` blocks |
| RFC-4180 quoting | `buildMarkdownStringMatcher` |
| Embedded grammar text | between the `BEGIN/END EMBEDDED` markers |
