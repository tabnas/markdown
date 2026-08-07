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
