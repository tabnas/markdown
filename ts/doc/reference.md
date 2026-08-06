# Reference: @tabnas/markdown (TypeScript)

Complete, dry reference for the public API, every option, and the AST the plugin produces. For a guided introduction start with the [tutorial](tutorial.md); for task recipes see the [how-to guide](guide.md).

## Package

```bash
npm install @tabnas/markdown @tabnas/parser
```

| | |
|---|---|
| Package | `@tabnas/markdown` |
| Module type | CommonJS (`main: dist/markdown.js`, `types: dist/markdown.d.ts`) |
| Peer dependencies | `@tabnas/parser` (>=0) |
| Node | >=24 |
| License | MIT |

## Exports

```ts
import { Markdown, parseDocument, parseInline } from '@tabnas/markdown'
import type { MarkdownOptions, DocumentNode, Block, Inline } from '@tabnas/markdown'
```

| Export | Kind | Purpose |
|---|---|---|
| `Markdown` | `Plugin` | The plugin. Register with `tn.use(Markdown, options?)`. |
| `Markdown.defaults` | `MarkdownOptions` | Default option values (`{gfm:true, breaks:false}`). |
| `parseDocument` | function | Parse a Markdown source string to a `DocumentNode` (bare, no engine). |
| `parseInline` | function | Parse inline text to `Inline[]` (bare, no engine). |
| `MarkdownOptions`, `DocumentNode`, `Block`, … | types | AST types. |

## Entry point

There is no standalone `parse` export; the plugin installs itself into a Tabnas instance:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown /* , options */)

j.parse('# Hello') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
```

- `options` is an optional partial `MarkdownOptions`; omitted keys take their defaults.
- `.parse(src)` returns a `DocumentNode` (`{type:'document', children:Block[]}`), never `[]` except for empty input.
- Parse errors do not occur for Markdown (the spec is permissive); malformed syntax is treated as plain text.

## Options

`Markdown.defaults`:

```ts
type MarkdownOptions = {
  gfm?: boolean   // default true — enables GFM extensions (strikethrough, autolink)
  breaks?: boolean // default false — when true, soft line breaks become hard breaks
}
```

| Option | Type | Default | Effect |
|---|---|---|---|
| `gfm` | `boolean` | `true` | When `true`, enables `~~strikethrough~~` and `<https://autolink>` handling. Tables/task lists are documented future extensions and are not parsed even when `gfm:true` (see `dx-report.md` §2.3). |
| `breaks` | `boolean` | `false` | When `true`, a soft line break (`\n` inside a paragraph without trailing spaces) becomes a `break` node. When `false`, soft breaks become a single space. Hard breaks (two trailing spaces before `\n`) are always `break` nodes regardless of this option. |

## AST

All nodes are plain JSON. `document` is the root.

### Document

```ts
{ type: 'document', children: Block[] }
```

### Blocks

| Type | Shape |
|---|---|
| `heading` | `{type:'heading', depth:1..6, children:Inline[]}` |
| `paragraph` | `{type:'paragraph', children:Inline[]}` |
| `blockquote` | `{type:'blockquote', children:Block[]}` |
| `list` | `{type:'list', ordered:boolean, start:number|null, spread:boolean, children:ListItem[]}` |
| `listItem` | `{type:'listItem', spread:boolean, children:Block[]}` |
| `code` | `{type:'code', lang:string|null, meta:string|null, value:string}` |
| `html` | `{type:'html', value:string}` |
| `thematicBreak` | `{type:'thematicBreak'}` |

### Inline

| Type | Shape |
|---|---|
| `text` | `{type:'text', value:string}` |
| `emphasis` | `{type:'emphasis', children:Inline[]}` (`*`/`_`) |
| `strong` | `{type:'strong', children:Inline[]}` (`**`/`__`) |
| `inlineCode` | `{type:'inlineCode', value:string}` (`` ` ``) |
| `link` | `{type:'link', url:string, title:string|null, children:Inline[]}` |
| `image` | `{type:'image', url:string, title:string|null, alt:string}` |
| `delete` | `{type:'delete', children:Inline[]}` (`~~`, when `gfm:true`) |
| `break` | `{type:'break'}` |

Inline content is produced by `parseInline(text, {gfm,breaks})`; block children produced by `parseDocument(src, opts)` recursively use it for headings, paragraphs, and list items.

## Grammar

The grammar file is `markdown-grammar.jsonic` (top-level), embedded via `ts/embed-grammar.js` into both runtimes. For prose it is intentionally a single `markdown` entry rule whose `bo` action calls `parseDocument(ctx.src(), opts)` and whose `open`/`close` (`#AA` loop / `#ZZ`) merely satisfies the engine's trailing-content check. Block structure lives in JS (see `dx-report.md` §2.1). The railroad diagram therefore shows a single `markdown` → `document` node — this is honest about where the complexity resides.

## Errors

The plugin does not throw parse errors for well-formed or malformed Markdown — per CommonMark, all input is valid Markdown and is rendered as some block (typically a paragraph). Lex errors (e.g. from a mis-configured engine string matcher) surface as `TabnasError` with `code: 'unterminated_string'`; the prose plugin disables the string/comment/number/value lexers to avoid this class for its own syntax.

## Compatibility

The 0.4 `csv`-family API (`field`/`record`/`header`/`object` etc.) is no longer supported in this package. Record parsing remains available via `@tabnas/csv`. If you previously used `new Tabnas().use(jsonic).use(Markdown)` for records, migrate to `new Tabnas().use(jsonic).use(Csv)` (see `@tabnas/csv` README).
