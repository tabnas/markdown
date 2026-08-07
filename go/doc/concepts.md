# Concepts: how @tabnas/markdown works (Go)

Background and design rationale — why the parser is shaped the way it is, what the shape
cost, and what porting it to Go changed. For the API see the [reference](reference.md);
for recipes see the [how-to guide](guide.md).

Two things are worth settling before anything else, because they decide how you read the
rest of this document.

**The AST is the primary output.** A JSON-shaped tree of blocks and inlines — here, a
`map[string]any` — is what this package exists to produce, and what `ParseDocument`
returns. Nothing else runs when you ask for it.

**There is an HTML emitter, and it is not a side utility.** The CommonMark specification
defines conformance as HTML output: the 652 examples in the suite are pairs of Markdown
and expected HTML, compared byte for byte. Without a renderer there is no way to make the
claim "652/652" mean anything. So `ToHTML` exists as the instrument that measures the
parser. That it is also useful — you can call it and get correct HTML — is a consequence
of building the measuring device honestly, not the reason it was built. More on this
below.

TypeScript is canonical and this package is a port of it; see `AGENTS.md`. The first two
thirds of this document therefore describe a design that is shared, and the last third
describes the places where Go forced a different implementation of the same behaviour.

## Two phases, and why the order is forced

The parser follows the strategy the specification itself describes in Appendix A: resolve
block structure over the whole document first, then parse inline content inside each leaf
block.

The order is not a convenience. It is forced by the fact that Markdown's block syntax is
line-oriented and its inline syntax is not, and the two disagree about what a line means.
Consider a paragraph containing an unclosed emphasis marker followed by a line that opens
a fenced code block. If you scanned for inlines first you would have to decide what the
`*` does before you know whether the text after it is even part of the same paragraph.
Conversely, a `` ` `` that opens a code span cannot suppress a block start: a code span
never spans a blank line, so block boundaries always win. Deciding blocks first makes that
precedence structural rather than something the inline scanner has to remember.

The block phase, in `block.go`, keeps a *spine* of open blocks — the document, its last
child, that block's last child, and so on. Every open block imposes a continuation
condition on the next line: a block quote wants a `>`, a list item wants a certain content
indent, a fenced code block wants anything that is not its closing fence. For each line
the parser walks that spine consuming markers, then looks for new block starts at the
offset the walk reached, then puts whatever is left into the deepest open block.

Unmatched blocks are closed *after* the search for new starts rather than during the spine
walk, and that ordering carries the whole of lazy continuation. A paragraph inside a block
quote may continue on a line with no `>` at all. If the walk closed the quote the moment
its marker went missing, the paragraph would be orphaned. By deferring the close until
something else actually matches, a plain text line finds the paragraph still open and
joins it.

Two positions are tracked along each line: an offset, used to slice the text that is kept,
and a tab-expanded display column, used for every indentation decision. They cannot be
collapsed into one. A tab may be *partially* consumed by a list marker — the marker takes
two of the tab's columns and the remaining two become content indent — and only the column
counter can express that. In Go the offset is a byte index; see
[Byte offsets, rune classification](#byte-offsets-rune-classification) for what that costs.

The inline phase, in `inline.go`, then runs over the raw text each paragraph and heading
accumulated. It is a single left-to-right scan that appends nodes as it goes. Code spans,
autolinks, raw HTML tags, entities, escapes and line breaks are all decided locally, at
the scan position, and consumed whole. Two constructs cannot be: emphasis and links.

## Emphasis needs a stack because you cannot decide an asterisk when you see it

An opening `*` is not knowable as an opening `*` at the moment it is read. Whether a
delimiter run opens emphasis, closes it, does both, or is just literal text depends on what
surrounds it, on what other runs are still unmatched behind it, and on how long those runs
are. So the scan does not decide. It emits each run as ordinary text and pushes a record of
it onto a delimiter stack; a separate pass resolves the stack afterwards, walking forward
to each potential closer and backward from there to the nearest usable opener, and
rewriting the sibling run in place when a pair is found.

The **flanking rules** are what let a run be classified at all. A run is left-flanking when
it is not followed by whitespace and either not followed by punctuation or else preceded by
whitespace or punctuation; right-flanking is the mirror image. This is how `a * b` stays
literal while `a *b* c` does not, and it is why the classification is done on the
specification's own definitions of Unicode whitespace and Unicode punctuation rather than
on a regexp character class. Those near-misses are exactly where conformance leaks: version
0.31.2 widened the punctuation class from P\* alone to P\* ∪ S\*, which is why `$`, `+`,
`<`, `=`, `>`, `^`, `` ` ``, `|`, `~` and currency symbols now count, and a P-only test
costs a handful of emphasis examples and nothing else — which makes it very hard to notice.
`common.go` spells both classes out; `isUnicodePunctuation` is `unicode.IsPunct ||
unicode.IsSymbol`, and `isUnicodeWhitespace` is the spec's §2.3 list rather than
`unicode.IsSpace`, which differs from it at the edges.

Underscore is stricter than asterisk for a reason that has nothing to do with parsing
elegance: intraword `_` is common in identifiers. `foo_bar_baz` must stay literal, and the
flanking rules produce that outcome without a special case, because the `_` runs there are
both left- and right-flanking and so are disqualified from acting intraword.

The **rule of three** is the strangest-looking part of the algorithm and the one with the
clearest motive. When a delimiter run could both open and close, matching it greedily
produces results that disagree with what people write. The rule is: if either side of a
candidate pair can play both roles, the two run lengths may not sum to a multiple of three,
unless both lengths are themselves multiples of three. It exists to make sequences like
`*foo**bar**baz*` nest the way an author means them to — one emphasis wrapping a strong —
instead of splitting into fragments. It is a heuristic, openly. The specification adopted it
because the alternatives were worse, and any implementation that wants the emphasis section
to pass has to adopt it too.

## Links need a different stack, and a pre-pass

Brackets get their own stack, for a related but distinct reason. When `[` is read the
parser does not yet know whether it will turn out to be a link, an image, a reference, or
literal text — that is only settled when a `]` arrives and the parser looks at what follows
it. So `[` and `![` are pushed, and the arrival of `]` triggers the resolution: try an
inline destination, then a full reference, then a collapsed one, then a shortcut.

The two stacks interact, and the interaction is the source of Markdown's inline precedence.
Bracket matching happens *during* the scan; emphasis matching happens *afterwards*, over
the delimiter stack. That single fact is what makes brackets bind more tightly than
emphasis. Each bracket also records where the top of the delimiter stack was when it
opened, giving emphasis resolution inside a link label a floor it may not reach below — so
a `*` outside the label cannot pair with one inside it.

Reference links need something the scan cannot provide at all: a definition that may appear
later in the document. `[foo]` on line 1 can be defined on line 900. The parser handles
this by collecting definitions during the block phase, when each paragraph is finalised. A
paragraph may begin with link reference definitions; they are consumed from its front into
a `RefMap`, and if nothing is left the paragraph disappears entirely. By the time the
inline phase runs, every definition in the document is already known, so reference
resolution is a map lookup rather than a second traversal. The map's keys are normalised —
trimmed, internal whitespace collapsed, case folded — because label matching in Markdown is
deliberately forgiving. Getting that fold right in Go took more than `strings.ToUpper`; see
[Case folding a link label](#case-folding-a-link-label).

This is also why definitions are handled at paragraph finalisation rather than by a
dedicated line scan: a definition is only a definition when it sits at the start of what
would otherwise be a paragraph, and you do not know where a paragraph starts until the
block phase has decided it.

## Two trees: a native one, and a projection

Internally the parse produces a native CommonMark node tree of `*MdNode`. The public AST is
a projection of that tree, built by `ast.go`. Having two representations is a real cost —
one more thing to keep in step — so it is worth being clear about what each is for.

The native tree is **linked**: `Parent`, `FirstChild`, `LastChild`, `Prev`, `Next`, with no
children slice anywhere. This is not stylistic. Both phases splice nodes mid-walk. The
block phase opens and closes blocks from the tail of a spine while it is walking that
spine; the inline phase's delimiter resolution removes matched delimiters and wraps the
sibling run between them in a new node, in place, while iterating over the same run. Every
one of those operations is a constant-time pointer rewrite on a linked tree, and an index
shuffle on a slice of children. The renderer's `NodeWalker` exists for the same reason: a
depth-first traversal that reports entering and exiting a container separately maps
directly onto opening and closing tags, and `ResumeAt` can reposition it mid-walk — which
is how an image skips over its own children after flattening them into an `alt` attribute.

The public AST is the opposite: plain maps and slices, no cycles, no parent pointers. It is
what callers actually want to hand to `encoding/json`, range over, or compare in a test
fixture. It has been this package's output since before the rewrite, and the rewrite kept
it that way — the shared `.tsv` fixtures pass untouched. Two things did change: raw inline
tags now get their own `html` node instead of leaking into `text`, and `spread` acquired a
meaning — see below.

The projection is deliberately lossy, in three places.

*Soft breaks collapse.* With `Breaks: false`, a soft line break becomes a single space
inside the surrounding text run instead of a node of its own. This is long-standing
documented behaviour of this package and it is what most consumers of the AST want: a
paragraph wrapped at 80 columns should not read as a sequence of fragments. The native tree
keeps real softbreak nodes, so the HTML render is unaffected, and `Breaks: true` promotes
them to `break` nodes for callers who need the distinction.

*Source positions are dropped.* The AST carries no line or column information. The native
tree keeps `SourcePos` on block nodes, which is why `ParseTree` exists: if you need to map
a node back to the source — for an editor, a linter, an error message — that is the tree to
ask.

*The block/inline distinction for raw HTML narrows.* The native tree distinguishes
`html_block` from `html_inline`; the AST calls both `html`. Their context makes them
unambiguous — one appears among blocks, the other among inlines — so the distinction is
recoverable, and one node type is simpler.

One thing the projection now does *better* is `spread`. It follows mdast semantics: a list
is spread when it is loose, an item when it holds blank-line-separated blocks. The previous
implementation latched the field off whether the file ended in a newline, so `- a\n- b\n`
and `- a\n\n- b\n` produced identical output and the field carried no information at all.
Nothing could have depended on it, which is what made the change safe to make.

The projection also has one decision that exists only in Go: every `children` slice is
allocated non-nil, and every absent optional value is an untyped `nil` rather than a missing
key. Both are `encoding/json` concerns — a nil slice marshals to `null` where an empty one
marshals to `[]`, and a missing key is not the same JSON document as a key with `null`.
The previous port left the slices nil, which silently changed the JSON of every empty
document, blockquote, list item and link label relative to the TypeScript. The
[reference](reference.md#ast) states the resulting contract exactly.

## Tight and loose lists

Tightness is the one piece of block structure that cannot be decided when the block is
opened, and it is worth understanding because it is the only place where a list's *later*
content changes how its *earlier* content renders.

A list is loose if any of its items is followed by a blank line, or if any item directly
contains two blocks separated by one. Otherwise it is tight, and a tight list omits the
`<p>` wrappers around its items' paragraphs. So the same paragraph node renders as
`<li>a</li>` or as `<li>\n<p>a</p>\n</li>` depending on something that may not appear until
several lines later.

The parser resolves this by assuming every list is tight and letting finalisation prove
otherwise, once the list is closed and all its items are known. A blank line at the very
end of the last item does not count — that blank line is what closed the list, not
something inside it.

The renderer then reads the decision back off the list rather than off the paragraph. A
paragraph checks its *grandparent*: a paragraph whose grandparent is a tight list is
necessarily some item's direct child, and drops its tags. This is why the renderer's
newline discipline matters so much. Every block asks only to "be at the start of a line",
never emitting a newline on a neighbour's or a child's behalf, so `<li>` deliberately does
not end a line and a tight item comes out as `<li>a</li>` with no interior newlines at all,
while a loose one gets its newline from its first child's own leading request. Neither case
needs to know about the other.

## Why the parser is engine-free

This package is a plugin for the Tabnas engine, but the parser inside it does not depend on
the engine. Nothing reachable from `commonmark.go` imports `github.com/tabnas/parser/go`;
`markdown.go` is the only file that mentions it. The dependency points one way and never
back.

This buys three things.

The conformance suite runs without constructing an engine. `TestCommonMarkSpec` calls
`ToHTML` directly, so when an example fails you know it is the parser, because there is
nothing else in the call. A parser entangled with a lexer would leave you bisecting between
the two. (Go compiles the package as a unit, so the engine module is still a build
requirement here — the guarantee is about the code path and the failure surface, not about
the module graph. In TypeScript, where modules are separate, the conformance run genuinely
loads no engine at all.)

It keeps `go.mod` honest. The module requires the bare engine and nothing else: no jsonic,
no indirect requirements. Earlier documentation claimed that while `go.mod` said otherwise;
it is now true, and it stays true only because there is one file that could ever add an
import.

And it makes this package a port of the parser rather than of the plugin. Because the
parser is a self-contained set of files with no engine surface, porting it is a mechanical
translation of algorithms, file for file — `block.ts` to `block.go`, `inline.ts` to
`inline.go` — and parity can be checked directly, by comparing outputs rather than
behaviour under a shared host.

The cost is that the plugin cannot use the engine's lexer for anything. It reads the raw
source through `ctx.Src` and consumes the token stream only to satisfy the engine's
trailing-content check. It also has to switch off the string, comment, number and value
lexers first, because they would otherwise mangle Markdown before the parser sees it:
backticks lex as unterminated strings, `# heading` as a comment, `1. list` as a number.

## Why there is an HTML renderer

It is worth restating the motive plainly, because "Markdown library with an HTML output
function" invites the assumption that HTML is the point.

The CommonMark specification is not a grammar with a normative AST. It is 652 worked
examples, each one an input document and the exact HTML a conformant implementation must
produce. There is no way to score a parser against it except by rendering. So the renderer
is the test instrument: it is what turns "the parse tree looks right" into a falsifiable
claim, and it is the only thing that distinguishes a parser scoring 40% from one scoring
100%, since without it neither number exists. Newline placement in `html.go` is a
correctness contract, not a formatting preference, because the comparison is byte for byte.

Having built it, there is no reason to hide it — `ToHTML` is a supported part of the API
and produces spec-conformant output. But the AST remains the primary product, and the
renderer remains, first, the thing that proves the AST was built correctly.

One consequence of taking the specification seriously deserves stating on its own: **the
HTML is not sanitized.** CommonMark requires raw HTML blocks and inline tags to pass
through verbatim, and this renderer does exactly that. A conformant renderer cannot filter
them, because filtering them would fail the suite. Anything rendering untrusted Markdown
needs a sanitizer downstream. GFM's disallowed-raw-HTML extension is implemented, but it
neutralises nine tag names and nothing else — it is not a sanitizer, and it says nothing
about attributes or `javascript:` destinations.

## What is and is not GFM

The package parses CommonMark, with four GFM extensions: strikethrough, task list items,
autolink literals and disallowed raw HTML. Tables and footnotes are not implemented, and
`GFM` gates the four as a single switch rather than as four flags — a document is either
GitHub-flavoured or it is not, and a per-extension matrix is configuration surface nobody
asked for.

Three of the four are deliberately *not* in the inline scanner. Task list markers are
consumed in the block phase, over a paragraph's raw text, before the inline scanner sees
brackets at all; autolink literals are a post-pass over the finished inline tree; the
raw-HTML filter is a rendering step. That keeps the scanner that decides code spans, raw
HTML, emphasis and links exactly as CommonMark specifies it — which is what makes
"652/652 with `GFM: true` as well as `GFM: false`" a structural property rather than a
result that has to be re-earned by every extension.

The honest statement of scope is still narrower than "CommonMark/GFM": a caller who needs
tables knows to look elsewhere rather than discovering it at runtime.

## The grammar file is inert

`markdown-grammar.jsonic` at the repository root, and the `grammarText` constant embedded
from it into both runtimes, do nothing.

They are not a simplified grammar, or a grammar that handles the easy cases with Go
handling the rest. Block structure is decided entirely by the line algorithm in `block.go`.
The file declares one rule with alts that consume the token stream so the engine's
trailing-content check passes, and that is its entire contribution. `markdown.go` contains a
`_ = grammarText` statement, and that is the only thing in the package that touches the
constant. The railroad diagram generated from it is empty — a bare track with no boxes on it.

It is kept because `embed-grammar.js` embeds it into both runtimes, and removing it would
mean changing the embedding step and the two files that carry the embedded block. That is
the only reason. It is stated here plainly rather than described as "intentionally trivial"
or "honest about where the complexity lives", because those phrasings suggest the grammar
is a deliberate minimal expression of something, and it is not — it is a leftover with a
build step attached.

Whether a Markdown grammar *could* usefully be expressed in the engine's declarative alts
is a separate and more interesting question. Block structure probably could, at considerable
length; inline structure almost certainly could not, since the delimiter and bracket stacks
are precisely the parts that a single-pass declarative rule machine cannot express. That is
why the specification describes an algorithm rather than a grammar, and why every conformant
implementation is an algorithm too.

## Differences from the TS version

Everything above describes a design the two runtimes share. What follows is where Go could
not spell it the same way. In every case the observable behaviour is identical — that is the
point — but the route to it differs, and each of these was a defect in the previous port
before it was a design note.

### Byte offsets, rune classification

A Go `string` is bytes; a JavaScript string is UTF-16 code units. The port scans by **byte**
offset, because every character the block and inline scanners branch on is ASCII, and no
UTF-8 continuation byte can hold an ASCII value. Slicing at a byte offset the scanner
reached therefore never splits a rune, and the scan is exact.

What is *not* safe is treating the byte at that offset as a character. The previous port did
— it appended `string(text[i])` when accumulating literal text — and `text[i]` is a `byte`,
so `string()` widened each UTF-8 continuation byte into its own code point. `café` came back
as `cafÃ©`. Every multi-byte character in the corpus was corrupted, silently, because the
output was still valid UTF-8 and still looked like text.

So the rule the port follows is: **scan by byte, classify by rune.** Wherever a whole
character is needed — emphasis flanking above all, which is decided on Unicode whitespace
and Unicode punctuation — the rune is decoded with `utf8.DecodeRuneInString` or
`utf8.DecodeLastRuneInString` first. `TestNonASCII` in `robust_test.go` pins the round trip.

The same distinction has a performance edge. A `sourcepos` column counts *characters*, as
the spec means it, but this port's offsets are bytes, so `block.go` has to convert on the
way out. Counting characters from the start of the line each time makes a line of *n* nested
containers — `> - > - > - …`, cheap to write and chosen by whoever supplies the input — cost
O(n²), where the canonical runtime is linear because its offsets are already index-like.
`TestNestedContainersAreNotQuadratic` pins the incremental anchor that fixes it. This is a
denial-of-service bound, not a micro-benchmark.

### Regexes are never built from input-derived counts

The canonical runtime builds a closing-fence pattern from the opening fence's length. The
first Go port did the same, interpolating the count into a `{n,}` bounded repeat — and RE2
caps bounded repeats at 1000. `regexp.MustCompile` on a longer one panics, and a panic in a
parsing library called from a request handler aborts the process. A code fence of 1001
backticks is a trivial thing to paste, so that was a remote kill switch.

The rule now is absolute: **no regexp in this package is constructed from a count derived
from the input.** Fence runs are counted with a loop. `robust_test.go` feeds 2000-character
fences of both kinds, plus a fence at a rune boundary, and requires that nothing panics.

This is a Go-specific hazard rather than a Go-specific mistake. JavaScript's backtracking
engine has no such cap, so the pattern is fine there and looks fine when ported; the failure
only appears at a size no unit test happens to use.

### RE2 has no lookahead

Two of the block-phase patterns in the canonical runtime use lookahead:
`` /^`{3,}(?!.*`)|^~{3,}/ `` for an opening code fence, and
`` /^(?:`{3,}|~{3,})(?=[ \t]*$)/ `` for a closing one. RE2 supports neither, and there is no
rewriting that recovers them, because both express a condition on the *rest of the line*
that the match itself must not consume.

Both are hand-coded scanners in `block.go` instead. They are short, and the interesting part
is the argument that a scanner is *equivalent* rather than merely similar. For the opening
fence: a backtick fence is only a fence when the info string that follows holds no backtick,
and backtracking to a shorter run cannot rescue a rejected one, because the shorter run's
own next character is then a backtick — so a single maximal-run count plus one
`strings.IndexByte` decides it exactly. For the closing fence: the maximal run must be
followed by spaces and tabs only, which is a loop.

A third hand-coded scanner exists for a different RE2 reason. The link-label pattern is
`/\[(?:[^\\[\]]|\\.){0,1000}\]/sy`, and RE2 expands a bounded repeat by copying the
sub-automaton — a thousand copies of an alternation, compiled at package init, for a
construct that a loop handles in a few lines.

### JavaScript whitespace is not Go whitespace

Four places in the algorithm turn on "whitespace" as JavaScript defines it, because the
canonical runtime reaches them through `String.prototype.trim()` or a `\s` class: the fenced
code block's info string, the trailing-space test that decides a hard line break, the
language/meta split of an info string, and §4.7 label matching.

Neither obvious Go tool is that set.

- `strings.TrimSpace` uses `unicode.IsSpace`, which **trims U+0085 NEXT LINE** — JavaScript
  keeps it — and **keeps U+FEFF** — JavaScript trims it.
- Go's `regexp` `\s` is ASCII-only: `[\t\n\f\r ]`. It omits the vertical tab and every
  Unicode space separator, so a pattern using it rejects input the canonical runtime
  accepts.

Neither disagreement is exotic enough to be theoretical: a zero-width no-break space at the
front of an info string is exactly the kind of thing that survives a copy-paste, and
`&#160;` in an info string puts a non-breaking space back *after* the block phase has
trimmed it.

So `common.go` defines the set once, as `isJSSpace` — the ECMAScript *WhiteSpace* and
*LineTerminator* productions, which is the spec's §2.3 Unicode whitespace plus the vertical
tab and U+FEFF — with `jsTrim` and `jsSpaceIndex` over it. `jsSpaceIndex` returns the width
of the match as well as its index, because JavaScript can step past the match with `+ 1`
where Go must advance a whole rune.

Note that this is deliberately a *different* set from `isUnicodeWhitespace`, which is §2.3
exactly and answers a different question: emphasis flanking. Two whitespace predicates in
one file looks like duplication and is not. Collapsing them would break the other one.

### Case folding a link label

§4.7 matches link labels case-insensitively, and the reference implementation approximates
Unicode case folding by lowercasing and then uppercasing. In JavaScript that round trip
uses the **full** case mappings — the unconditional entries of Unicode's `SpecialCasing.txt`
— so `"ß".toUpperCase()` is `"SS"`.

Go's `strings.ToUpper` and `strings.ToLower` apply the **simple** mappings: one rune in, one
rune out. `strings.ToUpper("ß")` is `"ß"`. So `[ẞ]` does not resolve against a definition of
`[SS]`, which is CommonMark example 540, and 103 further code points — mostly Greek Extended
iota-subscript forms and the Latin/Armenian ligatures — diverge the same way.

`common.go` therefore carries `jsFullCaseUpper`, a table keyed on the original rune and
holding the result of the whole round trip. Keying on the original rune is well defined
because in the root locale both halves are pure per-code-point mappings, and the one
context-sensitive rule, Final_Sigma, washes out: σ and ς both uppercase to Σ. ASCII takes a
fast path, since for ASCII the round trip is just uppercasing.

One gap is documented rather than fixed: Go's `unicode` tables are Unicode 15.0 where the
canonical runtime's are newer, so 55 code points added since then have casing pairs Go does
not know. That is a data-version difference, not a mapping-model one, and no spec example or
fixture reaches it.

### Smaller shape differences

| | TypeScript | Go |
|---|---|---|
| Options | a partial `MarkdownOptions` object | an `Options` struct; `ResolveOptions` converts the plugin's `map[string]any` |
| Nullable fields on the tree | `title: string \| null`, `info: string \| null` | `Title` + `HasTitle`, `Info` + `HasInfo` |
| Sticky regexes | `/…/y` matched at `pos` | `^`-anchored patterns matched against `subject[pos:]`; a Go string slice is a header, not a copy |
| Entity decoding | a vendored HTML5 table (`entities.ts`) | the standard library's table, gated so only semicolon-terminated references decode — `html.UnescapeString` also accepts legacy forms such as `&auml`, which §6.2 does not |
| Unicode punctuation | a regexp character class | `unicode.IsPunct \|\| unicode.IsSymbol` |
| Case-insensitive regexes | the `i` flag | both cases spelled out. Go folds `(?i)[A-Za-z]` over all of Unicode, so it would also match U+017F LONG S and U+212A KELVIN SIGN, and `<ſpan>` would be accepted as a tag |
| `grammarText` | exported | unexported |

## The parity contract, stated exactly

The two runtimes are held to one standard rather than to each other: both run the vendored
CommonMark 0.31.2 suite, and both score 652/652 across all 26 sections. That is the primary
guarantee, and it is a byte-for-byte HTML comparison against the specification's own
expected output.

Cross-runtime agreement is checked on top of that, and it is worth being precise about what
each check does and does not cover, because an earlier version of this document claimed
"everything else behaves the same across both runtimes, as enforced by `test/spec/`" — and
`test/spec/` has 36 fixtures. Thirty-six cases cannot enforce agreement over a parser.

What `test/spec/*.tsv` actually guarantees: 36 hand-written cases, each an input plus the
expected AST as JSON plus an optional options map, run through the **plugin** path
(`j.Parse`) by `go/parity_test.go` here and `ts/test/parity.test.ts` there, and auto-
discovered so adding a `.tsv` runs it in both. They are a regression net for the AST shapes
someone thought worth pinning, and they are the file you add to when you change behaviour
deliberately. They are not a proof of equivalence, they cover only the option combinations
their own rows name, and they say nothing about HTML.

What actually supports the claim that the runtimes agree is a wider comparison: the 652 spec
inputs run under all four `gfm` × `breaks` combinations in both runtimes — 2608 records —
comparing **both** the AST and the HTML on each. The result is 0 differing ASTs and 0
differing HTML outputs. That is a statement about 652 documents chosen to exercise every
corner of the specification, under every option this package has, on both of its outputs.

What none of it covers is `sourcepos`. The public AST does not carry source positions, so an
AST comparison is blind to them, and the HTML does not encode them either. A divergence in
the positional data is invisible to every automated check above, which is why
`TestSourcePosColumnsCountCharacters` exists as a direct unit test — both runtimes had the
column unit wrong at first, in opposite directions, and `😀 *x*` reported column 5 in one and
6 in the other. If you touch anything positional, compare the native trees by hand.
