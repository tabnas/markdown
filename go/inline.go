/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Phase 2 of the CommonMark parse (spec 0.31.2, Appendix A, "Phase 2: inline
// structure"). Port of ts/src/inline.ts — keep the two in step. The block
// phase left the raw text of every paragraph and heading in StringContent;
// this file turns that text into inline children and clears it.
//
// The shape is the spec's own algorithm: one left-to-right scan over the
// subject that appends nodes as it goes, plus two auxiliary stacks that are
// resolved out of band, because neither construct can be decided at the
// point where its opener is seen:
//
//   - the delimiter stack (§6.4) holds `*`/`_` (and, under GFM, `~`) runs,
//     and is resolved by processEmphasis;
//   - the bracket stack (§6.3) holds `[` and `![`, and is resolved by
//     parseCloseBracket when a `]` turns up.
//
// Everything else — code spans, autolinks, raw HTML, entities, escapes,
// breaks — is decided locally at the scan position. That is what produces
// the precedence §6.3 requires: a code span, autolink or raw HTML tag is
// consumed whole during the scan, so it binds more tightly than emphasis and
// more tightly than the brackets of a link label. Conversely brackets bind
// more tightly than emphasis, because bracket matching happens during the
// scan while emphasis matching happens afterwards over the delimiter stack.
//
// --- What differs from the TypeScript, and why ------------------------------
//
//   - The subject is scanned by BYTE offset. Every character the scan
//     branches on is ASCII, and no UTF-8 continuation byte can collide with
//     an ASCII value, so byte scanning is exact. Wherever a whole *character*
//     is needed — emphasis flanking above all — the rune is decoded with
//     utf8.DecodeRuneInString / utf8.DecodeLastRuneInString before being
//     handed to isUnicodeWhitespace / isUnicodePunctuation.
//   - TS's sticky (`y`) regexes become `^`-anchored patterns matched against
//     subject[pos:]. Go has no sticky flag; a string slice is a header rather
//     than a copy, so this costs nothing.
//   - The C_* character-code constants are written as Go byte literals
//     instead: block.go declares the same spec constants, and Go has one
//     package scope where TypeScript has one scope per module. For the same
//     reason `text()` is textNode() and the raw-HTML grammar fragments carry
//     an `inline` prefix.
//   - parseLinkLabel and the small whitespace regexes are hand-coded loops.
//     The label pattern's {0,1000} repeat of an alternation expands to a
//     thousand NFA copies under RE2, and the rest are cheaper as loops.
//   - Titles are `string` + a `has` bool rather than `string | null`, matching
//     the MdNode/RefDef shape node.go and options.go already fixed.

import (
	"regexp"
	"strings"
	"unicode/utf8"
)

// --- §6.6 Raw HTML: the tag grammar, spelled out because it is stricter than
// `<[^>]*>` in every part. Attribute names are XML-restricted-to-ASCII;
// unquoted attribute values exclude quotes, `=`, `<`, `>`, backtick and all
// control characters; comments/PIs/declarations/CDATA each have their own
// terminator. inlineSpace stands in for "spaces, tabs, and up to one line
// ending": a paragraph can never contain a blank line, so a single run can
// only ever span one line ending anyway.

// inlineSpace is JavaScript's `\s`, spelled out. Go's `\s` is only
// [\t\n\f\r ] — it omits the vertical tab and every Unicode space separator —
// so using it here would reject tags the TypeScript accepts.
const inlineSpace = "[ \\t\\n\\v\\f\\r" +
	"\\x{00a0}\\x{1680}\\x{2000}-\\x{200a}\\x{2028}\\x{2029}\\x{202f}\\x{205f}\\x{feff}\\x{3000}]"

const (
	inlineTagName           = `[A-Za-z][A-Za-z0-9-]*`
	inlineAttributeName     = `[a-zA-Z_:][a-zA-Z0-9:._-]*`
	inlineUnquotedValue     = "[^\"'=<>`\\x00-\\x20]+"
	inlineSingleQuotedValue = `'[^']*'`
	inlineDoubleQuotedValue = `"[^"]*"`

	inlineAttributeValue = "(?:" + inlineUnquotedValue +
		"|" + inlineSingleQuotedValue + "|" + inlineDoubleQuotedValue + ")"
	inlineAttributeValueSpec = "(?:" + inlineSpace + "*=" + inlineSpace + "*" + inlineAttributeValue + ")"
	inlineAttribute          = "(?:" + inlineSpace + "+" + inlineAttributeName + inlineAttributeValueSpec + "?)"

	inlineOpenTag  = "<" + inlineTagName + inlineAttribute + "*" + inlineSpace + "*/?>"
	inlineCloseTag = "</" + inlineTagName + inlineSpace + "*>"

	// §6.6: `<!-->` and `<!--->` are comments in their own right; otherwise
	// the body simply may not contain `-->`, which the lazy quantifier
	// enforces.
	inlineHTMLComment           = `<!-->|<!--->|<!--[\s\S]*?-->`
	inlineProcessingInstruction = `[<][?][\s\S]*?[?][>]`
	inlineDeclaration           = `<![A-Za-z][^>]*>`
	// TS compiles the whole tag pattern with the `i` flag, which here only
	// ever matters for the literal `CDATA` — every other letter in the
	// grammar is already spelled in both cases. Written out rather than left
	// to `(?i)`, because Go folds case over all of Unicode: `(?i)[A-Za-z]`
	// also matches U+017F LATIN SMALL LETTER LONG S and U+212A KELVIN SIGN,
	// where JavaScript's non-unicode `i` deliberately does not, and that would
	// make Go accept `<ſpan>` as a tag when TS does not.
	inlineCDATA = `<!\[[cC][dD][aA][tT][aA]\[[\s\S]*?\]\]>`

	inlineHTMLTag = "(?:" + inlineOpenTag +
		"|" + inlineCloseTag +
		"|" + inlineHTMLComment +
		"|" + inlineProcessingInstruction +
		"|" + inlineDeclaration +
		"|" + inlineCDATA + ")"
)

// inlineJSDot is JavaScript's `.` without the `s` flag. Go's `.` excludes only
// `\n`; JavaScript's also excludes `\r`, U+2028 and U+2029.
const inlineJSDot = `[^\n\r\x{2028}\x{2029}]`

var (
	// The §2.1 ASCII punctuation set as a character class, built from the same
	// string common.go's IsEscapable tests against so the two cannot drift.
	inlineEscapableClass = "[" + regexp.QuoteMeta(escapableASCII) + "]"
	inlineEscapedChar    = `\\` + inlineEscapableClass

	reHtmlTag = regexp.MustCompile(`^` + inlineHTMLTag)
	// TS applies the `i` flag here too; common.go's entityPattern already
	// spells both cases out, so no flag is needed — and leaving it off keeps
	// Go's Unicode case folding from widening `[a-zA-Z]` (see inlineCDATA).
	reEntityHere = regexp.MustCompile(`^(?:` + entityPattern + `)`)

	// §6.3 link destination, pointy-bracket form: no line endings, no
	// unescaped `<` or `>`.
	reLinkDestinationBraces = regexp.MustCompile(`^<(?:[^<>\n\\\x00]|\\` + inlineJSDot + `)*>`)

	// §6.3 link title in `"`, `'` or `(...)` delimiters. The escape
	// alternative comes first so that `\"` inside a double-quoted title is
	// consumed as an escape rather than closing the title.
	reLinkTitle = regexp.MustCompile(
		`^(?:"(?:` + inlineEscapedChar + `|[^"\x00])*"` +
			`|'(?:` + inlineEscapedChar + `|[^'\x00])*'` +
			`|\((?:` + inlineEscapedChar + `|[^()\x00])*\))`)

	// §6.5 autolinks. Scheme: an ASCII letter then 1–31 more of letter/digit/
	// `+`/`.`/`-`, i.e. 2–32 characters in total, case-insensitive.
	reAutolink      = regexp.MustCompile(`^<[A-Za-z][A-Za-z0-9.+-]{1,31}:[^<>\x00-\x20]*>`)
	reEmailAutolink = regexp.MustCompile("^<([a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+" +
		`@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?` +
		`(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)>`)
)

// maxLinkParenNesting bounds `(` nesting inside a bare link destination. The
// spec describes balanced parentheses without stating a bound; cmark uses 32,
// and the bound is what keeps an unbalanced run from being quadratic (see
// parseLinkDestination).
const maxLinkParenNesting = 32

func textNode(literal string) *MdNode {
	node := NewNode(NodeText)
	node.Literal = literal
	return node
}

// isInlineSpecialByte reports whether a byte can start an inline construct,
// i.e. whether it terminates a run of ordinary text. TS spells this as the
// negated classes of reMain / reMainGfm; `~` is only special under GFM.
func isInlineSpecialByte(c byte, gfm bool) bool {
	switch c {
	case '\n', '`', '[', ']', '\\', '!', '<', '&', '*', '_':
		return true
	case '~':
		return gfm
	}
	return false
}

// isInlineWhitespaceByte is TS's reWhitespaceChar, /[ \t\n\v\f\r]/.
func isInlineWhitespaceByte(c byte) bool {
	switch c {
	case ' ', '\t', '\n', '\v', '\f', '\r':
		return true
	}
	return false
}

// The trim in parse() decides whether trailing spaces make a hard line break,
// so it must remove exactly what String.prototype.trim removes — not
// strings.TrimSpace, which trims U+0085 (JavaScript keeps it) and keeps U+FEFF
// (JavaScript trims it). That set is jsTrim, in common.go.

// trimDelimTail drops n bytes from the end of a delimiter run's literal. The
// literal is a run of ASCII `*`, `_` or `~`, so bytes and characters agree.
func trimDelimTail(s string, n int) string {
	if n >= len(s) {
		return ""
	}
	return s[:len(s)-n]
}

// delimiter is one `*`, `_` or `~` run on the delimiter stack (§6.4).
type delimiter struct {
	// cc is the delimiter character.
	cc byte
	// numdelims counts delimiters still unused; it drops as pairs are consumed.
	numdelims int
	// origdelims is the run length as originally scanned — what the rule of
	// three is computed on.
	origdelims int
	node       *MdNode
	previous   *delimiter
	next       *delimiter
	canOpen    bool
	canClose   bool
}

// bracket is one `[` or `![` on the bracket stack (§6.3).
type bracket struct {
	node     *MdNode
	previous *bracket
	// previousDelimiter is the top of the delimiter stack when this bracket
	// opened — the emphasis floor.
	previousDelimiter *delimiter
	// index is the offset of the `[` in the subject.
	index int
	image bool
	// active is cleared on earlier openers once a link is built: no links in
	// links.
	active bool
	// bracketAfter records that another bracket opened after this one, so its
	// text contains a `[`.
	bracketAfter bool
}

type delimScan struct {
	numdelims int
	canOpen   bool
	canClose  bool
}

// openersKey is the openers_bottom map key: TS builds the string
// `cc:canOpen:origdelims%3`; a struct key is the Go equivalent.
type openersKey struct {
	cc      byte
	canOpen bool
	mod3    int
}

type inlineParser struct {
	subject    string
	pos        int
	delimiters *delimiter
	brackets   *bracket
	refmap     RefMap
	options    Options
}

// --- scanning primitives ---

// peek returns the byte at pos, or -1 at the end of the subject. Every caller
// compares it against an ASCII literal, which a UTF-8 continuation byte can
// never equal.
func (p *inlineParser) peek() int {
	if p.pos < len(p.subject) {
		return int(p.subject[p.pos])
	}
	return -1
}

// match applies an `^`-anchored regex at pos; on success it advances past the
// match. This is the Go spelling of TS's sticky-regex helper.
func (p *inlineParser) match(re *regexp.Regexp) (string, bool) {
	loc := re.FindStringIndex(p.subject[p.pos:])
	if loc == nil {
		return "", false
	}
	m := p.subject[p.pos+loc[0] : p.pos+loc[1]]
	p.pos += loc[1]
	return m, true
}

// spnl consumes spaces, and at most one line ending, between the parts of a
// link. TS: / *(?:\n *)?/y.
func (p *inlineParser) spnl() {
	p.skipSpaces()
	if p.pos < len(p.subject) && p.subject[p.pos] == '\n' {
		p.pos++
		p.skipSpaces()
	}
}

// skipSpaces is TS's reInitialSpace, / */y.
func (p *inlineParser) skipSpaces() {
	for p.pos < len(p.subject) && p.subject[p.pos] == ' ' {
		p.pos++
	}
}

// skipIndent is TS's reIndent, / {0,3}/y.
func (p *inlineParser) skipIndent() {
	for n := 0; n < 3 && p.pos < len(p.subject) && p.subject[p.pos] == ' '; n++ {
		p.pos++
	}
}

// spaceAtEndOfLine is TS's reSpaceAtEndOfLine, / *(?:\n|$)/y. On failure pos
// is left alone, as a failed sticky match would leave it. The greedy run can
// never usefully backtrack — a space is neither `\n` nor end of subject — so
// a single forward scan is the whole language.
func (p *inlineParser) spaceAtEndOfLine() bool {
	i := p.pos
	for i < len(p.subject) && p.subject[i] == ' ' {
		i++
	}
	if i < len(p.subject) {
		if p.subject[i] != '\n' {
			return false
		}
		i++
	}
	p.pos = i
	return true
}

// charBefore returns the whole code point ending at end, so an astral
// character is classified as one character by the flanking rules rather than
// as a lone surrogate (TS) or a stray byte (Go). Start of subject counts as a
// line ending (§6.4).
func (p *inlineParser) charBefore(end int) rune {
	if end <= 0 {
		return '\n'
	}
	r, _ := utf8.DecodeLastRuneInString(p.subject[:end])
	return r
}

// charAtPos returns the whole code point at i; end of subject counts as a
// line ending.
func (p *inlineParser) charAtPos(i int) rune {
	if i >= len(p.subject) {
		return '\n'
	}
	r, _ := utf8.DecodeRuneInString(p.subject[i:])
	return r
}

// --- §6.3 code spans ---

// parseBackticks consumes a code span: a backtick string to the next backtick
// string of *exactly* the same length. Unmatched opening backticks are
// literal text.
//
// Hand-coded rather than TS's /`+/y plus a global /`+/g scan, so that no
// regex is built from an input-derived run length.
func (p *inlineParser) parseBackticks(block *MdNode) bool {
	s := p.subject
	startpos := p.pos
	for p.pos < len(s) && s[p.pos] == '`' {
		p.pos++
	}
	if p.pos == startpos {
		return false
	}
	tickLen := p.pos - startpos

	afterOpenTicks := p.pos

	for i := afterOpenTicks; i < len(s); {
		if s[i] != '`' {
			i++
			continue
		}
		runStart := i
		for i < len(s) && s[i] == '`' {
			i++
		}
		p.pos = i
		if i-runStart == tickLen {
			node := NewNode(NodeCode)
			// Line endings become spaces; then one leading and one trailing
			// space are stripped together, but only if the content is not all
			// spaces (which is what lets a code span hold backticks).
			contents := strings.ReplaceAll(s[afterOpenTicks:p.pos-tickLen], "\n", " ")
			hasNonSpace := false
			for j := 0; j < len(contents); j++ {
				if contents[j] != ' ' {
					hasNonSpace = true
					break
				}
			}
			if len(contents) > 2 && contents[0] == ' ' && contents[len(contents)-1] == ' ' && hasNonSpace {
				node.Literal = contents[1 : len(contents)-1]
			} else {
				node.Literal = contents
			}
			block.AppendChild(node)
			return true
		}
	}

	// No closing run of the same length: the opener is literal text.
	p.pos = afterOpenTicks
	block.AppendChild(textNode(s[startpos:afterOpenTicks]))
	return true
}

// --- §6.1 backslash escapes, §6.7 hard line breaks ---

func (p *inlineParser) parseBackslash(block *MdNode) bool {
	p.pos++
	if p.peek() == '\n' {
		p.pos++
		block.AppendChild(NewNode(NodeLinebreak))
		p.skipSpaces()
	} else if p.pos < len(p.subject) && IsEscapable(p.subject[p.pos]) {
		block.AppendChild(textNode(p.subject[p.pos : p.pos+1]))
		p.pos++
	} else {
		block.AppendChild(textNode(`\`))
	}
	return true
}

// --- §6.5 autolinks ---

func (p *inlineParser) parseAutolink(block *MdNode) bool {
	if m, ok := p.match(reEmailAutolink); ok {
		dest := m[1 : len(m)-1]
		block.AppendChild(p.makeAutolink("mailto:"+dest, dest))
		return true
	}
	if m, ok := p.match(reAutolink); ok {
		dest := m[1 : len(m)-1]
		block.AppendChild(p.makeAutolink(dest, dest))
		return true
	}
	return false
}

func (p *inlineParser) makeAutolink(destination string, label string) *MdNode {
	// Autolink text is literal: no backslash escapes, no entity references.
	node := NewNode(NodeLink)
	// Not percent-encoded here — see parseLinkDestination.
	node.Destination = destination
	node.Title = ""
	node.HasTitle = false
	node.AppendChild(textNode(label))
	return node
}

// --- §6.6 raw HTML ---

func (p *inlineParser) parseHtmlTag(block *MdNode) bool {
	m, ok := p.match(reHtmlTag)
	if !ok {
		return false
	}
	node := NewNode(NodeHTMLInline)
	node.Literal = m
	block.AppendChild(node)
	return true
}

// --- §6.4 emphasis: delimiter runs and flanking ---

// scanDelims classifies the run of cc starting at pos without consuming it.
//
// §6.4: a run is left-flanking iff it is not followed by Unicode whitespace
// and either not followed by Unicode punctuation or else preceded by
// whitespace or punctuation; right-flanking is the mirror image. `_`
// additionally may not open inside a word (rules 2, 4, 6, 8), which is what
// keeps snake_case_words intact; `*` has no such restriction.
//
// The two neighbouring characters are decoded as runes, not read as bytes:
// classify `é` or `。` by its first UTF-8 byte and most of §6.4 quietly
// breaks on non-ASCII input.
func (p *inlineParser) scanDelims(cc byte) *delimScan {
	startpos := p.pos
	numdelims := 0

	for p.pos < len(p.subject) && p.subject[p.pos] == cc {
		numdelims++
		p.pos++
	}
	if numdelims == 0 {
		return nil
	}

	charBefore := p.charBefore(startpos)
	charAfter := p.charAtPos(p.pos)

	afterIsWhitespace := isUnicodeWhitespace(charAfter)
	// §2.1 punctuation is P* ∪ S*, which isUnicodePunctuation already covers.
	// `*$*alpha.` is the case that needs the symbol half: with `$` counted as
	// punctuation the closing `*` is not right-flanking, so the line stays
	// literal text.
	afterIsPunctuation := isUnicodePunctuation(charAfter)
	beforeIsWhitespace := isUnicodeWhitespace(charBefore)
	beforeIsPunctuation := isUnicodePunctuation(charBefore)

	leftFlanking := !afterIsWhitespace &&
		(!afterIsPunctuation || beforeIsWhitespace || beforeIsPunctuation)
	rightFlanking := !beforeIsWhitespace &&
		(!beforeIsPunctuation || afterIsWhitespace || afterIsPunctuation)

	var canOpen, canClose bool
	if cc == '_' {
		canOpen = leftFlanking && (!rightFlanking || beforeIsPunctuation)
		canClose = rightFlanking && (!leftFlanking || afterIsPunctuation)
	} else {
		canOpen = leftFlanking
		canClose = rightFlanking
	}

	p.pos = startpos
	return &delimScan{numdelims: numdelims, canOpen: canOpen, canClose: canClose}
}

// handleDelim emits a delimiter run as text and pushes it on the delimiter
// stack.
func (p *inlineParser) handleDelim(cc byte, block *MdNode) bool {
	res := p.scanDelims(cc)
	if res == nil {
		return false
	}

	numdelims := res.numdelims
	startpos := p.pos
	p.pos += numdelims

	node := textNode(p.subject[startpos:p.pos])
	block.AppendChild(node)

	// GFM strikethrough only recognises runs of one or two tildes; longer
	// runs stay literal.
	stackable := true
	if cc == '~' {
		stackable = numdelims <= 2
	}

	if (res.canOpen || res.canClose) && stackable {
		delim := &delimiter{
			cc:         cc,
			numdelims:  numdelims,
			origdelims: numdelims,
			node:       node,
			previous:   p.delimiters,
			next:       nil,
			canOpen:    res.canOpen,
			canClose:   res.canClose,
		}
		if delim.previous != nil {
			delim.previous.next = delim
		}
		p.delimiters = delim
	}

	return true
}

func (p *inlineParser) removeDelimiter(delim *delimiter) {
	if delim.previous != nil {
		delim.previous.next = delim.next
	}
	if delim.next == nil {
		// Top of stack.
		p.delimiters = delim.previous
	} else {
		delim.next.previous = delim.previous
	}
}

// processEmphasis is the spec's "process emphasis" procedure. stackBottom is
// the floor: for the whole-block call it is nil, and for the inlines of a link
// it is the delimiter that was on top when the link's `[` opened.
func (p *inlineParser) processEmphasis(stackBottom *delimiter) {
	// openers_bottom, keyed by (delimiter char, whether the closer can also
	// open, closer run length mod 3). Once a closer of some class fails to
	// find an opener, no later closer of the same class needs to look past
	// that point either — this is what keeps the search linear.
	openersBottom := map[openersKey]*delimiter{}

	// First closer above stackBottom.
	closer := p.delimiters
	for closer != nil && closer.previous != stackBottom {
		closer = closer.previous
	}

	for closer != nil {
		if !closer.canClose {
			closer = closer.next
			continue
		}

		closercc := closer.cc
		key := openersKey{cc: closercc, canOpen: closer.canOpen, mod3: closer.origdelims % 3}
		bottom := stackBottom
		if v, ok := openersBottom[key]; ok {
			bottom = v
		}

		opener := closer.previous
		openerFound := false
		for opener != nil && opener != stackBottom && opener != bottom {
			if opener.cc == closercc && opener.canOpen {
				if closercc == '~' {
					// GFM strikethrough: opener and closer runs must be the
					// same length, so `~~x~` is not a match.
					if opener.numdelims == closer.numdelims {
						openerFound = true
						break
					}
				} else {
					// §6.4 rules 9 and 10, the "rule of three": when either
					// side of the pair could both open and close, the two run
					// lengths may not sum to a multiple of 3 — unless both are
					// themselves multiples of 3.
					oddMatch := (closer.canOpen || opener.canClose) &&
						closer.origdelims%3 != 0 &&
						(opener.origdelims+closer.origdelims)%3 == 0
					if !oddMatch {
						openerFound = true
						break
					}
				}
			}
			opener = opener.previous
		}

		oldCloser := closer

		if openerFound && opener != nil {
			// Two delimiters make strong, one makes emph; strikethrough always
			// consumes the whole (equal-length) run.
			var useDelims int
			switch {
			case closercc == '~':
				useDelims = closer.numdelims
			case closer.numdelims >= 2 && opener.numdelims >= 2:
				useDelims = 2
			default:
				useDelims = 1
			}
			var nodeType NodeType
			switch {
			case closercc == '~':
				nodeType = NodeDel
			case useDelims == 1:
				nodeType = NodeEmph
			default:
				nodeType = NodeStrong
			}

			openerInl := opener.node
			closerInl := closer.node

			opener.numdelims -= useDelims
			closer.numdelims -= useDelims
			openerInl.Literal = trimDelimTail(openerInl.Literal, useDelims)
			closerInl.Literal = trimDelimTail(closerInl.Literal, useDelims)

			// Everything between the two delimiter text nodes becomes the
			// content of the new node.
			emph := NewNode(nodeType)
			tmp := openerInl.Next
			for tmp != nil && tmp != closerInl {
				nxt := tmp.Next
				tmp.Unlink()
				emph.AppendChild(tmp)
				tmp = nxt
			}
			openerInl.InsertAfter(emph)

			// Delimiters between the pair can never match anything now.
			if opener.next != closer {
				opener.next = closer
				closer.previous = opener
			}

			if opener.numdelims == 0 {
				openerInl.Unlink()
				p.removeDelimiter(opener)
			}
			if closer.numdelims == 0 {
				closerInl.Unlink()
				tempstack := closer.next
				p.removeDelimiter(closer)
				closer = tempstack
			}
		} else {
			closer = closer.next
			openersBottom[key] = oldCloser.previous
			if !oldCloser.canOpen {
				// It found no opener and cannot be one: it is inert.
				p.removeDelimiter(oldCloser)
			}
		}
	}

	// Drop everything above the floor.
	for p.delimiters != nil && p.delimiters != stackBottom {
		p.removeDelimiter(p.delimiters)
	}
}

// --- §6.3 links and images ---

// parseLinkTitle returns the link title without its delimiters, unescaped.
// The bool is false when there is no title here — TS's `null`.
func (p *inlineParser) parseLinkTitle() (string, bool) {
	title, ok := p.match(reLinkTitle)
	if !ok {
		return "", false
	}
	return unescapeString(title[1 : len(title)-1]), true
}

// parseLinkDestination returns a destination that is backslash-unescaped and
// entity-decoded, but **not** percent-encoded: [l](/ä) yields /ä, not /%C3%A4.
//
// Encoding is a rendering concern and html.go applies normalizeURI when it
// writes the attribute. Doing it here too would be invisible in the HTML
// (normalizeURI is idempotent over its own output) but would put the encoded
// form in the public AST, where mdast — and every previous release of this
// package — promises the decoded one.
// parseLinkDestination reads either `<...>`, or a bare run with balanced
// unescaped parentheses and no ASCII control characters or spaces. The bool
// is false when there is no destination here.
func (p *inlineParser) parseLinkDestination() (string, bool) {
	if braced, ok := p.match(reLinkDestinationBraces); ok {
		return unescapeString(braced[1 : len(braced)-1]), true
	}
	if p.peek() == '<' {
		return "", false
	}

	savepos := p.pos
	openparens := 0
	c := p.peek()

scan:
	for c != -1 {
		switch {
		case c == '\\' && p.pos+1 < len(p.subject) && IsEscapable(p.subject[p.pos+1]):
			p.pos++
			if p.peek() != -1 {
				p.pos++
			}
		case c == '(':
			// Bail rather than scan on once nesting passes the limit. Without
			// this, input like `[a](b` repeated makes every `]` scan to end of
			// input looking for parens that never balance, which is quadratic
			// in the document size. cmark caps nesting at the same depth, and
			// no spec example nests more than two deep.
			if maxLinkParenNesting <= openparens {
				return "", false
			}
			p.pos++
			openparens++
		case c == ')':
			if openparens < 1 {
				break scan
			}
			p.pos++
			openparens--
		case c <= ' ' || c == 0x7F:
			// Spaces and ASCII control characters end the bare form.
			break scan
		default:
			p.pos++
		}
		c = p.peek()
	}

	// An empty destination is only legal immediately before the closing paren
	// of an inline link, as in `[link]()`.
	if p.pos == savepos && c != ')' {
		return "", false
	}
	if openparens != 0 {
		return "", false
	}

	return unescapeString(p.subject[savepos:p.pos]), true
}

// parseLinkLabel returns the byte length of the link label at pos, brackets
// included, advancing pos past it; 0 if there is no label here or it is over
// the §4.7 999-character limit.
//
// Hand-coded where TS uses /\[(?:[^\\[\]]|\\.){0,1000}\]/sy: RE2 expands a
// {0,1000} repeat of an alternation into a thousand copies of it. The scan
// below accepts the same language. A backtracking engine can never do better
// than the greedy run, because every alternative of the repeat begins on a
// character that is not `]`, so shortening the run can only put a non-`]`
// where the closing bracket must be.
func (p *inlineParser) parseLinkLabel() int {
	s := p.subject
	start := p.pos
	if start >= len(s) || s[start] != '[' {
		return 0
	}

	i := start + 1
	iters := 0
	// Length in UTF-16 code units, which is the unit TS's `m.length > 1001`
	// test counts in — hence the 2 for an astral character below.
	units := 1

	for {
		if i >= len(s) {
			return 0
		}
		c := s[i]
		if c == ']' {
			i++
			units++
			p.pos = i
			// §4.7: at most 999 characters between the brackets.
			if units > 1001 {
				return 0
			}
			return i - start
		}
		// An unescaped `[` cannot be consumed by the repeat, and is not the
		// `]` the pattern then demands. Nor can a 1001st repeat run.
		if c == '[' || iters == 1000 {
			return 0
		}
		if c == '\\' {
			// `\\.` — with the `s` flag, any character at all, line endings
			// included.
			i++
			units++
			if i >= len(s) {
				return 0
			}
		}
		r, size := utf8.DecodeRuneInString(s[i:])
		i += size
		if r > 0xFFFF {
			units += 2
		} else {
			units++
		}
		iters++
	}
}

func (p *inlineParser) addBracket(node *MdNode, index int, image bool) {
	if p.brackets != nil {
		p.brackets.bracketAfter = true
	}
	p.brackets = &bracket{
		node:              node,
		previous:          p.brackets,
		previousDelimiter: p.delimiters,
		index:             index,
		image:             image,
		active:            true,
		bracketAfter:      false,
	}
}

func (p *inlineParser) removeBracket() {
	if p.brackets != nil {
		p.brackets = p.brackets.previous
	}
}

func (p *inlineParser) parseOpenBracket(block *MdNode) bool {
	startpos := p.pos
	p.pos++
	node := textNode("[")
	block.AppendChild(node)
	p.addBracket(node, startpos, false)
	return true
}

// parseBang handles `!`, which only matters when it introduces an image.
func (p *inlineParser) parseBang(block *MdNode) bool {
	startpos := p.pos
	p.pos++
	if p.peek() == '[' {
		p.pos++
		node := textNode("![")
		block.AppendChild(node)
		p.addBracket(node, startpos+1, true)
	} else {
		block.AppendChild(textNode("!"))
	}
	return true
}

// parseCloseBracket is the spec's "look for link or image" procedure. On a
// `]`, walk back to the newest bracket opener and try, in order: an inline
// `(dest "title")`, a full `[label]`, a collapsed `[]` or a shortcut
// reference. Anything that fails backtracks to just after the `]` and leaves a
// literal `]`.
func (p *inlineParser) parseCloseBracket(block *MdNode) bool {
	p.pos++
	startpos := p.pos

	opener := p.brackets
	if opener == nil {
		block.AppendChild(textNode("]"))
		return true
	}
	if !opener.active {
		// Deactivated by an enclosing link: not an opener any more.
		block.AppendChild(textNode("]"))
		p.removeBracket()
		return true
	}

	isImage := opener.image
	dest := ""
	title := ""
	hasTitle := false
	matched := false

	savepos := p.pos

	// Inline link: `](` destination title `)`.
	if p.peek() == '(' {
		p.pos++
		p.spnl()
		if d, ok := p.parseLinkDestination(); ok {
			p.spnl()
			// A title has to be separated from the destination by whitespace,
			// so `[a](/url"t")` is not a titled link.
			t, tOK := "", false
			if p.pos-1 >= 0 && p.pos-1 < len(p.subject) && isInlineWhitespaceByte(p.subject[p.pos-1]) {
				t, tOK = p.parseLinkTitle()
			}
			p.spnl()
			if p.peek() == ')' {
				p.pos++
				dest = d
				title, hasTitle = t, tOK
				matched = true
			}
		}
		if !matched {
			p.pos = savepos
		}
	}

	if !matched {
		// Reference forms. A second label gives the full form; an empty or
		// absent one means the link text is itself the label (collapsed and
		// shortcut) — which is only possible if that text holds no bracket.
		reflabel := ""
		haveReflabel := false
		beforelabel := p.pos
		n := p.parseLinkLabel()
		if n > 2 {
			reflabel = p.subject[beforelabel+1 : beforelabel+n-1]
			haveReflabel = true
		} else if !opener.bracketAfter {
			lo, hi := opener.index+1, startpos-1
			if 0 <= lo && lo <= hi && hi <= len(p.subject) {
				reflabel = p.subject[lo:hi]
				haveReflabel = true
			}
		}
		if n == 0 {
			p.pos = savepos
		}

		if haveReflabel {
			if link, ok := p.refmap[normalizeReference(reflabel)]; ok {
				dest = link.Destination
				title = link.Title
				hasTitle = link.HasTitle
				matched = true
			}
		}
	}

	if !matched {
		p.removeBracket()
		p.pos = startpos
		block.AppendChild(textNode("]"))
		return true
	}

	nodeType := NodeLink
	if isImage {
		nodeType = NodeImage
	}
	node := NewNode(nodeType)
	node.Destination = dest
	// An empty title is the same as no title at all: `[a](/url "")` renders a
	// bare `<a href="/url">`. TS spells this `null === title || '' === title`.
	if hasTitle && title != "" {
		node.Title = title
		node.HasTitle = true
	}

	tmp := opener.node.Next
	for tmp != nil {
		nxt := tmp.Next
		tmp.Unlink()
		node.AppendChild(tmp)
		tmp = nxt
	}
	block.AppendChild(node)

	// Emphasis inside the link text resolves now, and only down to the
	// delimiter that was current when the `[` opened.
	p.processEmphasis(opener.previousDelimiter)
	p.removeBracket()
	opener.node.Unlink()

	// Links may not contain links (§6.3): every earlier link opener is spent.
	// Image openers survive, since images may nest in links.
	if !isImage {
		for b := p.brackets; b != nil; b = b.previous {
			if !b.image {
				b.active = false
			}
		}
	}

	return true
}

// --- §6.2 entities, §6.8/§6.9 breaks and plain text ---

func (p *inlineParser) parseEntity(block *MdNode) bool {
	m, ok := p.match(reEntityHere)
	if !ok {
		return false
	}
	block.AppendChild(textNode(decodeEntity(m)))
	return true
}

// parseString consumes a chunk of ordinary text: everything up to the next
// character that could start an inline construct. TS spells this as the
// sticky regexes reMain / reMainGfm; a byte scan is exact, because the
// terminators are all ASCII.
func (p *inlineParser) parseString(block *MdNode) bool {
	s := p.subject
	startpos := p.pos
	gfm := p.options.GFM
	for p.pos < len(s) && !isInlineSpecialByte(s[p.pos], gfm) {
		p.pos++
	}
	if p.pos == startpos {
		return false
	}
	block.AppendChild(textNode(s[startpos:p.pos]))
	return true
}

// parseNewline implements §6.7/§6.8: two or more spaces before the line
// ending make a hard break, anything else a soft one. Trailing spaces are
// dropped either way, as is the indentation of the next line.
func (p *inlineParser) parseNewline(block *MdNode) bool {
	p.pos++
	lastc := block.LastChild
	if lastc != nil && lastc.Type == NodeText &&
		len(lastc.Literal) > 0 && lastc.Literal[len(lastc.Literal)-1] == ' ' {
		hardbreak := len(lastc.Literal) > 1 && lastc.Literal[len(lastc.Literal)-2] == ' '
		// TS: literal.replace(/ *$/, '').
		lastc.Literal = strings.TrimRight(lastc.Literal, " ")
		if hardbreak {
			block.AppendChild(NewNode(NodeLinebreak))
		} else {
			block.AppendChild(NewNode(NodeSoftbreak))
		}
	} else {
		block.AppendChild(NewNode(NodeSoftbreak))
	}
	p.skipSpaces()
	return true
}

// parseInline consumes one inline construct at pos; false only at the end of
// the subject.
func (p *inlineParser) parseInline(block *MdNode) bool {
	c := p.peek()
	if c == -1 {
		return false
	}

	res := false
	switch c {
	case '\n':
		res = p.parseNewline(block)
	case '\\':
		res = p.parseBackslash(block)
	case '`':
		res = p.parseBackticks(block)
	case '*', '_':
		res = p.handleDelim(byte(c), block)
	case '~':
		// Only GFM gives `~` a meaning; otherwise it is ordinary text (and
		// the non-GFM text scan swallows whole runs of it).
		if p.options.GFM {
			res = p.handleDelim(byte(c), block)
		} else {
			res = p.parseString(block)
		}
	case '[':
		res = p.parseOpenBracket(block)
	case '!':
		res = p.parseBang(block)
	case ']':
		res = p.parseCloseBracket(block)
	case '<':
		res = p.parseAutolink(block) || p.parseHtmlTag(block)
	case '&':
		res = p.parseEntity(block)
	default:
		res = p.parseString(block)
	}

	if !res {
		// Nothing claimed it: the character is literal text. Only `<` and `&`
		// reach here, so the byte at pos is a whole character — TS's
		// String.fromCharCode(c) with the same effect and no mojibake risk.
		startpos := p.pos
		p.pos++
		block.AppendChild(textNode(p.subject[startpos:p.pos]))
	}

	return true
}

// parse turns one paragraph's or heading's raw content into inline children.
func (p *inlineParser) parse(block *MdNode) {
	// Trimming is what stops trailing spaces at the end of a block from
	// making a hard break, and drops the leading indentation of the first line.
	p.subject = jsTrim(string(block.StringContent))
	p.pos = 0
	p.delimiters = nil
	p.brackets = nil

	for p.parseInline(block) {
	}

	block.StringContent = nil
	p.processEmphasis(nil)
}

// parseReference implements §4.7: consume leading link reference definitions.
// Called by the block phase while finalizing a paragraph, repeatedly, until it
// returns 0.
func (p *inlineParser) parseReference(s string) int {
	p.subject = s
	p.pos = 0

	startpos := p.pos

	// §4.7 allows up to three spaces of indentation before the label. The
	// block phase normally strips a paragraph line's indentation before it
	// gets here, so this usually consumes nothing.
	p.skipIndent()

	labelStart := p.pos
	matchChars := p.parseLinkLabel()
	if matchChars == 0 {
		return 0
	}
	rawlabel := p.subject[labelStart+1 : labelStart+matchChars-1]

	if p.peek() == ':' {
		p.pos++
	} else {
		p.pos = startpos
		return 0
	}

	p.spnl()

	dest, ok := p.parseLinkDestination()
	if !ok {
		p.pos = startpos
		return 0
	}

	beforetitle := p.pos
	p.spnl()
	title, hasTitle := "", false
	if p.pos != beforetitle {
		title, hasTitle = p.parseLinkTitle()
	}
	if !hasTitle {
		p.pos = beforetitle
	}

	// Nothing but spaces may follow on the definition's last line.
	atLineEnd := true
	if !p.spaceAtEndOfLine() {
		if !hasTitle {
			atLineEnd = false
		} else {
			// What looked like a title is something else; the definition may
			// still be valid without it.
			title, hasTitle = "", false
			p.pos = beforetitle
			atLineEnd = p.spaceAtEndOfLine()
		}
	}
	if !atLineEnd {
		p.pos = startpos
		return 0
	}

	normlabel := normalizeReference(rawlabel)
	if normlabel == "" {
		// A label must hold at least one non-whitespace character.
		p.pos = startpos
		return 0
	}

	// First definition wins.
	if _, exists := p.refmap[normlabel]; !exists {
		p.refmap[normlabel] = RefDef{Destination: dest, Title: title, HasTitle: hasTitle}
	}

	return p.pos - startpos
}

// --- GFM extended autolinks (autolink literals) -----------------------------
//
// GFM recognises `www.x.com`, `https://x.com` and `a@x.com` without the `<…>`
// CommonMark requires. This runs as a post-pass over the finished inline tree
// rather than as another branch of the scan above, which is how cmark-gfm does
// it and is the only shape that keeps 652/652 safe: the scanner that decides
// code spans, raw HTML, emphasis and links is not touched at all, and an
// autolink can therefore never win against a construct CommonMark says comes
// first.
//
// The pass rewrites text nodes into text/link/text runs. It does not descend
// into link (links may not contain links) or image (its children are the alt
// text), and code/html_inline are leaves it never looks inside.
//
// Everything here scans by BYTE, like the rest of this file. Every character
// the pass branches on is ASCII, and no byte of a multi-byte UTF-8 sequence
// can collide with one, so a non-ASCII character is exactly as unmatchable
// here as its UTF-16 code unit is in the TypeScript — it ends a domain run, it
// is not a valid autolink start, and it is not trailing punctuation. Slices
// therefore never cut a rune in half: every boundary this pass produces sits
// at an ASCII character. The two length caps below are in bytes rather than
// UTF-16 units, which is the same number: they can only bind inside a run of
// ASCII domain characters.

// maxAutolinkDomain is RFC 1035's limit on a fully-qualified domain name. See
// scanDomainEnd.
const maxAutolinkDomain = 253

// maxEmailLocal is RFC 5321's limit on the local part of an address, before
// the `@`.
const maxEmailLocal = 64

// autolinkSchemes are the three schemes the extension recognises without angle
// brackets.
var autolinkSchemes = []string{"https://", "http://", "ftp://"}

func isASCIIAlnum(c byte) bool {
	return ('0' <= c && c <= '9') || ('A' <= c && c <= 'Z') || ('a' <= c && c <= 'z')
}

func isAutolinkSpace(c byte) bool {
	switch c {
	case ' ', '\t', '\n', '\v', '\f', '\r':
		return true
	}
	return false
}

// isDomainChar reports the characters a domain is built from: a segment holds
// alphanumerics, `_` and `-`, and `.` separates them.
func isDomainChar(c byte) bool {
	return isASCIIAlnum(c) || '_' == c || '-' == c || '.' == c
}

// isEmailLocalChar reports what may appear before the `@`: alphanumerics and
// `.`, `-`, `_`, `+`.
func isEmailLocalChar(c byte) bool {
	return isASCIIAlnum(c) || '.' == c || '-' == c || '_' == c || '+' == c
}

// isAutolinkStart implements "All such recognized autolinks can only come at
// the beginning of a line, after whitespace, or any of the delimiting
// characters `*`, `_`, `~`, `(`."
//
// Offset 0 has no preceding character in s at all, so the caller answers for it
// through atStart — see startsAfterDelimiter.
func isAutolinkStart(s string, i int, atStart bool) bool {
	if 0 == i {
		return atStart
	}
	c := s[i-1]
	return isAutolinkSpace(c) || '*' == c || '_' == c || '~' == c || '(' == c
}

// startsAfterDelimiter reports whether offset 0 of a text node may begin an
// autolink, given the sibling in front of it.
//
// The rule above is about the source character immediately before the match,
// and by the time this pass runs the tree no longer carries source offsets —
// but the previous sibling's *type* names that character exactly:
//
//   - NodeEmph and NodeStrong end on the `*` or `_` that closed them, and
//     NodeDel on its second `~`. All three are delimiters the rule allows, so
//     `*a*www.b.com` links.
//   - NodeCode ends on a backtick, NodeLink and NodeImage on `)`, `]` or `>`,
//     and NodeHTMLInline on `>`. None are delimiters, so `x`www.a.com,
//     `[l](/u)www.a.com` and `<b>www.a.com` do not link.
//   - a NodeSoftbreak or NodeLinebreak puts offset 0 at the beginning of a
//     line, and so does having no previous sibling — that is the first inline
//     of the block, or of the emphasis span this pass descended into.
//
// A NodeText never appears here: linkifyChildren merges every run of adjacent
// text siblings before handing the first of them over.
func startsAfterDelimiter(prev *MdNode) bool {
	if nil == prev {
		return true
	}
	switch prev.Type {
	case NodeSoftbreak, NodeLinebreak, NodeEmph, NodeStrong, NodeDel:
		return true
	}
	return false
}

// matchesIgnoreCase is an ASCII case-insensitive comparison against an
// already-lowercase literal.
func matchesIgnoreCase(s string, at int, lower string) bool {
	if at+len(lower) > len(s) {
		return false
	}
	for k := 0; k < len(lower); k++ {
		c := s[at+k]
		if 'A' <= c && c <= 'Z' {
			c += 32
		}
		if c != lower[k] {
			return false
		}
	}
	return true
}

// scanDomainEnd returns the end of the run of domain characters at from,
// capped at maxAutolinkDomain.
//
// The cap is what keeps this pass linear. `_` both continues a domain and
// introduces a valid autolink start, so without a bound an input like
// strings.Repeat("_www.a_b.c", n) gives every underscore a candidate whose
// domain run reaches the end of the text — quadratic. Anything past the cap is
// still scanned, just as the path rather than as the domain, and no real
// domain comes near it.
func scanDomainEnd(s string, from int) int {
	max := len(s)
	if from+maxAutolinkDomain < max {
		max = from + maxAutolinkDomain
	}
	i := from
	for i < max && isDomainChar(s[i]) {
		i++
	}
	return i
}

// isValidDomain reports a valid domain: at least one period, and no underscore
// in either of the last two segments. from..to must already be a run of domain
// characters.
func isValidDomain(s string, from int, to int) bool {
	if to <= from {
		return false
	}
	periods := 0
	underscoresInLast := 0
	underscoresInPrev := 0
	for i := from; i < to; i++ {
		switch s[i] {
		case '_':
			underscoresInLast++
		case '.':
			underscoresInPrev = underscoresInLast
			underscoresInLast = 0
			periods++
		}
	}
	return 0 < periods && 0 == underscoresInLast && 0 == underscoresInPrev
}

// trimAutolinkEnd implements "extended autolink path validation" — pull the
// end of the match back off trailing characters that read as sentence
// punctuation rather than as part of a URL. One loop rather than three passes,
// because dropping a `)` can expose a `.` and vice versa.
//
//   - `?` `!` `.` `,` `:` `*` `_` `~` are dropped wherever they trail;
//   - a trailing `)` is dropped only while the match holds more `)` than `(`,
//     so `(www.a.com/x_(y))` keeps its inner pair and loses the outer one;
//   - a trailing `;` preceded by something shaped like an entity reference
//     takes the whole `&…;` with it.
//
// The parenthesis counts are taken once and then maintained, not recomputed
// per drop: a deeply parenthesised URL is otherwise quadratic in its own
// length.
func trimAutolinkEnd(s string, start int, end int) int {
	opening := -1
	closing := -1

	for start < end {
		c := s[end-1]

		switch c {
		case '?', '!', '.', ',', ':', '*', '_', '~':
			end--
			continue

		case ')':
			if opening < 0 {
				opening = 0
				closing = 0
				for i := start; i < end; i++ {
					switch s[i] {
					case '(':
						opening++
					case ')':
						closing++
					}
				}
			}
			if closing <= opening {
				return end
			}
			closing--
			end--
			continue

		case ';':
			// `&` followed by one or more alphanumerics, then this `;`.
			j := end - 2
			for j > start && isASCIIAlnum(s[j]) {
				j--
			}
			if j < end-2 && '&' == s[j] {
				end = j
				continue
			}
			return end
		}

		return end
	}

	return end
}

// autolinkMatch is one recognised autolink literal: s[start:end] linking to
// href.
type autolinkMatch struct {
	start int
	end   int
	href  string
}

// scanAutolinks returns every autolink literal in one text run, left to right
// and non-overlapping. It returns nil when there is nothing to do, which is
// the overwhelmingly common case and saves the caller a slice and a splice.
func scanAutolinks(s string, atStart bool) []autolinkMatch {
	length := len(s)
	var out []autolinkMatch

	// The maximal run of non-space, non-`<` characters containing the current
	// position — "zero or more non-space non-`<` characters may follow" the
	// domain. Cached because candidates are tried left to right, so each run is
	// scanned once however many candidates start inside it.
	runStart := -1
	runEnd := -1

	i := 0
	lastEnd := 0

	for i < length {
		c := s[i]

		// --- email: found by its `@`, then rewound to the start of the local
		// part
		if '@' == c {
			// The rewind is bounded to keep this pass linear, and RFC 5321
			// bounds a local part in the same place anyway, so the cap costs no
			// real address.
			capped := i - maxEmailLocal
			floor := lastEnd
			if capped > floor {
				floor = capped
			}
			k := i
			for k > floor && isEmailLocalChar(s[k-1]) {
				k--
			}

			// Stopping *on* the cap is not the same as finding a boundary. If
			// the character just outside it still belongs to the local part,
			// the local part is longer than the cap allows and there is no
			// address here at all: accepting the match would link the
			// 64-character tail of a longer run and leave the rest as text,
			// inventing an address the source never wrote. k can only reach
			// capped when the cap, rather than lastEnd, is what ended the loop;
			// and k of 0 is the start of the string, where there is no
			// character outside the cap to disqualify anything.
			if k == capped && 0 < k && isEmailLocalChar(s[k-1]) {
				i++
				continue
			}

			if k < i && isAutolinkStart(s, k, atStart) {
				// After the `@`: alphanumerics, `.`, `-`, `_`; at least one
				// period; a trailing `.` is not part of the address, and a
				// trailing `-` or `_` invalidates the whole thing.
				end := i + 1
				for end < length && isDomainChar(s[end]) {
					end++
				}
				for end > i+1 && '.' == s[end-1] {
					end--
				}

				var last byte
				if end > i+1 {
					last = s[end-1]
				}
				if end > i+1 && '-' != last && '_' != last && hasPeriod(s, i+1, end) {
					out = append(out, autolinkMatch{start: k, end: end, href: "mailto:" + s[k:end]})
					lastEnd = end
					i = end
					continue
				}
			}
			i++
			continue
		}

		// --- www / scheme: both need a valid start and a valid domain
		if !couldStartURL(c) || !isAutolinkStart(s, i, atStart) {
			i++
			continue
		}

		domainStart := -1
		prefix := ""
		if matchesIgnoreCase(s, i, "www.") {
			// The domain of a `www.` autolink includes the `www.` itself, so
			// the required period is already there and `www.foo_bar.com` is
			// rejected on the same footing as `http://foo_bar.com`.
			domainStart = i
			prefix = "http://"
		} else {
			for _, scheme := range autolinkSchemes {
				if matchesIgnoreCase(s, i, scheme) {
					domainStart = i + len(scheme)
					break
				}
			}
		}
		if 0 > domainStart {
			i++
			continue
		}

		domainEnd := scanDomainEnd(s, domainStart)
		// Checked before the run scan below, so a candidate that cannot
		// possibly match costs only its own domain run.
		if !isValidDomain(s, domainStart, domainEnd) {
			i++
			continue
		}

		if i >= runEnd || i < runStart {
			runStart = i
			runEnd = i
			for runEnd < length {
				d := s[runEnd]
				if isAutolinkSpace(d) || '<' == d {
					break
				}
				runEnd++
			}
		}

		end := trimAutolinkEnd(s, i, runEnd)
		// Trailing punctuation can eat back into the domain —
		// `www.commonmark.org.` loses its last period — so the domain is
		// validated again over what actually survived.
		survived := domainEnd
		if end < survived {
			survived = end
		}
		if end <= domainStart || !isValidDomain(s, domainStart, survived) {
			i++
			continue
		}

		out = append(out, autolinkMatch{start: i, end: end, href: prefix + s[i:end]})
		lastEnd = end
		i = end
	}

	return out
}

// couldStartURL reports the first character of `www.`, `http://`, `https://`
// and `ftp://`, in either case.
func couldStartURL(c byte) bool {
	return 'w' == c || 'W' == c || 'h' == c || 'H' == c || 'f' == c || 'F' == c
}

func hasPeriod(s string, from int, to int) bool {
	for i := from; i < to; i++ {
		if '.' == s[i] {
			return true
		}
	}
	return false
}

// linkifyTextNode replaces one text node with the text/link/text run its
// literal implies.
func linkifyTextNode(node *MdNode) {
	s := node.Literal
	matches := scanAutolinks(s, startsAfterDelimiter(node.Prev))
	if nil == matches {
		return
	}

	at := 0
	for _, m := range matches {
		if at < m.start {
			node.InsertBefore(textNode(s[at:m.start]))
		}
		link := NewNode(NodeLink)
		// Not percent-encoded here, for the same reason as
		// parseLinkDestination: encoding belongs to the renderer, and the AST
		// promises the decoded form.
		link.Destination = m.href
		link.AppendChild(textNode(s[m.start:m.end]))
		node.InsertBefore(link)
		at = m.end
	}
	if at < len(s) {
		node.InsertBefore(textNode(s[at:]))
	}

	node.Unlink()
}

// linkifyChildren autolinks the text children of one container and returns
// pending extended with the child containers still to visit. (The TypeScript
// pushes onto the caller's array in place; Go's append may reallocate, so the
// slice comes back instead.)
func linkifyChildren(parent *MdNode, pending []*MdNode) []*MdNode {
	child := parent.FirstChild

	for nil != child {
		if NodeText == child.Type {
			// Consolidate the run of adjacent text nodes first. The scan above
			// emits a node per delimiter run and per character it could not
			// claim, so `a.b-c_d@a.b` arrives as three siblings and the address
			// is only visible once they are one string. Merging changes nothing
			// downstream: the renderer writes text literals back to back, and
			// the AST projection already coalesces adjacent runs.
			sibling := child.Next
			if nil != sibling && NodeText == sibling.Type {
				var merged strings.Builder
				merged.WriteString(child.Literal)
				for nil != sibling && NodeText == sibling.Type {
					merged.WriteString(sibling.Literal)
					next := sibling.Next
					sibling.Unlink()
					sibling = next
				}
				child.Literal = merged.String()
			}

			after := child.Next
			linkifyTextNode(child)
			child = after
			continue
		}

		// No links inside links (§6.3), and an image's children are its alt
		// text.
		if NodeLink != child.Type && NodeImage != child.Type && child.IsContainer() {
			pending = append(pending, child)
		}
		child = child.Next
	}

	return pending
}

// linkifyAutolinks recognises GFM autolink literals throughout one finished
// block's inlines.
//
// Iterative rather than recursive: emphasis nests as deeply as the input says.
func linkifyAutolinks(block *MdNode) {
	pending := []*MdNode{block}
	for 0 < len(pending) {
		node := pending[len(pending)-1]
		pending = linkifyChildren(node, pending[:len(pending)-1])
	}
}

// inlineContentBlocks are the blocks whose raw content the block phase left
// for this phase to parse.
var inlineContentBlocks = map[NodeType]bool{
	NodeParagraph: true,
	NodeHeading:   true,
	// GFM table cells hold inlines and nothing else — "block-level elements
	// cannot be inserted in a table" — so a cell is parsed exactly as a
	// paragraph is, autolink post-pass included.
	NodeTableCell: true,
}

// parseInlines is the phase 2 entry point: walk the block tree and parse the
// string content of every paragraph, heading and table cell into inline
// children.
func parseInlines(doc *MdNode, refmap RefMap, opts Options) {
	parser := &inlineParser{refmap: refmap, options: opts}
	walker := doc.Walker()

	for ev := walker.Next(); ev != nil; ev = walker.Next() {
		node := ev.Node
		if !ev.Entering && inlineContentBlocks[node.Type] {
			parser.parse(node)
			if opts.GFM {
				linkifyAutolinks(node)
			}
		}
	}
}

// parseReference consumes link reference definitions from the front of a
// paragraph's raw content, adding them to refmap. It returns the number of
// BYTES consumed (TS counts UTF-16 units; block.go slices by byte), or 0 if s
// does not start with a definition.
//
// TS keeps one module-level InlineParser aside for this, because reference
// definitions are parsed during the block phase, before any parser for the
// document exists. Go allocates a fresh one per call instead: package-level
// mutable state would make concurrent parses race, and the struct is four
// words. Nothing here depends on the parse options.
func parseReference(s string, refmap RefMap) int {
	parser := &inlineParser{refmap: refmap, options: Options{GFM: false, Breaks: false}}
	return parser.parseReference(s)
}
