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

---

# 2026-08-06 — CommonMark 0.31.2 rewrite (652/652, both runtimes)

**Repo:** `@tabnas/markdown` **Date:** 2026-08-06, TS-first then Go
**Outcome:** the line-scanner prose parser described in §2 above is gone,
replaced by a full CommonMark 0.31.2 implementation in both runtimes.

## 6. What was replaced, and why not incrementally

The parser described in §2.1–§2.3 was a single left-to-right line scanner
(`parseDocument` ~250 lines, `parseInline` ~220) that classified each line
in a fixed precedence order and recursed on the interior. Measured against
the vendored 0.31.2 suite it scored **roughly 40%**. The failures were not
a list of missing features that could be worked through one at a time;
they came from three structural commitments, each of which the spec's own
algorithm (Appendix A, "A parsing strategy") makes in the opposite
direction:

* **One pass over the line, deciding as it went.** Lazy continuation
  requires the opposite: a line that fails a block quote's `>` test may
  still continue a paragraph inside it, so unmatched containers must be
  closed *after* the search for new block starts, not when their marker
  goes missing. A scanner that classifies a line before it knows whether
  anything else matched cannot express that. Nor could it express setext
  underlines, link reference definitions, or list tightness, all of which
  are decided when a block is *finalized* rather than when it is opened.
* **Local decisions for emphasis and links.** §2.2 hoped a delimiter walk
  would do. It cannot: whether a `*` run opens, closes, both or neither
  depends on what surrounds it, on what unmatched runs are still behind
  it, and on the lengths of both (the rule of three). Emphasis needs a
  delimiter stack resolved in a second pass, and links need a bracket
  stack for the same reason. Retrofitting a stack into a scanner that had
  already committed to emitting nodes in scan order was a rewrite of the
  inline half regardless.
* **Nodes built as children-arrays, top-down.** Both phases splice nodes
  mid-walk — the block phase closes containers from the tail of an open
  spine, the inline phase rewrites sibling runs in place. Every splice was
  an index rewrite. The replacement is a linked tree (`node.ts` /
  `node.go`), projected to the public AST afterwards.

§2.1's premise — *keep the plugin API, host the parser in the `bo` action,
do not encode indentation rules as declarative alts* — survived intact and
is the reason the rewrite was contained. What did not survive is the
implementation behind it. The tabnas seam (`ctx.src()`, `rule.clear()`,
the disabled lexers) is unchanged; only what happens after `ctx.src()` was
replaced.

## 7. Results

* **CommonMark 0.31.2: 652/652 in both TypeScript and Go**, all 26
  sections. Byte-for-byte HTML comparison against the vendored suite,
  which is now at `test/commonmark/spec.json`.
* **Parity: 652 examples × 4 option combinations (`gfm` × `breaks`) =
  2608 records, 0 differing ASTs and 0 differing HTML outputs.**

  Worth separating what was *verified once* from what is *enforced
  continuously*, because they are not the same set:

  - Verified, as one-off comparison runs during the port: AST and HTML
    over those 2608 records; then, after the `sourcepos` unit was fixed,
    the **native trees** over the same 2608 records — including
    `sourcepos` — plus a 200-case astral/BMP corpus built to exercise the
    column boundary. Zero differences in all three.
  - Enforced by committed tests: the conformance suite in each runtime,
    the 36 shared `.tsv` fixtures, and
    `TestSourcePosColumnsCountCharacters` (4 cases per runtime).

  The gap between those two lists is the honest caveat. The comparison
  that runs in CI covers the two public outputs, and it is structurally
  blind to `sourcepos` — the projection drops it and the HTML does not
  encode it. That is exactly why the runtimes could disagree on astral
  columns while every automated check stayed green, and why the column
  unit now has a direct test rather than relying on the parity net. A
  standing cross-runtime tree comparison would close the gap properly and
  is queued in §10.
* All **36** shared fixtures in `test/spec/*.tsv` pass in both runtimes,
  **untouched**. The public AST is otherwise unchanged by the rewrite,
  which is the strongest evidence available that this was a conformance
  fix and not a redesign of the output.
* Reproduce: `npm run conformance` from `ts/` (no build step, no engine
  needed) or `cd go && go test -run TestCommonMarkSpec -v ./...`.

Two AST changes callers may notice, both listed in the README:

* `spread` on lists and items now follows mdast semantics. It was
  previously latched by whether the file ended in a newline, so
  `- a\n- b\n` and `- a\n\n- b\n` produced identical output and the field
  carried no information at all. §2.3 recorded it as "computed but not yet
  used"; it was in fact not computed.
* Inline raw HTML now produces `{type:'html', value:'<b>'}` inline nodes.
  There was previously no inline node type for it and tags leaked into
  `text`.

## 8. Corrections to earlier sections

Stated here as corrections rather than edited above, per this file's
append-only rule.

* **§2.4 was wrong about `gfm`.** The option's comment read "enables
  strikethrough + autolink extensions". Autolink literals — bare `www.` /
  `https://` without angle brackets — were never implemented, and are
  still not. The flag gates **strikethrough only**. It gates nothing else
  because there is nothing else to gate. §2.3 and the §4 risk table were
  accurate on this point ("`gfm` gates only strikethrough"); §2.4
  contradicted them and the README inherited §2.4's version. The package
  parses CommonMark, with one GFM extension; that is the phrasing to use.
* **§2.3's deferred list is now partly resolved.** Delivered: full
  reference-link resolution (two-phase, definitions collected during
  paragraph finalization), HTML blocks (all seven types, not "raw line
  starting with `<` until blank"), entity and numeric character
  references, complete emphasis and strong emphasis including the
  underscore word-boundary rule §4 flagged as a known gap, and loose/tight
  list handling — which is real now, not merely an honest boolean.
  Still outstanding, and still deliberate: **GFM tables, task list items,
  autolink literals, footnotes**, and disallowed-raw-HTML filtering.
  Definition lists and math remain out of scope.
* **§2.3's "in scope" list undersold what shipped and oversold what
  worked.** It is superseded by the README's statement of the whole
  0.31.2 surface.
* **§5's queued `toHtml` is delivered.** `toHtml(src, opts)` /
  `ToHTML(src, opts)` render CommonMark-conformant HTML. §2.2 called a
  stringifier "out-of-scope for this strand"; the rewrite made it
  mandatory rather than optional, because the specification defines
  conformance as HTML output. Without a renderer there is no way to make
  "652/652" mean anything. The renderer is therefore the measuring
  instrument first and a public convenience second. `parseTree()` /
  `ParseTree()` plus `renderHTML` / `RenderHTML` expose the same path in
  two steps for callers who want to walk or mutate in between.
  The GFM table/task support queued alongside it is **not** delivered.
* **The HTML is not sanitized**, and nothing in §2 anticipated that this
  would need saying. CommonMark passes raw HTML blocks and inline tags
  through verbatim by specification: `<script>alert(1)</script>` in, the
  same out. GFM's disallowed-raw-HTML filter is not implemented. Every
  document that shows HTML output now carries this warning; keep it there.
* **§1's fixture-IR split is resolved**, not by choosing between the two
  shapes but by giving each a distinct job: `test/spec/*.tsv` pins the
  **AST** and is the TS/Go parity contract; `test/commonmark/spec.json`
  scores **HTML** and is the conformance contract. Neither can do the
  other's job — a projection change can leave 652/652 untouched, and the
  suite runs with `gfm:false` so it can never cover strikethrough.
  `test/AGENTS.md` documents both.
* **§1's "grammar embed was doc, not logic" is now stated plainly rather
  than hedged.** `markdown-grammar.jsonic` is **inert**: block structure
  is decided by the line algorithm in `block.ts` / `block.go`, and the
  `markdown` rule exists only to consume the token stream so the engine's
  trailing-content check passes. §2.1 predicted "a single
  `document → block` fan-out"; the railroad diagram it actually produces
  is empty — a bare track with no boxes on it. The file is kept solely because
  `ts/embed-grammar.js` embeds it. Documentation should say so rather than
  imply the grammar drives anything.

## 9. Cleanups landed with the rewrite

* **Go is genuinely bare-engine.** `go/go.mod` now requires
  `github.com/tabnas/parser/go` and nothing else — no jsonic, no indirect
  requirements. §2.5 decided this and the TypeScript side did it;
  `AGENTS.md` then claimed it for both while `go/go.mod` said otherwise.
  The claim and the file now agree.
* **`tabnasmarkdown.Make()` exists.** `README.md` documented it for a long
  time before it did. It was never caught because `go/doc/*.md` is not
  auto-executed, unlike the TypeScript examples. Go examples are now
  verified by compiling them against the real package; the process rule is
  in `AGENTS.md`.
* **RFC-4180 leftovers deleted**: `BuildMarkdownStringMatcher`, and
  `test/fixtures/` with its 258 orphaned CSV files. §1 called these
  "template copy from `csv` left intact"; they are gone rather than
  archived.
* **Adversarial-input tests added** — `go/robust_test.go`, Go side only so
  far; the TypeScript counterpart is outstanding. Markdown is routinely
  parsed from untrusted sources, so nothing may panic, hang, or go
  super-linear. Two cases guard real defects of the previous Go
  implementation, both of which the port's shape invited: a code fence
  over 1000
  characters panicked in Go, because the closing-fence regex interpolated
  the fence length into a `{n,}` bounded repeat and RE2 caps those at
  1000 — untrusted input could abort the host process; and non-ASCII text
  was corrupted because the inline scanner appended `string(text[i])`, a
  byte, so every UTF-8 continuation byte widened into its own code point.
* **Two executable contracts** are now in place and are documented as
  such in `AGENTS.md`: the conformance corpus, and the `// =>` assertions
  in the documentation, which `ts/test/doc-examples.test.ts` and
  `ts/tools/check-doc-examples.mjs` run. Neither may be weakened to make a
  change pass.

## 10. Follow-ups still queued

* GFM tables, task list items, autolink literals, footnotes,
  disallowed-raw-HTML filtering. Each would need its own option, not a
  widening of `gfm` — see the §8 correction.
* A TypeScript counterpart to `go/robust_test.go`. The adversarial corpus
  is expressible as inputs that must merely not blow up, so it belongs in
  both runtimes.
* A **standing** cross-runtime native-tree comparison. The tree-level
  check described in §7 was run by hand and found nothing, but nothing
  re-runs it, so positional drift between the runtimes would go unnoticed
  again. It needs a dump-and-diff step in CI, not a test inside either
  runtime, since neither can see the other.
* `debug/go:Model()`, `Tabnas.Safe/Strict` presets and the `tabnas-verify`
  gate (§5) remain untouched by this strand.

# 2026-08-07 — three GFM extensions in TypeScript (17/24, still 652/652)

## 11. What landed

Task list items, disallowed raw HTML and autolink literals, in the
TypeScript runtime only. With strikethrough that is four of the six GFM
extensions; tables and footnotes remain unimplemented. The GFM extension
corpus (`test/gfm/spec.json`, 24 examples) goes from 3/24 to **17/24** —
every non-table example — and CommonMark stays at **652/652**.

## 12. Corrections to earlier sections

* **§10's "each would need its own option, not a widening of `gfm`" is
  reversed.** All four extensions are gated on the single `gfm` flag. The
  argument for per-extension flags was that `gfm` had only ever meant
  strikethrough, so widening it would change behaviour for existing
  callers with `gfm` at its default. That is true, and it is the right
  trade anyway: a caller asking for `gfm` is asking for the GitHub
  dialect, not for one operator from it, and four booleans is
  configuration surface nobody asked for. The behaviour change is real
  and is documented in every README. The escape hatch is `gfm:false`,
  which now means "plain CommonMark, byte for byte" — a stronger and more
  useful guarantee than it had before.
* **§8's "the flag gates strikethrough only … there is nothing else to
  gate" no longer holds**, and neither does "the package parses
  CommonMark, with one GFM extension" as the phrasing to use. §8 was
  accurate when written.

## 13. Where each extension lives, and why not in the inline scanner

The governing constraint was that 652/652 must survive `gfm:true` as well
as `gfm:false`. Two of the three could have been written as new branches
of the inline scan; none of them was.

* **Task list items — the block phase.** `markTaskListItems` runs at the
  end of `block.ts`'s `parse`, over each item's first paragraph's
  `stringContent`, before the inline phase exists. Deciding it on raw
  text is what makes the marker beat the inline scanner: with `[x]: /url`
  defined in the document, `- [x] foo` is still a task item and not a
  link. It also settles `checked` before either `ast.ts` or `html.ts`
  looks at the tree. The marker's required trailing whitespace is matched
  as a space or tab, not "any whitespace" — a line ending there would
  mean the content starts on the next line, and consuming it would
  swallow the paragraph's first soft break.
* **Autolink literals — a post-pass over the finished inline tree.**
  `linkifyAutolinks` in `inline.ts`, which is how cmark-gfm does it. The
  scanner that decides code spans, raw HTML, emphasis and links is not
  touched at all, so an autolink literal cannot win against a construct
  CommonMark says comes first — that precedence is structural rather than
  something a new scanner branch has to remember. It consolidates
  adjacent `text` siblings before scanning, because the scanner emits a
  node per delimiter run: `a.b-c_d@a.b` arrives as three siblings and the
  address is only visible once they are one string. `link` and `image`
  subtrees are skipped (no links in links; an image's children are its
  alt text).
* **Disallowed raw HTML — the renderer.** `filterDisallowedTags` in
  `html.ts`, applied to `html_block` and `html_inline` output only. The
  node's literal and the AST's `value` keep the original text, which is
  what the extension specifies: it is a filter on the output, not a
  rewrite of the document.

## 14. `renderHTML` had to learn `gfm`, and how it defaults

The tag filter is the first render-time GFM behaviour, and `renderHTML`
can be called as `renderHTML(tree)` with no options — which is exactly
what `ts/tools/conformance.mjs` does, on trees parsed with `gfm:false`.
Defaulting `gfm` to `true` there would have applied the filter to a plain
CommonMark parse and cost conformance examples that contain `<script>`,
`<style>` and `<textarea>`.

So the block phase records the flag on the **document** node
(`MdNode.gfm`) and the renderer defaults to it; an explicit
`options.gfm` still wins. The tree now says which dialect produced it,
which is the honest answer to "what should a renderer do with a tree it
did not parse".

## 15. Keeping the autolink pass linear

Autolink literals are the one piece here with an adversarial cost
profile, and two shapes of input go quadratic if written naively.

* **Repeated failing candidates with a long greedy scan.** Every valid
  start position (`^`, whitespace, `*`, `_`, `~`, `(`) begins a candidate
  whose "non-space non-`<`" run can reach the end of the paragraph, so
  `'(http://'.repeat(n)` is n scans of length n. Two fixes: the domain is
  validated *before* the run is scanned, so a candidate that cannot match
  costs only its own domain run; and the run end is cached, since
  candidates are tried left to right and every candidate inside one run
  shares its end.
* **A long domain run shared by many candidates.** `_` both continues a
  domain and opens a candidate, so `'_www.a_b.c'.repeat(n)` gives every
  underscore a candidate whose domain reaches the end of the text. The
  domain scan is therefore capped at 253 characters (RFC 1035's limit on
  a fully-qualified name); anything past the cap is still scanned, just
  as the path rather than as the domain. The email local part is capped
  at 64 (RFC 5321) for the same reason, and that bounds the rewind from
  each `@`.

The parenthesis rule counts `(` and `)` once and then maintains the
counts as trailing `)` are dropped, rather than recounting per drop — a
deeply parenthesised URL is otherwise quadratic in its own length. All of
this is asserted: `ts/test/commonmark.test.ts` has a 4x-input ratio test
on the `_www.a_b.c` shape and exact-output tests on 20 000-deep
parentheses.

## 16. Deliberate divergences from cmark-gfm

* **A domain must contain a period, for URLs as well as for `www.`.**
  cmark-gfm passes `allow_short` for the scheme forms, so it links
  `http://localhost/x`. The GFM spec text says "followed by a valid
  domain", and a valid domain has at least one period; this follows the
  spec.
* **The domain is re-validated after trailing punctuation is trimmed.**
  cmark-gfm validates only before, so `www..` becomes a link whose text
  is `www`. Re-validating rejects that. No corpus example distinguishes
  the two.
* **Raw `<a>` tags do not suppress autolinking.** `<a href="x">www.b.com</a>`
  produces a nested anchor, because the outer `<a>` is an `html_inline`
  literal and not a `link` node. cmark-gfm behaves the same way; a
  renderer that cares needs a sanitizer, which it needs anyway.
* **The two length caps in §15 are semantic, not only budgetary.** The
  §15 wording ("anything past the cap is still scanned, just as the path
  rather than as the domain") describes the common case but understates
  two edges, and both are divergences in their own right:
  * An underscore past the 253rd character of a domain run no longer
    invalidates the domain, because validation never sees it. `www.` +
    248 characters + `_b.c` is correctly rejected; the same shape with
    249 characters is linked. cmark-gfm rejects both.
  * An email local part of 65 characters or more is not recognised at
    all — the rewind from the `@` stops at 64 and lands mid-word, which
    is not a valid autolink start. cmark-gfm has no such limit.

  Neither is corpus-visible, no real domain or address reaches either
  bound, and both runtimes agree — the caps are what keep the pass
  linear on `'_www.a_b.c'.repeat(n)`. Recorded here because they are
  observable behaviour, not just a performance measure.

## 17. Follow-ups this leaves

* **The Go port.** Same file-for-file placement as above. It is not
  optional housekeeping: `test/spec/*.tsv` now pins `listItem.checked`,
  which only TypeScript projects, so five parity rows fail in
  `go test ./...` until the port lands. That is the parity corpus doing
  its job — it is supposed to go red when the canonical runtime moves
  ahead — and it must not be "fixed" by editing the fixtures.
* **GFM tables**, the remaining 7/24. Deliberately out of this change:
  tables need block-level look-ahead of two lines and pipe handling, and
  they are the one extension that changes how ordinary prose containing
  `|` is parsed.
* **Footnotes**, still unimplemented in both runtimes.
* The §10 items not touched here — a TypeScript counterpart to
  `go/robust_test.go` (partly addressed: the autolink robustness block in
  `commonmark.test.ts` is adversarial-input testing, but only for this
  feature), and the standing cross-runtime native-tree comparison.

---

# 2026-08-07 — the Go port of the three GFM extensions (17/24 in both, still 652/652)

## 18. What landed

The three extensions §11–17 describe in TypeScript, ported to Go
file-for-file: `markTaskListItems` in `block.go`, `linkifyAutolinks` in
`inline.go`, `filterDisallowedTags` in `html.go`, `Checked`/`HasChecked`
and `GFM` on `MdNode`, and `listItem.checked` in `ast.go`. Placement,
control flow and names match the canonical runtime; a side-by-side diff
of the two files reads as one implementation.

Scores after the port:

* GFM extension corpus: **17/24 in Go**, the same 17 as TypeScript. The
  seven failures are all Tables. `go test -run TestGFMSpec -v ./...`
  prints the same per-section table `ts/tools/gfm-conformance.mjs` does.
* CommonMark: **652/652 in Go**, unchanged.
* `go test ./...` is green, including the five `test/spec/*.tsv` parity
  rows §17 flagged as expected failures. They were the port's first job
  and they are closed; the fixtures were not touched.

## 19. Corrections to earlier sections

* §17's first follow-up — "The Go port" — is **done**. The parity corpus
  did exactly what it was supposed to: it went red the moment the
  canonical runtime moved ahead, and green again when the port caught up.
* AGENTS.md, `README.md`, `go/README.md` and all four `go/doc/*.md`
  quadrants said "one GFM extension (strikethrough)" and "GFM's
  disallowed-raw-HTML filter is not implemented". All of that is now
  false and has been corrected. The root README's extension matrix no
  longer says "not yet" for Go.

## 20. Where Go could not copy TypeScript literally, and what it does instead

Four places, all forced by the language rather than chosen:

* **`RenderHTML` has no absent option.** TypeScript's
  `renderHTML(tree, options?)` defaults `gfm` from `doc.gfm` (§14); Go's
  `RenderHTML(doc *MdNode, opts Options)` always receives a resolved
  `Options`, so there is no absent case to default from. `MdNode.GFM` is
  still set on the document, and `RenderHTML(tree, Options{GFM: tree.GFM})`
  is the Go spelling of §14's behaviour. Not observable through any
  public path: `ToHTML` passes the same `Options` to both halves.
* **`checked: boolean | null` becomes `Checked` + `HasChecked`**, the
  same pair `node.go` already uses for `Title`/`HasTitle` and
  `Info`/`HasInfo`. `ast.go` gains `nullableBool` next to
  `nullableString`, so the projected field is a real JSON `null`.
* **The tagfilter regex uses a lookahead**, `(?=[\t\n\f\r />]|$)`, which
  RE2 has not. It is a hand-coded scan: find `<`, optional `/`, match one
  of the nine names with ASCII-only folding, then check the next byte.
  Same shape as the fence handling in `block.go`, and for the same reason.
* **`RE_TASK_LIST_MARKER` is a loop, not a regexp.** RE2 would match it
  correctly, but a per-item match that is only ever attempted at offset 0
  is the only shape guaranteed not to make a document of many items
  super-linear.

## 21. Byte-vs-rune: why the autolink pass can scan bytes

The port scans by byte offset like the rest of `inline.go`. That is exact
rather than merely convenient, and the argument is worth writing down
because the file's header hazard list exists for a reason:

Every character the pass branches on is ASCII. No byte of a multi-byte
UTF-8 sequence is below 0x80, so a non-ASCII character is exactly as
unmatchable in Go as its UTF-16 code unit is in TypeScript — it ends a
domain run, it is not a valid autolink start, and it is not trailing
punctuation. Every boundary the pass computes therefore sits at an ASCII
character, and no slice can cut a rune in half. The two length caps
(§15's 253 and 64) are in bytes here and UTF-16 units there, which is the
same number: both can only bind inside a run of ASCII domain characters.

Verified rather than argued: see §22.

## 22. Cross-runtime verification

Three comparisons, all one-off runs (the committed nets are the two
conformance suites and the `.tsv` fixtures):

* **The corpora.** 652 CommonMark + 24 GFM examples × `gfm` × `breaks` =
  **2704 records**, comparing both the AST and the HTML. Zero differences.
  This is §7's 2608-record comparison extended with the GFM corpus and
  re-run against the ported code.
* **An adversarial corpus of 26 383 inputs** (105 532 records) built to
  hit exactly the boundaries the new code computes: a multi-byte
  character at every one of them, the full trailing-punctuation pair
  matrix, entity-like semicolons, paren balance, domain-validity edges,
  email edges, every task-marker shape, the tagfilter's lookahead, and
  random assemblies from a token alphabet. Zero differences after §23.
* **A second corpus of 44 637 multi-line documents** (178 548 records)
  assembled from markdown line fragments, to reach the interactions
  between the extensions and block structure. Zero differences.

## 23. A pre-existing divergence the fuzzing found: Go's `(?i)` is not JavaScript's `i`

The adversarial run turned up **16 mismatching records that had nothing
to do with this port**: `<ſcript>`, `<ſscript>`, `<scriptſ>`, `<linK>`
and friends were HTML blocks in Go and paragraphs in TypeScript, with
`gfm` off as well as on. Confirmed against a pristine worktree at `HEAD`.

Go's `(?i)` folds case over all of Unicode, so `(?i)script` also matches
`ſcript` (U+017F LATIN SMALL LETTER LONG S) and `(?i)[A-Za-z]` also
matches U+017F and U+212A KELVIN SIGN. JavaScript's `i` without `u`
deliberately never folds a non-ASCII code point onto an ASCII one.
`inline.go` was written around this — its comment on `inlineCDATA` says
so explicitly — but `block.go`'s four HTML-block regexes were not.

Fixed by spelling the case out: `asciiFoldPattern` turns a lowercase
literal into two-element classes (`h[1-6]` → `[hH][1-6]`), and type 7
drops `(?i)` entirely because every letter in `blockOpenTag` /
`blockCloseTag` is already a both-cases class. `TestCaseFoldingIsASCIIOnly`
in `go/robust_test.go` pins it, with the two code points written as
escapes rather than literals — they are homoglyphs, and that test is
precisely about telling them apart.

Worth noting how invisible this was: both spec corpora are pure ASCII, so
neither conformance suite could reach it, and no `.tsv` fixture holds a
non-ASCII tag name. It took a cross-runtime comparison over deliberately
non-ASCII input. That is the same lesson as §7's `sourcepos` column bug —
the automated nets check the cases someone thought of.

## 24. Follow-ups this leaves

* **GFM tables**, the remaining 7/24, still deliberately out of scope in
  both runtimes.
* **Footnotes**, still unimplemented in both.
* **The standing cross-runtime comparison** queued in §10 and §17 is now
  overdue twice. The three runs in §22 were throwaway scripts; §23 is a
  direct argument for making the adversarial half of them a committed
  job, since it is the only thing that has ever caught this class of bug.
* **A TypeScript twin of `TestCaseFoldingIsASCIIOnly`** was not added:
  TypeScript is already correct here and the defect was Go-only. It would
  still be worth having as a guard on the canonical side.

---

# 2026-08-07 — adversarial verification pass

Everything below is an independent re-run of the claims in the two
entries above, plus what that re-run found. The scoreboards hold:
TypeScript 652/652 and 17/24 (the seven failures all Tables), Go
`go test ./...` green, doc examples 35/35.

## 25. What the verification actually covered

* **Corpus integrity.** `test/commonmark/spec.json` is byte-identical to
  `origin/main`. `test/gfm/spec.json` was re-extracted from the upstream
  spec prose independently — all 24 extension examples, byte-for-byte,
  with upstream's own example numbers and nothing omitted. Neither runner
  was loosened; `ts/tools/conformance.mjs` is untouched and the GFM
  runner compares with `===` and counts a throw as a failure.
* **`gfm:false` inertness**, the highest-risk regression, checked against
  a real `origin/main` worktree over all 676 examples × `breaks`:
  **1352 records, zero HTML differences.** The AST differs in exactly one
  way — the added `checked: null` key on `listItem`, on 322 nodes — and a
  structural diff confirms there is no other change of value, key order
  or shape anywhere. The autolink post-pass does not run with `gfm:false`.
* **Cross-runtime parity**, TypeScript against Go, AST + HTML + the
  native tree including `sourcepos`: **2704 records, zero differences**
  on the corpora; **20 016 records, zero differences** on a
  deliberately non-ASCII fuzz corpus (homoglyphs, 2/3/4-byte runes and
  Unicode spaces at every boundary the new code computes); **33 172
  records, zero differences** on an entity corpus covering all 2125
  canonical names.
* **Robustness.** 60 TypeScript runs and 76 Go runs over the adversarial
  set — 50k `www.` runs, 10 000 parens each way, 10 000 task markers,
  1000-deep nesting, autolinks inside links/code/raw HTML/alt text,
  lone surrogates, NUL bytes and (Go only) genuinely invalid UTF-8:
  **no throw and no panic anywhere.**

## 26. The linearity claim, measured properly

§15's claim is correct, but the end-to-end timings that appear to
support it are not the evidence they look like. Measuring
`parse`+`renderHTML` at 4× input steps flags five shapes at 8–9×
(`'(www.a.b'.repeat(n)`, `'http://a.b)'.repeat(n)` and friends) — which
reads as n^1.5.

It is not. Timing `linkifyAutolinks` **alone**, on a paragraph holding a
single text node, gives **2.00–2.05× per doubling on every shape out to
6–16 MB** — flatly linear, including the two the domain cap exists for.
The 8–9× readings are a one-off heap/GC step in the surrounding parse
and render at one particular input size: the ratio spikes at ~640 KB and
returns to ~2.0× at the next doubling, which no quadratic term does.

Worth keeping in mind for the committed perf job §24 asks for: a
whole-pipeline timing ratio is too noisy to assert on, and the isolated
pass is both quieter and the thing actually under test.

## 27. Two pre-existing Go entity defects the fuzzing found

Same class as §23, found the same way, and — like §23 — present on
`origin/main` and unrelated to the GFM work. `go/common.go` decodes named
references with the standard library's table; two things about that table
are not §6.2's rules.

**`html.UnescapeString` matches the legacy semicolon-less aliases as a
*prefix* of a longer name.** `&ampa;` came back as `&a;` and `&nbspa;`
as U+00A0 followed by `a;`, because the stdlib consumed `&amp` / `&nbsp`
and left the tail; `decodeEntity` only checked that the whole run ended
in `;`, which it did. §6.2 admits only the exact semicolon-terminated
names, so both are literal text, which is what the TypeScript's explicit
2125-entry table gives. The gate now also rejects a decode that left part
of the reference behind — the residue is always a non-empty suffix of the
name plus the `;`, and `body` is capped at 32 characters by
`entityPattern`, so the check is bounded.

**`&nGt;` and `&nLt;` are in the WHATWG table but not the stdlib's.**
`html.UnescapeString` returns them unchanged, so Go emitted them
literally where TypeScript emitted U+226B/U+226A + U+20D2.
`missingEntities` supplies the two; the test below proves they are the
only gaps.

Both are pinned by `go/entities_test.go`, which reads
`ts/src/entities.ts` — the canonical generated table — and asserts Go
agrees on **every one of the 2125 names**, and on the near-misses formed
by appending a character to each. That last assertion has to be stated as
"decodes iff the table has it", not "does not decode": some real names
are proper prefixes of other real names (`sup` → `sup1`, `le` → `leq`,
`colone` → `coloneq`), which is exactly why a prefix-matching decoder
looked plausible in the first place.

This is the third defect in this class (§23, and both halves of this
section). The pattern is now unmistakable: **every place Go leans on a
standard-library text routine where TypeScript spells the rule out is a
divergence candidate**, because the stdlib implements HTML5's rules and
this parser implements CommonMark's. `go/common.go`'s header already
flags entity decoding and Unicode punctuation as the two such places;
entity decoding has now been wrong twice.

## 28. Corrections to earlier sections

* **§13 and `AGENTS.md` overstated the reference-definition claim.**
  Deciding the task marker on raw text does not make it "beat a
  `[x]: /url` reference definition" — `- [x]: /url` *is* a reference
  definition, and the item comes out empty. What it beats is the
  reference *link* that definition would otherwise produce: with `[x]:
  /url` in the document, `- [x] foo` is a task item while `- [x]foo`
  (no space, so no marker) renders as `<a href="/url">x</a>foo`.
  `AGENTS.md` has been corrected; the comment in `block.ts` was already
  precise.
* **§15's caps are semantic, not only budgetary** — written up in §16,
  where the other divergences are.

## 29. Follow-ups this leaves

* Everything in §24 still stands, and §23's argument for a committed
  adversarial cross-runtime job is now stronger by two more defects.
  The generator that found these is worth rebuilding as a committed
  job rather than a throwaway: it is templates with a hole, filled with
  a character set chosen for byte-vs-rune and case-folding hazards.
* **Audit the remaining stdlib leans in `go/common.go`** against the
  TypeScript, in the same table-driven way `entities_test.go` now does
  for entities. `isUnicodePunctuation` is the one left.
* **A TypeScript-side entity table test** is not needed — `entities.ts`
  *is* the table — but nothing currently asserts that the generator's
  output still matches upstream `entities.json`.

---

# 2026-08-07 — GFM tables: the extension set is complete (24/24, still 652/652)

## 30. What landed

Tables, in both runtimes, in one change: block detection in `block.ts` /
`block.go`, rendering in `html.ts` / `html.go`, projection in `ast.ts` /
`ast.go`, three native node types (`table`, `table_row`, `table_cell`) in
`node.ts` / `node.go`. With this the **GFM extension set is complete**:
tables, task list items, autolink literals, strikethrough and disallowed
raw HTML — five extensions, all gated on `gfm` (default `true`).

Scores: GFM extension corpus **24/24 in both runtimes**, up from 17/24,
with the Tables section going 0/8 → 8/8 and nothing else moving.
CommonMark **652/652 in both, unchanged**, all 26 sections. The parser is
conformant to CommonMark 0.31.2 and that is now the claim the READMEs and
the doc quadrants make, each with the command that runs the vendored
suite.

## 31. The public AST is mdast's, and has no header flag

The three public node types are the mdast ones, spelled exactly as mdast
spells them:

    { type: 'table', align: ('left'|'right'|'center'|null)[], children: TableRowNode[] }
    { type: 'tableRow',  children: TableCellNode[] }
    { type: 'tableCell', children: Inline[] }

`align` has one entry per column, `null` where the delimiter cell carried
no colon. Every row has exactly as many cells as `align` has entries: the
block phase pads short rows with empty cells and truncates long ones, so
no consumer has to reconcile a ragged table. Go projects `align` as
`[]any` with `nil` entries rather than a typed slice, so it marshals as
`[null, "center"]` and matches the TypeScript JSON byte for byte.

The one shape decision worth arguing is what is *not* there. The native
tree carries `isHeaderRow` / `IsHeaderRow` on the row, because the
renderer needs it to place `<thead>` and to choose `th` over `td`. The
projection drops it: **mdast has no header flag, and the first row is the
header row by convention.** Inventing a `header: true` field would have
been more informative and less useful — every existing mdast consumer
already reads position, none of them read a field this parser made up,
and a public AST that is "mdast plus one" is not mdast. The ordering the
tree already guarantees carries the same information.

Alignment goes on the table rather than on each cell for the same reason:
mdast puts it there, it is a property of the column and not of the cell,
and a 200-cell table would otherwise repeat it 200 times.

## 32. Why tables were held back for their own change

§17 deferred tables out of the first GFM change and §24 kept them
deferred. That was right, and the reason is narrower than "they are
bigger": **tables are the only extension that changes how ordinary prose
is parsed.**

The other four cannot. Task list items rewrite an item's own first
paragraph, autolink literals are a post-pass over a finished inline tree,
disallowed raw HTML is a render-time filter, and strikethrough is a
delimiter run that CommonMark leaves unclaimed. None of them can alter
what a document's blocks *are*. Tables can: any paragraph line containing
`|` is a candidate header row, and the line after it decides. A document
that meant nothing by its pipes must still parse as it did.

Two behaviours are the load-bearing ones, and both come from where the
block start is tried:

* the table start is attempted **last**, after every built-in start has
  refused the line — which is where cmark-gfm reaches its extensions
  from. So `foo` over `---` is still a setext `<h2>`, and `- | -` is
  still a list item holding the text `| -`, because the setext and
  list-item starts get the line first.
* a delimiter row with no paragraph above it is just a paragraph.

Bundling that with three extensions that provably could not move core
conformance would have made a regression hunt read the wrong diff.

## 33. Paragraph splitting — the part that could regress core conformance

A table is recognised when a delimiter row follows a paragraph whose
**last line** has a matching cell count. The header row is that last line
only, so a paragraph with earlier lines is split: the lines above stay a
paragraph, and the table opens after it.

    para line one
    h1 | h2
    --- | ---
    a | b

    <p>para line one</p>
    <table>…

This is the only place in the parser where a block start reaches back
into an already-open block and rewrites it, which is exactly why it was
the risk. Three details make it safe:

* the paragraph above is **finalized**, not discarded, so it still
  contributes its link reference definitions. `[ref]: /u` on the first
  line of a split paragraph still resolves a `[ref]` later in the
  document.
* the header row's cell count must equal the delimiter row's, or there is
  no table and the line is nothing special — the paragraph continues, as
  GFM spec example 6 requires.
* "the paragraph's last line" is O(1). `lastAddedLine` / `lastAddedTo` on
  the parser record what `addLine` last wrote and where; the accumulated
  `stringContent` is only touched on the split branch, at most once per
  table. Re-deriving the last line by scanning accumulated content would
  have made every pipe-bearing paragraph quadratic in its own length.

Delimiter cells are hyphens with an optional leading and/or trailing
colon and nothing else; one `+`, one interior space, one stray character
and the line is not a delimiter row. Leading and trailing pipes are
optional and may differ from row to row. The table ends at a blank line
or at the start of another block.

Padding is the one part of a table whose node count is not bounded by its
input — a 10 000-column header followed by 10 000 one-cell rows is 60 KB
of source asking for 10^8 cells — so the empty cells inserted to pad
short rows are capped document-wide by `MAX_AUTOCOMPLETED_CELLS` /
`maxAutocompletedCells`, which is cmark-gfm's cap and is there for
cmark-gfm's reason. Everything else in the extension is linear.

## 34. Escaped pipes resolve before the inline phase

`\|` does not split a cell, and it becomes a literal `|` at *split* time
— before inline parsing, not during it. That ordering is not cosmetic: it
is the only way a code span inside a cell can contain a pipe, because the
inline scanner treats a code span's content as verbatim and would have
kept the backslash.

    | f\|oo  |
    | ------ |
    | b `\|` az |

renders `f|oo` in the header and `b <code>|</code> az` in the body.

An escaped trailing pipe is content, not the optional row delimiter, so
the backslash run in front of a trailing `|` is counted for parity — an
even-length run leaves the pipe unescaped and it is dropped as the
delimiter. The split loop rewrites by segment rather than by character,
so the common case (a cell with no escapes) still costs one slice.

## 35. A header-only table emits no `<tbody>`

`<thead>` and `<tbody>` have no nodes of their own — GFM's table model
has rows, not sections — so the renderer writes them from the row it is
on. `<thead>` comes from the row flagged as the header; `<tbody>` is
written by the **first body row** rather than unconditionally, which is
what makes a header-only table come out as

    <table>
    <thead>
    <tr>
    <th>abc</th>
    <th>def</th>
    </tr>
    </thead>
    </table>

with no empty `<tbody>` in it. Writing `<tbody>` when the table opened
would have been one line shorter and wrong against the corpus.

## 36. Verification

* **CommonMark held**: 652/652 in both runtimes, all 26 sections,
  byte-for-byte HTML against the vendored 0.31.2 suite.
* **GFM reached 24/24** in both runtimes, every section asserted in both.
  Go's runner no longer reports a section without asserting it.
* **`gfm:false` is byte-identical to the pre-tables commit**: both
  corpora plus the shared parity fixtures — 652 + 24 + 39 = **715 inputs
  × both `breaks` values = 1430 records, zero differences**, rendered by
  HEAD and by `d97dab9` and compared byte for byte. That is the check
  that matters here: the guarantee `gfm:false` makes is "plain
  CommonMark, byte for byte", and tables are the extension most able to
  break it. No `table` node is produced at all with `gfm:false`.

  The figure carried into this entry from the original verification note
  was "1360 records", which no combination of the corpora produces —
  676 × 2 is 1352, 715 × 2 is 1430. It was quoted rather than derived.
  1430 is the re-run count and is what the docs now say.
* **Behaviour beyond the eight upstream examples** is pinned per runtime
  in `ts/test/commonmark.test.ts` and `go/gfm_test.go`, mirrored
  case-for-case: delimiter-row forms, inconsistent pipes, paragraph
  splitting, setext and list-item precedence, escaped pipes, padding and
  truncation, tables nested in block quotes and list items, two adjacent
  tables, the AST projection including `align: [..., null]`, `gfm:false`
  inertness, and the bounded-padding and linearity properties.

## 37. Corrections to earlier sections

* **§11, §17, §18 and §24 are superseded on tables.** "17/24", "the seven
  failures are all Tables", "GFM tables, the remaining 7/24, still
  deliberately out of scope in both runtimes" — all closed. The corpus is
  24/24 in both runtimes.
* **§11's "four of the six GFM extensions" was a miscount of the
  denominator.** The GFM spec's extension set is five, and footnotes are
  not in it — they are a GitHub product feature with no section in the
  spec suite. The set is now complete at five of five; footnotes remain
  unimplemented and are not counted against that.
* **"36 shared fixtures" in §7, `AGENTS.md` and `test/AGENTS.md` has been
  stale since the 0.31.2 rewrite**, which added three rows to
  `inline.tsv` in the same commit that recorded the number. There are
  **39** cases in 8 files. Both agent guides now say 39.
* **`AGENTS.md`'s extension walk-through said "the four newer
  extensions"**, which stopped being a usable label once tables joined
  them. It now covers all five, strikethrough included, and names the
  three files tables occupy.
* The disallowed-raw-HTML filter is now described in `AGENTS.md` as what
  it is — nine tag names escaped, and **not a sanitizer**. Nothing about
  the code changed; the earlier text simply left room to read it as one.
* **§13's "652/652 must survive `gfm:true` as well as `gfm:false`" was
  never true, and neither was `go/doc/concepts.md`'s claim that it is a
  structural property.** Measured in both runtimes, `gfm:true` scores
  **643/652** — six HTML-blocks examples (170, 171, 172, 173, 176, 178)
  where the tagfilter escapes the `<script>`, `<style>` and `<textarea>`
  the suite expects verbatim, and three Autolinks examples (608, 611,
  612) where text the suite expects to stay literal becomes a link.
  Those nine are the extensions behaving exactly as specified, which is
  why the conformance figure is measured with `gfm:false`, where the
  specification's own options are. The real structural property is the
  one worth claiming: `gfm:false` is byte-identical to a pure-CommonMark
  parse. Both quadrant `concepts.md` files now say this.

## 38. What is still unimplemented, and what `gfm` must not become

* **Footnotes.** Not in the GFM spec suite. Worth stating in the docs
  because `[^1]` is a valid CommonMark link label, so a footnote authored
  on GitHub renders here as a **broken link** with no error — a silent
  wrong answer, which is the kind that needs documenting most.
* **Everything outside CommonMark+GFM**: math, front matter, definition
  lists, heading attributes, admonitions, wiki links, emoji shortcodes,
  highlight, sub/superscript. Each would need its own opt-in flag.

§12 argued for one flag over four booleans, and that argument holds
exactly as far as the GFM dialect goes: a caller asking for `gfm` is
asking for GitHub's Markdown, and now gets all of it. It does not extend
past that. `gfm` must not grow into "every extension anyone wants", or it
stops meaning anything a caller can reason about — and the one collision
already on the record is the argument in miniature: `H~2~O` becomes
`H<del>2</del>O` under `gfm:true`, because GFM's single-tilde
strikethrough occupies the syntax other dialects use for subscript. A
`sub`/`sup` extension folded into the same flag would have to fight
strikethrough for it.

## 39. Follow-ups this leaves

* **`test/spec/*.tsv` does not cover the table node types.** `table`,
  `tableRow` and `tableCell` are public AST and are pinned only by
  hand-mirrored unit tests in each runtime; the parity corpus is what
  keeps the runtimes honest without anyone remembering to mirror. A
  `table.tsv` would close that, and is the same gap §17 flagged when
  `listItem.checked` was TypeScript-only.
* **The standing cross-runtime native-tree comparison** queued in §10,
  §17 and §24 is now overdue three times. Tables add `sourcepos` on rows
  and cells, which the AST drops and the HTML does not encode, so it is
  invisible to every committed check.
* **Footnotes** remain unimplemented in both runtimes. If they are ever
  added they need their own flag, not a widening of `gfm`.

## 40. The table padding cap is behaviour, not only a budget

`MAX_AUTOCOMPLETED_CELLS` (0x80000, cmark-gfm's figure and cmark-gfm's reason)
bounds one thing tables do that nothing else in the parser does: a table's node
count is not bounded by its input length. A 2000-column header over 8000
one-cell body rows is 69 KB of Markdown asking for 16 million cells, because
every short row is padded out to the header width.

The cap makes that a true ceiling rather than a slow path. Measured, holding the
header at 2000 columns and quadrupling the body:

| rows | input | output | time |
|---|---|---|---|
| 500 | 25 KB | 5.0 MB | 1102 ms |
| 2000 | 34 KB | 5.1 MB | 930 ms |
| 8000 | 69 KB | 5.2 MB | 959 ms |

Sixteen times the rows, flat output and flat time.

Recorded here for the same reason as the two autolink caps in §16: it is a
budget *and* an observable behaviour. Past the cap, padded cells stop being
emitted, so a sufficiently pathological table renders with rows shorter than its
header. Both runtimes share the figure and agree. No real document approaches
it — reaching the cap needs the column count multiplied by the row count to
exceed half a million — but a caller who hits it should find it written down
rather than have to infer it.

The wider point, which §16 made about autolinks and applies again: every cap
that exists to keep a pass linear is also a semantic edge, and the two are worth
stating together rather than filing one under performance.

## 41. 2026-08-07 — conformance re-verified against upstream, fixture counts corrected

Both claims were re-run from scratch, and both hold:

* **CommonMark 0.31.2 — 652/652, both runtimes.** `cd ts && npm run
  conformance` and `cd go && go test -run TestCommonMarkSpec -v ./...` each
  report 652/652 across all 26 sections.
* **GFM extensions — 24/24, both runtimes.** `node ts/tools/gfm-conformance.mjs`
  and `go test -run TestGFMSpec -v ./...` each report 24/24 across all five
  sections.

The vendored corpora were checked against upstream rather than taken on trust,
which is the step none of the earlier verification sections recorded:

* `test/commonmark/spec.json` is **byte-identical** to
  `https://spec.commonmark.org/0.31.2/spec.json`
  (sha256 `d431b29d97b6f73e69d547109cf5081578fac931e72afe95639ebe766c1b2a20`,
  140487 bytes, 652 examples). The score is therefore a score against the
  published suite, not against a local copy of it.
* `test/gfm/spec.json` matches the 24 `example table` / `example autolink` /
  `example strikethrough` / `example tagfilter` / `example disabled` blocks
  extracted from `github/cmark-gfm`'s `test/spec.txt` — same markdown, same
  expected HTML, zero differences, and no example present here that is not
  upstream.

The parity figure §36 and the READMEs quote was re-measured the same way:
676 examples (652 + 24) × 4 `gfm` × `breaks` combinations = **2704 records**,
comparing both the projected AST and the rendered HTML, TypeScript build against
Go build. **0 differing ASTs, 0 differing HTML outputs.** The `gfm:true` figure
for the CommonMark suite is likewise confirmed at **643/652** — six in HTML
blocks (the tag filter) and three in Autolinks, which is the extensions working.

The eight `go/README.md` Go examples were executed against the real package in a
scratch module — the manual check AGENTS.md asks for, since no harness runs Go
blocks — and every one produced exactly its documented output, `Make()`
included.

### Correction to §17, §24, §39 and the doc set: the fixture count

`test/spec/` was documented everywhere as **39 cases in 8 files**. It has been
**75 cases in 10 files** for some time: `autolink.tsv` (23), `blockquote.tsv`
(2), `code.tsv` (4), `heading.tsv` (7), `inline.tsv` (13), `list.tsv` (6),
`mixed.tsv` (1), `paragraph.tsv` (4), `table.tsv` (10), `thematic.tsv` (5).
Both runners agree on 75, being directory-driven.

Two consequences the docs had not caught up with:

* §39's follow-up "a `table.tsv` would close that" is **done**. `table.tsv`
  pins `table` / `tableRow` / `tableCell`, `align`, escaped pipes, and row
  padding and truncation in both runtimes, so `test/AGENTS.md`'s "what this
  corpus does not yet pin" paragraph no longer applied and has been replaced.
  The HTML side of tables is still per-runtime and hand-mirrored.
* `listItem.checked` is carried by **eight** rows, not five — six in
  `list.tsv`, one each in `blockquote.tsv` and `mixed.tsv`.

Counts corrected in `AGENTS.md`, `test/AGENTS.md`, `README.md`,
`ts/doc/{guide,reference}.md` and `go/doc/{guide,reference,concepts}.md`. No
code changed; the corpora, the runners and the comparisons were left exactly as
they were.

One further doc fix, same rule (the text must match the code): AGENTS.md's
three-outputs table spelled the Go renderer `RenderHTML(tree)`. The signature is
`RenderHTML(doc *MdNode, opts Options)`, as the same file says correctly two
screens further down.

### The native-tree / `SourcePos` comparison, finally run

Queued in §10, §17, §24 and §39 and never done: the AST drops source positions
and the HTML does not encode them, so nothing in the repo could see a positional
divergence between the runtimes. It has now been measured, ad hoc, the same
shape as the AST/HTML run — `parseTree` / `ParseTree` over all 676 examples ×
4 option combinations, dumping each node as `{type, sourcepos, children}` in
document order and comparing the serialisations:

**2704 records, 0 differing native trees.** Structure, node types and
`sourcepos` / `SourcePos` all agree, tables included — which is what §39 was
worried about, since tables set positions on rows and cells.

Still open, and now the only part of that follow-up left: **it is not a
committed check.** The run lived in a scratch module outside the repo, so the
next positional change is as invisible to CI as it was before. Landing it wants
a fixture format that carries positions (the `*.tsv` corpus compares the AST,
which has none) or a per-runtime tree-dump golden file.

## 42. 2026-08-17 — recognizers split from drivers; the native-tree comparison is a committed check

First stage of a staged refactor whose goal is recorded here before any
engine code lands: making the plugin path *genuinely* use the tabnas engine
(the grammar is inert — §2.1, §8 — and the package is meant to be a
demonstrator), without giving up the engine-free core. The engine-free
parser stays canonical for algorithm semantics and remains the public API's
fast path; a later stage adds an engine-driven plugin path over shared
code, and CI will compare the two byte-for-byte. The rule that makes two
paths affordable is **anti-drift**: recognition logic exists once, as pure
functions; drivers — the hand loop today, engine lexer adapters later —
delegate to them and never reimplement them.

This entry lands that rule, with zero behaviour change (652/652, 24/24, the
75 shared fixtures, and the doc examples all green in both runtimes):

* `block.ts` now exports `segmentNextLine` — the single home of line
  cutting and §2.3 NUL replacement, which `BlockParser.parse` and any
  future line lexer share — plus the line recognizers the block starts
  already used inline: `isThematicBreakLine`, `matchAtxHeading`,
  `stripAtxClosing`, `matchCodeFence`, `matchClosingCodeFence`,
  `setextHeadingLevel`, `htmlBlockOpenKind`, `htmlBlockCloses`,
  `matchBulletListMarker`, `matchOrderedListMarker`, `matchTaskListMarker`,
  `isBlank`, `splitTableRow`, `parseDelimiterRow`. The starts and handlers
  now call these instead of owning regexes.
* `inline.ts` now exports pure `(subject, pos)` recognizers —
  `scanCodeSpan`, `scanEscape`, `scanAngleAutolink`, `scanHtmlTag`,
  `scanEntity`, `scanDelimiterRun`, `classifyBreak`, `scanLinkTitle`,
  `scanLinkDestination`, `scanLinkLabel`, `scanInlineLinkTail`, and the
  position helpers — with the `InlineParser` methods reduced to wrappers
  that apply the scan result to the tree and the stacks.
* `go/block.go` / `go/inline.go` mirror the split name-for-name where Go
  did not already have the function form (Go's hand-coded recognizers
  predate this; they are unexported, which same-package drivers can reach).
* `ts/test/recognizer.test.ts` and `go/recognizer_test.go` pin the result
  shapes in both runtimes.

And it closes §41's one open follow-up. The native-tree comparison is now
a **committed check**, not an ad-hoc scratch run: `test/spec/tree/*.json`
holds a canonical serialisation of `parseTree` over every fixture input —
`type`, `sourcepos`, literals, fence data, `listData`, `tableAlign`,
`checked`, `gfm` — generated from the canonical TypeScript
(`MD_TREE_GOLDEN=write npm test` after a build) and asserted by BOTH
runtimes (`ts/test/tree-golden.test.ts`, `go/tree_golden_test.go`) against
the same files. A positional divergence between the runtimes is no longer
invisible to CI.

Follow-ups, in landing order: fixtures for the corpus-blind cases (link
tails containing backticks, tables nested under block quotes, a single
trailing space before a line ending) plus a differential harness comparing
the public engine-free path against the plugin path over the corpora and
seeded random documents — that gate must precede any engine-driven stage;
then the engine block driver; then the engine inline phase.

## 43. 2026-08-17 — the blind-spot fixtures and the differential gate

The gate §42 named as "must precede any engine-driven stage" is landed, in
both runtimes, before any engine code exists — so every later stage
inherits it from its first commit.

* `test/spec/mixed.tsv` (the cross-construct fixture file) gains the
  corpus-blind behaviours as ordinary shared fixtures, with their native
  trees in `test/spec/tree/mixed.json`: a
  backtick inside a *successful* link destination or title followed by a
  code span (`` [a](b`c) `d` `` — the case a flat pre-lex of the inline
  phase gets wrong), the failing-tail
  precedence case, a table nested under a block quote (delimiter row at a
  container-adjusted offset) with its `gfm:false` byte-identity twin, a
  single trailing space before a soft break, and a mid-paragraph hard
  break. None of these appears in the 652+24 corpus examples or the
  previous 75 fixtures; all were verified against the current parser
  before being pinned.
* `ts/test/differential.test.ts` / `go/differential_test.go` require the
  plugin path (`new Tabnas().use(Markdown, opts).parse(src)` /
  `Make(opts).Parse(src)`) to produce the same AST as the engine-free
  path, JSON-flattened, over: all 652 CommonMark examples, all 24 GFM
  examples, every shared fixture, and 200 seeded pseudo-random documents —
  each under `{}` and `{gfm:false}`. The document generator is
  byte-identical across the runtimes (same fragment pool, same xorshift32;
  verified by hashing all 200 documents in both), so a failure reproduces
  by seed number in either runtime.

Today the two paths share every line of code and the gate is trivially
green — that is the point. The moment the plugin path stops sharing the
drivers, "all suites green" stops being sufficient, and this is the check
that knows.

## 44. 2026-08-17 — the engine block driver: the plugin path stops being a bypass

Stage 3 of §42's plan. The plugin's parse is now genuinely engine-driven,
in both runtimes, with every contract green on both legs of the dual-path
CI — including the engine-path conformance suites added here
(`ts/test/engine-conformance.test.ts`, `go/engine_conformance_test.go`:
all 652 CommonMark examples and all 24 GFM examples through
`tn.parse(...)`, HTML byte-compared via a `meta.md.keepTree` handshake and
AST-compared against the engine-free path).

What the engine now owns on the plugin path:

* **Tokenization.** A custom `mdLine` matcher (`ts/src/engine-block.ts`,
  `go/engineblock.go`) registered at order 1e5 — ahead of every built-in,
  with all built-ins disabled as deliberate configuration — emits one
  `#LB` token per physical line, cut by the shared `segmentNextLine`. The
  token's `use` payload is deliberately small: `{text, blank, tblArm}`,
  where `tblArm` (line contains `|` anywhere) is an honest superset of
  "delimiter row" for a later `gfm`-gated alt: the real decision needs
  the container-adjusted offset only the block algorithm knows.
* **Per-parse state.** `parse.prepare` — the first plugin use of that
  hook — seeds a fresh `BlockParser` on `ctx.u.md`; `block.ts` grew a
  behaviour-identical `finish()` split out of `parse()` so an incremental
  driver finalizes exactly as the batch loop does.
* **Dispatch.** Real rules: `markdown` pushes `line`; `line` consumes one
  `#LB` per iteration via `r:` tail-recursion, its action feeding the
  shared `incorporateLine`; `markdown`'s close action on `#ZZ` finalizes,
  runs the shared inline phase, and projects the AST into `rule.node`.
  `debug.model()` now shows a two-rule grammar and the old
  token-draining formality is gone.

Nothing engine-side recognizes or decides anything — the anti-drift rule
holds, which is why the differential gate stayed green on the first run.

Two side findings worth recording:

* **The old wiring was quadratic on the plugin path.** The `bo` action ran
  the whole engine-free parse once per rule instance in the `#AA` token
  drain — one full parse per lexed token. The Go differential suite
  dropped from ~60s to ~0.5s when the driver landed. The new
  `TestEngineBlockLinearity` ratio test (and a real markdown source in
  `go/perf_test.go`, retiring the `"a,b,c\n1,2,3"` CSV leftover) pins the
  linearity so it cannot regress silently.
* **`check-doc-examples.mjs`'s engine stand-in is contract-coupled.** It
  emulated the old `bo`+`ctx.src()` contract and broke the moment the
  plugin stopped using it; it now drives the plugin's own matcher and alt
  actions the way the engine's lexer and rule loop would. The stand-in is
  part of the plugin's contact surface and moves with it.

Interim inconsistency, stated: the embedded `markdown-grammar.jsonic` text
still documents the old single-rule shape. It is inert (§8) and is deleted
outright in the final stage, when `debug.model()`/railroad become the
grammar's source of truth; hand-editing the embedded block mid-stream is
forbidden by the embed contract.

Next: the engine inline phase (nested instance, sequential lexing, the
cursor-owning bracket matcher), then the GFM extension seam.
