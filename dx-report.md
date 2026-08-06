# Markdown DX Report — prose rescope (CommonMark/GFM subset)

**Repo:** `@tabnas/markdown` (was CSV-family) → prose Markdown  
**Branch / date:** 2026-08-06, TS-first then Go  
**Decisions (from `admin/doc/design/agent-parsing-review-2026-08-06.md` addendum):** rescope to full CommonMark/GFM prose, `debug/go:Model()` parity, fixtures ad-hoc, jsonic-base pattern blessed, `Safe/Strict` presets, `tabnas-verify` gate.

---

## 1. Starting point audit (what DX looked like before)

* **Name lied.** `AGENTS.md` warned "Don't be misled… parses delimited rows, not GFM" while `README.md` and `package.json` still said "markdown syntax". Playground preset would mis-route. Lesson: template copy from `csv` left `markdown-grammar.jsonic` + `test/fixtures/*.csv` intact — no one had updated `consts.ts` blurb or `ts/doc/*` after the fork.
* **Reuse was accidental.** Both `ts/src/markdown.ts` and `go/markdown.go` import `jsonic` because csv does. New prose parser has no CSV needs — the dependency is now pure cost (peerDep graph, `Embed` mismatch). Hybrid pattern (csv vs yaml) needs a decision early: *bare engine vs jsonic*.
* **Fixture IR split.** `test/spec/*.tsv` (input→expected→opts, unescaped `\n`) vs `test/fixtures/*.csv→.json` + `manifest.json` (the csv mirror). Same data expressed two ways — agents copied the nearer sibling and recreated the split elsewhere. Keeping "ad-hoc per plugin" (per maintainer choice) means we keep both, but we must be explicit in `test/AGENTS.md` which one is canonical for prose.
* **Grammar embed was doc, not logic.** `markdown-grammar.jsonic` is 40 lines defining `markdown/newline/record/text` plus three code-driven `list/elem/val` rules. Railroad `ts/doc/grammar.svg` therefore showed CSV — not prose. Any prose successor must either make the diagram honest (via `@tabnas/railroad` extraction from live `Tabnas`) or stop claiming the embedded grammar is the source of truth.
* **Tests are CSV-shaped.** `parity.test.ts` (`loadSpec` + `unescape`) is reusable; the fixture *content* however assumes `object:true` record arrays. Prose fixtures need a different expected shape (document AST) but can reuse the same loader.

## 2. Design decisions for the prose parser

### 2.1 Why a bare-engine JS parser inside a tabnas plugin?

`zon/TEMPLATE.md` §3 says "not JSON-family → ABNF or bare engine". Markdown is not JSON-shaped, so jsonic base is dropped. ABNF for CommonMark exists but is large and indentation-sensitive in ways RFC 5234 repetitions don't capture well (lists, blockquotes, reference link resolution). 

**Choice:** keep the plugin API (`new Tabnas().use(Markdown).parse(src) → Document`) and make the engine *host* a JS block parser rather than encoding every indentation rule as declarative alts. Block structure is still driven by tabnas rule `markdown` whose `bo` action calls `parseDocument(src, opts)` and whose `open`/`close` just match `#ZZ`. Inline structure (emphasis/strong/code/link/image/autolink/strikethrough) is parsed by a JS scanner invoked from each block's action.

This keeps:
- one grammar to maintain (JS), diff-friendly, checkable before run,
- parity achievable TS→Go by porting the same functions,
- `debug.model()` honest (single `markdown` rule, plus helpers exposed as refs for diagram),
- playground + share-links safe (grammar remains declarative-ish; no inline `a: (r)=>…` needed beyond the wrapper).

Trade-off: the railroad for `markdown` will be a single `document → block` fan-out rather than a fine-grained CFG. Documented as intentional — the prose complexity lives in the JS helper, not in alt ordering.

### 2.2 AST shape (mdast-adjacent)

Chosen to be familiar to JS/Go consumers and to `remark`/`mdast` tooling without importing them:

```js
{ type:'document', children: Block[] }
Block = Heading | Paragraph | Blockquote | List | ListItem | Code | Html | ThematicBreak
Heading   = { type:'heading', depth:1..6, children: Inline[] }
Paragraph = { type:'paragraph', children: Inline[] }
Blockquote= { type:'blockquote', children: Block[] }
List      = { type:'list', ordered:boolean, start:number|null, children: ListItem[] }
ListItem  = { type:'listItem', children: Block[], spread?:boolean }
Code      = { type:'code', lang:string|null, meta:string|null, value:string }
Html      = { type:'html', value:string }
ThematicBreak = { type:'thematicBreak' }
Inline = Text | Emphasis | Strong | InlineCode | Link | Image | Break | Delete
Text      = { type:'text', value:string }
Emphasis  = { type:'emphasis', children: Inline[] }
Strong    = { type:'strong', children: Inline[] }
InlineCode= { type:'inlineCode', value:string }
Link      = { type:'link', url:string, title:string|null, children: Inline[] }
Image     = { type:'image', url:string, title:string|null, alt:string }
Break     = { type:'break' }
Delete    = { type:'delete', children: Inline[] } // GFM strikethrough, when gfm:true
```

Plain strings are never returned — the document node is always the root, matching how `remark` parses. For callers that want HTML, a `toHtml(doc)` helper is out-of-scope for this strand but noted as a follow-up.

### 2.3 Scope — what "fully working" means in this PR

**In:**

- Blocks: ATX headings (`#`…`######`), Setext headings (`===`/`---` underline), thematic breaks (`***`/`---`/`___`), fenced code (```` ``` ````/`~~~` with info string), indented code (4-space), blockquotes (`>`), unordered lists (`-`/`*`/`+`), ordered lists (`1.`/`1)` with `start`), paragraphs, HTML blocks (raw line starting with `<` until blank). ATX heading content parsed as inline.
- Inline: escapes (`\*`), code spans (`` ` ``), emphasis (`*`/`_`), strong (`**`/`__`), links `[text](url "title")`, images `![alt](url)`, autolinks `<http://…>`, hard breaks (two trailing spaces or `\\` + newline or literal `\n` inside paragraph source), GFM strikethrough (`~~`) when `gfm:true`.
- Reference-style links are deferred (link definitions collected but resolution is single-pass inline; full reference map requires two passes — noted below).
- Tables and task lists deferred (GFM tables need block-level look-ahead of 2 lines and pipe handling that deserves its own follow-up). The `gfm` flag gates strikethrough only for now; table/task support is recorded as a gap instead of shipped half-baked.

**Deferred on purpose:**

- Full CommonMark reference-link resolution (2-pass), footnote extensions, definition lists, math.
- Loose vs tight list `spread` is computed but not yet used to insert `<p>` wrappers — spec says loose items wrap para children — but keeping the boolean honest lets HTML renderers decide.
- Setext heading detection that must not mistake thematic break or list marker is careful but not exhaustive.

### 2.4 Options

Previous `MarkdownOptions` (trim/comment/number/header/object/field/record/string…) were CSV-shaped. New shape:

```ts
type MarkdownOptions = {
  gfm?: boolean   // default true — enables strikethrough + autolink extensions; tables/taskLists recorded as future
  breaks?: boolean // default false — when true, single soft breaks become <br> (GFM `breaks`); otherwise soft = space
}
```

Keep `Markdown.defaults = { gfm:true }` so existing `new Tabnas().use(Markdown)` stays zero-config.

### 2.5 Dependency change

Drop `@tabnas/jsonic` peerDep. The plugin depends only on `@tabnas/parser`. `ts/package.json` peerDependencies becomes `"@tabnas/parser": ">=0"`, devDependencies keep `file:../../parser/ts` + debug/railroad.

## 3. Observations while implementing (rolling)

* **Lexer order was irrelevant once the parser short-circuits.** The old `stringmarkdown` matcher at `1e5` (RFC4180 `""` quoting) conflicted with inline code `` ` ``. Removing jsonic lets `order` be driven by code fences and escapes only. Lesson: when the prose parser lives in the `bo` action, lex config shrinks to `lex.emptyResult` and `rule.start`.
* **`ctx.src()` is the seam.** `Context.src:()=>string` (from `parser/ts/src/context.ts`) is the only stable way to read the full source in a state action. The alternative `lex.src` is not exposed on `Context`. Any rescope needs to use that function, not a `token.src`.
* **`rule.clear()` is required when overriding `markdown`.** Existing siblings `json`, `yaml`, `toml` all `exclude:'jsonic,imp'` because they inherit rules. Prose parser clears the rule before defining it, otherwise the inherited `val` alts try to match Markdown's leading `#` as a `#TX`. Verified against `csv`'s strict-mode path.
* **Inline inside block is the real work.** Rough LOC split: block scanner ~250 lines, inline scanner ~220 lines. Block handles indentation/fence/lists/blockquote; inline handles delimiter stacks. Porting Go will be dominated by the inline `findClosingBracket`/`findClosingParen` loops and the strong vs emphasis precedence.
* **Test strategy must mix fixtures + unit cases.** `test/spec/*.tsv`'s `input→expected(JSON)` is ideal for small isolated cases (one heading, one list). `test/fixtures/*.md→*.json` is better for multi-line documents. We keep `parity.test.ts`'s `unescape` + directory discovery but point it at `test/spec` (TSV) plus an optional `test/fixtures`-style directory for prose documents. Keeping "ad-hoc" per maintainer means we document *both* and let the suite run them.

## 4. Risks / mitigations

| Risk | Mitigation |
|---|---|
| Prose tests differ shape from csv tests, so existing `go test ./...` will go red until fixtures are replaced | Keep csv fixtures under `test/fixtures/csv-legacy/` (optional) or delete and bump semver; note breaking change in README/CHANGELOG |
| Inline emphasis inside links (`[**a**](url)`) needs recursive descent | `parseInline` recurses on inner slice and merges `text` buffers — tested with `[*a*]` and nested `**a *b* c**` |
| GFM tables look like paragraphs plus pipes — premature table detection would misclassify ordinary `a \| b` prose | Tables are **not** enabled in this slice; `gfm` gates only strikethrough; table detection deferred to follow-up PR with explicit `gfmTables` flag |
| Go parity drift on string escapes and emphasis edge cases (`_inside_word_` should not be emphasis per CommonMark) | Document underscore rule as simplified (any `_*_`) for this slice; note exact CommonMark underscore word-boundary rule as known gap |

## 5. Follow-ups queued (not in this slice)

* `debug/go:Model()` — separate strand, needs `debug/go/model.go` structured type (queued per §10 decision 2).
* `Tabnas.Safe/Strict` presets — engine-level PR, not markdown.
* `tabnas-verify` gate — admin-level wrapper.
* Markdown HTML stringifier (`toHtml`) and GFM table/task support.

---

*This file is append-only during the rescope — new dated entries go at the bottom.*
