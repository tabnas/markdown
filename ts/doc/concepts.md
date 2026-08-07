# Concepts: how @tabnas/markdown works (TypeScript)

Background and design rationale — why the parser is shaped the way it is, and what the
shape cost. For the API see the [reference](reference.md); for recipes see the
[how-to guide](guide.md).

Three things are worth settling before anything else, because they decide how you read the
rest of this document.

**The parser is conformant to CommonMark 0.31.2.** All 652 examples of the specification's
own test suite pass, in all 26 sections, in both the TypeScript and the Go runtime. The
suite is vendored at `test/commonmark/spec.json` and `npm run conformance` runs it, so the
claim is a measurement anyone can repeat rather than a description of intent. Most of what
follows is an account of what it cost to make that number true, which is why the number
comes first.

**The AST is the primary output.** A JSON tree of blocks and inlines is what this package
exists to produce, and what `parseDocument` returns. Nothing else runs when you ask for
it.

**There is an HTML emitter, and it is not a side utility.** The CommonMark specification
defines conformance as HTML output: the 652 examples in the suite are pairs of Markdown
and expected HTML, compared byte for byte. Without a renderer there is no way to make the
claim "652/652" mean anything. So the renderer exists as the instrument that measures the
parser. That it is also useful — you can call `toHtml` and get correct HTML — is a
consequence of building the measuring device honestly, not the reason it was built. More
on this below.

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

The block phase, in `block.ts`, keeps a *spine* of open blocks — the document, its last
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

Two positions are tracked along each line: a UTF-16 offset, used to slice the text that is
kept, and a tab-expanded display column, used for every indentation decision. They cannot
be collapsed into one. A tab may be *partially* consumed by a list marker — the marker
takes two of the tab's columns and the remaining two become content indent — and only the
column counter can express that.

The inline phase, in `inline.ts`, then runs over the raw text each paragraph and heading
accumulated. It is a single left-to-right scan that appends nodes as it goes. Code spans,
autolinks, raw HTML tags, entities, escapes and line breaks are all decided locally, at
the scan position, and consumed whole. Two constructs cannot be: emphasis and links.

## Emphasis needs a stack because you cannot decide an asterisk when you see it

An opening `*` is not knowable as an opening `*` at the moment it is read. Whether a
delimiter run opens emphasis, closes it, does both, or is just literal text depends on
what surrounds it, on what other runs are still unmatched behind it, and on how long those
runs are. So the scan does not decide. It emits each run as ordinary text and pushes a
record of it onto a delimiter stack; a separate pass resolves the stack afterwards,
walking forward to each potential closer and backward from there to the nearest usable
opener, and rewriting the sibling run in place when a pair is found.

The **flanking rules** are what let a run be classified at all. A run is left-flanking when
it is not followed by whitespace and either not followed by punctuation or else preceded by
whitespace or punctuation; right-flanking is the mirror image. This is how `a * b` stays
literal while `a *b* c` does not, and it is why the classification is done on the
specification's own definitions of Unicode whitespace and Unicode punctuation rather than
on `\s` and `\p{P}`. Those near-misses are exactly where conformance leaks: `\s` includes
U+FEFF and misses characters the spec counts, and version 0.31.2 widened the punctuation
class from P\* alone to P\* ∪ S\*, which is why `$`, `+`, `<`, `=`, `>`, `^`, `` ` ``, `|`,
`~` and currency symbols now count. Getting either set slightly wrong costs a handful of
emphasis examples and nothing else, which makes it very hard to notice.

Underscore is stricter than asterisk for a reason that has nothing to do with parsing
elegance: intraword `_` is common in identifiers. `foo_bar_baz` must stay literal, and the
flanking rules produce that outcome without a special case, because the `_` runs there are
both left- and right-flanking and so are disqualified from acting intraword.

The **rule of three** is the strangest-looking part of the algorithm and the one with the
clearest motive. When a delimiter run could both open and close, matching it greedily
produces results that disagree with what people write. The rule is: if either side of a
candidate pair can play both roles, the two run lengths may not sum to a multiple of
three, unless both lengths are themselves multiples of three. It exists to make sequences
like `*foo**bar**baz*` nest the way an author means them to — one emphasis wrapping a
strong — instead of splitting into fragments. It is a heuristic, openly. The specification
adopted it because the alternatives were worse, and any implementation that wants the
emphasis section to pass has to adopt it too.

## Links need a different stack, and a pre-pass

Brackets get their own stack, for a related but distinct reason. When `[` is read the
parser does not yet know whether it will turn out to be a link, an image, a reference, or
literal text — that is only settled when a `]` arrives and the parser looks at what
follows it. So `[` and `![` are pushed, and the arrival of `]` triggers the resolution:
try an inline destination, then a full reference, then a collapsed one, then a shortcut.

The two stacks interact, and the interaction is the source of Markdown's inline precedence.
Bracket matching happens *during* the scan; emphasis matching happens *afterwards*, over
the delimiter stack. That single fact is what makes brackets bind more tightly than
emphasis. Each bracket also records where the top of the delimiter stack was when it
opened, giving emphasis resolution inside a link label a floor it may not reach below — so
a `*` outside the label cannot pair with one inside it.

Reference links need something the scan cannot provide at all: a definition that may
appear later in the document. `[foo]` on line 1 can be defined on line 900. The parser
handles this by collecting definitions during the block phase, when each paragraph is
finalised. A paragraph may begin with link reference definitions; they are consumed from
its front into a reference map, and if nothing is left the paragraph disappears entirely.
By the time the inline phase runs, every definition in the document is already known, so
reference resolution is a map lookup rather than a second traversal. The map's keys are
normalised — trimmed, internal whitespace collapsed, case folded — because label matching
in Markdown is deliberately forgiving.

This is also why definitions are handled at paragraph finalisation rather than by a
dedicated line scan: a definition is only a definition when it sits at the start of what
would otherwise be a paragraph, and you do not know where a paragraph starts until the
block phase has decided it.

## Two trees: a native one, and a projection

Internally the parse produces a native CommonMark node tree. The public AST is a
projection of that tree, built by `ast.ts`. Having two representations is a real cost —
one more thing to keep in step — so it is worth being clear about what each is for.

The native tree is **linked**: parent, first child, last child, previous and next
siblings, with no children array anywhere. This is not stylistic. Both phases splice nodes
mid-walk. The block phase opens and closes blocks from the tail of a spine while it is
walking that spine; the inline phase's delimiter resolution removes matched delimiters and
wraps the sibling run between them in a new node, in place, while iterating over the same
run. Every one of those operations is a constant-time pointer rewrite on a linked tree, and
an index shuffle on an array of children. The renderer's walker exists for the same reason:
a depth-first traversal that reports entering and exiting a container separately maps
directly onto opening and closing tags, and it can be repositioned mid-walk — which is how
an image skips over its own children after flattening them into an `alt` attribute.

The public AST is the opposite: plain JSON, arrays of children, no cycles, no parent
pointers. It is what callers actually want to hand to `JSON.stringify`, walk with
`children.map`, or compare in a test fixture. It has been this package's output since
before the rewrite, and the rewrite kept it that way — the shared `.tsv` fixtures pass
untouched. Two things did change: raw inline tags now get their own `html` node instead of
leaking into `text`, and `spread` acquired a meaning — see below.

The projection is deliberately lossy, in four places.

*Soft breaks collapse.* With `breaks: false`, a soft line break becomes a single space
inside the surrounding text run instead of a node of its own. This is long-standing
documented behaviour of this package and it is what most consumers of the AST want: a
paragraph wrapped at 80 columns should not read as a sequence of fragments. The native
tree keeps real soft-break nodes, so the HTML render is unaffected, and `breaks: true`
promotes them to `break` nodes for callers who need the distinction.

*Source positions are dropped.* The AST carries no line or column information. The native
tree keeps `sourcepos` on block nodes, which is why `parseTree` exists: if you need to map
a node back to the source — for an editor, a linter, an error message — that is the tree to
ask.

*The block/inline distinction for raw HTML narrows.* The native tree distinguishes an HTML
block from an inline tag; the AST calls both `html`. Their context makes them
unambiguous — one appears among blocks, the other among inlines — so the distinction is
recoverable, and one node type is simpler.

*A table's header-row flag goes.* The native tree marks the header row, because the
renderer needs to pick `<th>` over `<td>` mid-walk. The AST relies on mdast's convention
instead: the first row is the header row. See below.

One thing the projection now does *better* is `spread`. It follows mdast semantics: a list
is spread when it is loose, an item when it holds blank-line-separated blocks. The previous
implementation latched the field off whether the file ended in a newline, so `- a\n- b\n`
and `- a\n\n- b\n` produced identical output and the field carried no information at all.
Nothing could have depended on it, which is what made the change safe to make.

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

The renderer then reads the decision back off the list rather than off the paragraph.
A paragraph checks its *grandparent*: a paragraph whose grandparent is a tight list is
necessarily some item's direct child, and drops its tags. This is why the renderer's
newline discipline matters so much. Every block asks only to "be at the start of a line",
never emitting a newline on a neighbour's or a child's behalf, so `<li>` deliberately does
not end a line and a tight item comes out as `<li>a</li>` with no interior newlines at all,
while a loose one gets its newline from its first child's own leading request. Neither case
needs to know about the other.

## Why the parser is engine-free

`@tabnas/markdown` is a plugin for the Tabnas engine, but the parser inside it does not
depend on the engine. Nothing reachable from `commonmark.ts` imports `@tabnas/parser`; the
plugin wiring in `markdown.ts` is the only file that mentions it, and even there only for
types. The dependency points one way and never back.

This buys three things.

The conformance suite runs without the engine, without a build step, and without
`node_modules`. It stages the sources in a temporary directory and executes them under
Node's type stripping. That means the 652/652 figure can be checked in an environment
where the engine is not installed or not yet built — including CI on a fresh clone, and
including the sibling-development setup where the engine is a local `file:` dependency
that may be mid-change.

It keeps the failure surface honest. When a conformance example fails you know it is the
parser, because there is nothing else in the process. A parser entangled with a lexer would
leave you bisecting between the two.

And it makes the Go port a port of the parser rather than of the plugin. TypeScript is
canonical and Go follows it (see `AGENTS.md`); because the parser is a self-contained set
of modules with no engine surface, porting it is a mechanical translation of algorithms,
and parity can be checked directly — 652 examples × 4 option combinations, comparing both
ASTs and both HTML outputs.

The cost is that the plugin cannot use the engine's lexer for anything. It reads the raw
source through `ctx.src()` and consumes the token stream only to satisfy the engine's
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
100%, since without it neither number exists. Newline placement in that renderer is a
correctness contract, not a formatting preference, because the comparison is byte for
byte.

Having built it, there is no reason to hide it — `toHtml` is a supported part of the API
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

The package parses CommonMark, with the complete set of five GFM extensions: tables, task
list items, autolink literals, strikethrough and disallowed raw HTML. That is 24/24 on the
GFM specification's extension corpus, and the set is closed — there is no sixth extension
in the specification waiting to be written. `gfm` gates the five as a single switch rather
than as five flags: a document is either GitHub-flavoured or it is not, and a
per-extension matrix is configuration surface nobody asked for.

Four of the five are deliberately *not* in the inline scanner. Tables are decided in the
block phase, so a cell's content reaches the inline scanner exactly as a paragraph's would
and nothing about emphasis or code spans changes. Task list markers are consumed in the
block phase too, over a paragraph's raw text, before the inline scanner sees brackets at
all; autolink literals are a post-pass over the finished inline tree; the raw-HTML filter
is a rendering step. Only strikethrough is a genuine addition to the scanner, and it is
one delimiter character the scanner previously ignored.

That keeps the machinery that decides code spans, raw HTML, emphasis and links exactly as
CommonMark specifies it, and the consequence is that `gfm: false` is not an approximation
of CommonMark but the same parse: byte-identical output over 1430 checked records, not a
resemblance. Turning the extensions on does move nine of the 652 spec examples — six in
HTML blocks, where the disallowed-raw-HTML filter escapes the `<script>`, `<style>` and
`<textarea>` the suite expects verbatim, and three in Autolinks, where text the suite
expects to stay literal becomes a link. Those nine are the extensions doing precisely what
they are specified to do. It is also why the conformance figure is measured where the
specification's own options are, with `gfm: false`, rather than being quoted from a run
that has extensions layered over it.

What `gfm` does *not* mean is "everything a Markdown file might contain". Footnotes are the
closest miss: they are a GitHub product feature, not part of the GFM specification suite,
and there is nothing to conform to. The failure mode is quiet rather than loud, because
`[^1]` is a perfectly good CommonMark link label — a GitHub-authored footnote renders as a
broken link rather than raising anything. If the definition body happens to look like a
destination it is a link reference definition, and the footnote marker silently becomes a
real link to it.

The other collision is inherited from GFM itself. Strikethrough accepts a single `~`, and
several other dialects spell subscript with single tildes, so `H~2~O` renders as
`H<del>2</del>O` under `gfm: true`. There is no way to have GFM strikethrough and not have
that; the only escape is `gfm: false` or escaping the tildes in the source.

Everything further out — math, front matter, definition lists, heading attributes,
admonitions, wiki links, emoji shortcodes, highlight, sub/superscript — is absent on
purpose. Each is a different dialect's idea, and each would need its own opt-in flag if it
were ever added. Letting `gfm` grow to mean "everything" would turn a flag whose meaning
is defined by an external specification into a flag whose meaning is whatever this package
happened to implement last, and would take the conformance argument above with it. The
honest statement of scope is still narrower than "CommonMark/GFM": a caller who needs
footnotes knows to look elsewhere rather than discovering it at runtime.

## Tables are the extension that reaches backwards

Every other extension looks forward. The scanner reads a `~`, or the block phase reads a
`[x]` at the head of an item it has already opened, or the renderer reads a tag name it is
about to write. Tables are the one construct in CommonMark or GFM whose trigger appears
*after* the text it governs, and that single fact accounts for nearly everything odd about
how they are implemented.

A delimiter row — `| --- | :-: |` — is meaningless on its own. It only means "table" if
the line above it is a header row with the same number of cells, and by the time the block
phase reads it, that line is no longer a line: it has been appended to an open paragraph's
accumulated text and the paragraph is the tip of the spine. So the table start has to
reach back into the paragraph, take its last line away, and decide from the two lines
together. If the paragraph had earlier lines, they are not part of the table: the
paragraph is truncated to those lines and finalised, and the table opens as a sibling
after it. Finalising it rather than discarding it matters, because a paragraph's front may
hold link reference definitions, and a definition that has already been written into the
document should not vanish because the last line of the same paragraph turned out to be a
table header.

CommonMark has one construct that reaches back at all — the Setext underline, which
consumes the paragraph above it and turns it into a heading — and it is instructive that
the table start is worse. Setext takes the paragraph *whole*: the block is replaced, and
nothing is left over. A table start takes only the last line and leaves the rest standing
as a paragraph, which means it is the one block start in the parser that has to edit the
accumulated text of a block it is not replacing. Everything else the block phase does is
additive: match continuations, open new blocks, close unmatched ones. That is why the
whole of the reaching-backwards logic is confined to one function.

That exception is also why tables are the one extension that could plausibly have cost
core conformance. The other four are bolted on where nothing else is looking — a
character the scanner ignored, a post-pass, a render step — but a table start is
evaluated against an open paragraph on lines that CommonMark has its own opinions about.
Spec examples full of pipes, Setext headings whose underline is a run of hyphens, list
items beginning with `-`: all of them present lines that a careless delimiter-row test
would claim. The mitigations are ordering and strictness. The table start is tried last, on
a line every built-in start has already refused: after the Setext underline, so `foo` over
`---` is still an `<h2>` and not a one-column table; after the thematic break; after the
list marker, so `- | -` is still a list item. Then it is strict about what it accepts. A
delimiter cell must be hyphens with at most one leading and one trailing colon and nothing
else, so a single stray character puts the line back to being ordinary text, and the two
rows must agree on their cell count exactly, which is what stops a paragraph with an
incidental pipe in it from dragging the next line into a table. The 652 examples are the
check on all of that: they are scored with `gfm: false`, and run again under every
`gfm` × `breaks` combination as a standing assertion that no example throws.

Escaped pipes are the second place where tables have to break the usual order of
operations, and this one is about phases rather than blocks. A cell is delimited by unescaped
pipes, so splitting a row means understanding `\|` — that much is unavoidable. What is
less obvious is that `\|` must be *resolved* to a literal `|` at split time, before the
inline phase ever sees the cell, rather than being left for the ordinary escape handling
that resolves `\*` and `\[`. The reason is code spans. Inline escape resolution does not
happen inside a code span; that is what a code span is for. So a cell written
`` b `\|` az `` would keep its backslash forever if the resolution waited for the inline
phase, and would render as `b <code>\|</code> az` instead of `b <code>|</code> az`. No
amount of unescaping *after* inline parsing can fix it either, because by then the
backslash is inside a literal that is defined to be untouched. Resolving at split time is
the only point in the pipeline where the information is still editable, which is why the
splitter rewrites exactly this one escape and leaves every other backslash alone for the
inline phase to handle in the usual way.

Tables are also the only extension that added node types rather than a field. Task lists
became `listItem.checked`, strikethrough reused the `delete` node the AST already had,
autolink literals produce ordinary `link` nodes, and the raw-HTML filter changes no tree
at all. A table cannot be expressed that way. It is not a variation on a block that
already exists — it is a two-dimensional structure with per-column data attached to the
container rather than to the cells, and rows and cells that hold content in their own
right. Any attempt to encode it in existing types (a list of lists, a paragraph with a
marker field) would make consumers reconstruct the shape from a convention, which is worse
than three honest node types. mdast had already settled the question, and matching it
means `table`, `tableRow` and `tableCell` behave for downstream tooling exactly as they do
everywhere else in that ecosystem.

Matching mdast also imports one of its conventions: there is no header flag. The first row
is the header row, and that is all. The native tree does carry a flag, because the
renderer has to choose `<th>` over `<td>` while walking a row and cannot afford to walk
back up to the table to ask. The projection drops it, on the grounds that a convention two
ecosystems already share is cheaper than a field that can disagree with the ordering. The
same reasoning explains the padding: rather than let a row be short and make every
consumer handle a ragged table, the block phase pads short rows and truncates long ones so
that a row's cell count always equals the column count. `align[i]` is then the alignment of
cell `i` of every row, with no bounds checking anywhere.

## The grammar file is inert

`markdown-grammar.jsonic` at the repository root, and the `grammarText` constant embedded
from it into both runtimes, do nothing.

They are not a simplified grammar, or a grammar that handles the easy cases with JavaScript
handling the rest. Block structure is decided entirely by the line algorithm in `block.ts`.
The file declares one rule with alts that consume the token stream so the engine's
trailing-content check passes, and that is its entire contribution. The railroad diagram
generated from it is empty — a bare track with no boxes on it.

It is kept because `embed-grammar.js` embeds it into both runtimes, and removing it would
mean changing the embedding step and the two files that carry the embedded block. That is
the only reason. It is stated here plainly rather than described as "intentionally trivial"
or "honest about where the complexity lives", because those phrasings suggest the grammar
is a deliberate minimal expression of something, and it is not — it is a leftover with a
build step attached.

Whether a Markdown grammar *could* usefully be expressed in the engine's declarative alts
is a separate and more interesting question. Block structure probably could, at
considerable length; inline structure almost certainly could not, since the delimiter and
bracket stacks are precisely the parts that a single-pass declarative rule machine cannot
express. That is why the specification describes an algorithm rather than a grammar, and
why every conformant implementation is an algorithm too.
