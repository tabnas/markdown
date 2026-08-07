/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Phase 1 of the CommonMark parse: block structure. Port of ts/src/block.ts,
// which is canonical — keep the two in step, function for function.
// Spec 0.31.2, Appendix A "A parsing strategy".
//
// The document is a tree of blocks whose right spine — the document, then last
// child, then last child of that, and so on — is *open*: each open block
// imposes a continuation condition on the next line. For every line we
//
//  1. walk that spine, consuming each block's continuation marker (`>` for a
//     block quote, the content indent for a list item, ...) and remember the
//     last container that matched;
//  2. look for new block starts at the offset that walk reached, closing the
//     unmatched tail of the spine as soon as a start actually matches;
//  3. add whatever is left of the line to the deepest open block, opening a
//     paragraph if there is nowhere else for the text to go.
//
// Unmatched blocks are closed in step 2/3 rather than in step 1 because a
// paragraph line may be a *lazy continuation*: its containers' markers are
// missing, but the paragraph carries on regardless (§5.1 rule 2).
//
// Positions on a line are tracked twice: `offset`, an index used to slice the
// text we keep, and `column`, the tab-expanded display column used for every
// indent decision (§2.2). The two must move together, including the case where
// a marker consumes only part of a tab — see advanceOffset and addLine.
//
// This file owns everything the block phase sets on a node: type, level,
// literal (code_block/html_block), info/isFenced, listData (including the
// final `tight`), sourcepos, and `StringContent` — the raw, unparsed text of
// paragraphs and headings that the inline phase consumes.
//
// Where Go differs from the TypeScript, and why:
//
//   - `offset` is a BYTE index into the line, where the TypeScript's is a
//     UTF-16 index. Every block delimiter is ASCII and no UTF-8 continuation
//     byte can collide with one, so scanning by byte is safe; `column` and
//     sourcepos columns still count CHARACTERS, which is what §2.2 and the
//     spec's sourcepos mean, so byte offsets pass through charCount on the way
//     to a column.
//   - `stringContent` is []byte here, appended to rather than concatenated.
//   - The two regexes in block.ts that use lookahead (RE_CODE_FENCE and
//     RE_CLOSING_CODE_FENCE) are hand-coded: RE2 has no lookahead. Fence runs
//     are counted with a loop, never with a regex built from an input-derived
//     repeat count.

import (
	"bytes"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

// §4.4: four columns of indentation open an indented code block.
const codeIndent = 4

// The TypeScript's C_TAB/C_SPACE/C_GREATERTHAN/C_LESSTHAN/C_OPEN_BRACKET are
// spelled as Go character literals at their use sites instead of as package
// constants: JavaScript has no character literal, Go does, and inline.go ports
// the same C_* set from inline.ts into this one shared package scope.

// §2.1: \r\n, \n and a lone \r are all line endings. The TypeScript splits on
// /\r\n|\n|\r/; splitLines is that split, spelled out.
func splitLines(s string) []string {
	lines := make([]string, 0, 1+strings.Count(s, "\n"))
	start := 0
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '\n':
			lines = append(lines, s[start:i])
			start = i + 1
		case '\r':
			lines = append(lines, s[start:i])
			if i+1 < len(s) && s[i+1] == '\n' {
				i++
			}
			start = i + 1
		}
	}
	return append(lines, s[start:])
}

// isThematicBreak implements §4.1 and the TypeScript's RE_THEMATIC_BREAK,
// /^(?:\*[ \t]*){3,}$|^(?:_[ \t]*){3,}$|^(?:-[ \t]*){3,}$/: 3+ matching `*`,
// `_` or `-`, each optionally followed by spaces/tabs, and nothing else.
func isThematicBreak(s string) bool {
	if len(s) == 0 {
		return false
	}
	c := s[0]
	if c != '*' && c != '_' && c != '-' {
		return false
	}
	count := 0
	for i := 0; i < len(s); i++ {
		switch b := s[i]; {
		case b == c:
			count++
		case b == ' ' || b == '\t':
		default:
			return false
		}
	}
	return count >= 3
}

// maybeSpecial is the fast reject RE_MAYBE_SPECIAL, /^[#`~*+_=<>0-9-]/: every
// block start begins with one of these characters.
func maybeSpecial(ln string, pos int) bool {
	if pos < 0 || pos >= len(ln) {
		return false
	}
	switch c := ln[pos]; c {
	case '#', '`', '~', '*', '+', '_', '=', '<', '>', '-':
		return true
	default:
		return '0' <= c && c <= '9'
	}
}

// maybeSpecialGFM is the same fast reject with `|` and `:` added, used when
// GFM is on: a GFM table's delimiter row may begin with either (`| --- |`,
// `:-: | ---:`), and neither is a CommonMark block start.
//
// Kept as a separate function rather than folded into the one above so that a
// GFM-off parse takes exactly the path it always did — same characters
// rejected, same lines never offered to the block starts at all.
func maybeSpecialGFM(ln string, pos int) bool {
	if pos < 0 || pos >= len(ln) {
		return false
	}
	switch c := ln[pos]; c {
	case '#', '`', '~', '*', '+', '_', '=', '<', '>', '-', ':', '|':
		return true
	default:
		return '0' <= c && c <= '9'
	}
}

// isBlank is `!RE_NON_SPACE.test(s)` for RE_NON_SPACE = /[^ \t\f\v\r\n]/. All
// members of the set are ASCII, so a byte scan classifies UTF-8 correctly: a
// continuation byte is >= 0x80 and therefore counts as non-space, exactly as
// the character it belongs to would.
func isBlank(s string) bool {
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case ' ', '\t', '\f', '\v', '\r', '\n':
		default:
			return false
		}
	}
	return true
}

// §5.2 list markers. RE_BULLET_LIST_MARKER is /^[*+-]/ (always one character
// wide); matchOrderedListMarker is /^(\d{1,9})([.)])/, returning the digit
// count and the delimiter, or 0 when there is no marker. Ten or more digits do
// not match: `\d{1,9}` can only give back digits, and a digit is not `.` or `)`.
func isBulletListMarker(s string) bool {
	return len(s) > 0 && (s[0] == '*' || s[0] == '+' || s[0] == '-')
}

func matchOrderedListMarker(s string) (digits int, delimiter byte) {
	i := 0
	for i < len(s) && '0' <= s[i] && s[i] <= '9' {
		i++
	}
	if i < 1 || i > 9 || i >= len(s) {
		return 0, 0
	}
	if s[i] == '.' || s[i] == ')' {
		return i, s[i]
	}
	return 0, 0
}

// matchATXHeadingMarker implements RE_ATX_HEADING_MARKER, /^#{1,6}(?:[ \t]+|$)/
// (§4.2): 1–6 `#` followed by spaces/tabs or the end of the line. It returns
// the level (the number of `#`) and the whole match's length, or 0 for neither.
// Seven or more `#` do not match, since backtracking to six still leaves a `#`
// where a space, tab or line end is required.
func matchATXHeadingMarker(s string) (level int, matchLen int) {
	n := 0
	for n < len(s) && s[n] == '#' {
		n++
	}
	if n < 1 || n > 6 {
		return 0, 0
	}
	if n == len(s) {
		return n, n
	}
	if s[n] != ' ' && s[n] != '\t' {
		return 0, 0
	}
	i := n
	for i < len(s) && (s[i] == ' ' || s[i] == '\t') {
		i++
	}
	return n, i
}

// matchCodeFence implements RE_CODE_FENCE, /^`{3,}(?!.*`)|^~{3,}/ (§4.5),
// returning the fence length or 0. RE2 has no lookahead, so the "no further
// backtick on the line" condition is a scan: a backtick fence is only a fence
// if the rest of the line — the info string — holds no backtick. Backtracking
// to a shorter run cannot rescue it, because the run's own next character is
// then a backtick. A tilde fence has no such restriction.
func matchCodeFence(s string) int {
	if len(s) == 0 {
		return 0
	}
	c := s[0]
	if c != '`' && c != '~' {
		return 0
	}
	n := 0
	for n < len(s) && s[n] == c {
		n++
	}
	if n < 3 {
		return 0
	}
	if c == '`' && strings.IndexByte(s[n:], '`') >= 0 {
		return 0
	}
	return n
}

// matchClosingCodeFence implements RE_CLOSING_CODE_FENCE,
// /^(?:`{3,}|~{3,})(?=[ \t]*$)/, returning the fence length or 0. Same
// reasoning as matchCodeFence for the lookahead: the maximal run must be
// followed by spaces or tabs only.
func matchClosingCodeFence(s string) int {
	if len(s) == 0 {
		return 0
	}
	c := s[0]
	if c != '`' && c != '~' {
		return 0
	}
	n := 0
	for n < len(s) && s[n] == c {
		n++
	}
	if n < 3 {
		return 0
	}
	for i := n; i < len(s); i++ {
		if s[i] != ' ' && s[i] != '\t' {
			return 0
		}
	}
	return n
}

// matchSetextHeadingLine implements RE_SETEXT_HEADING_LINE,
// /^(?:=+|-+)[ \t]*$/ (§4.3): a run of `=` or `-` with only spaces/tabs after
// it. Returns the run's character, or 0 when the line is not an underline.
func matchSetextHeadingLine(s string) byte {
	if len(s) == 0 {
		return 0
	}
	c := s[0]
	if c != '=' && c != '-' {
		return 0
	}
	i := 0
	for i < len(s) && s[i] == c {
		i++
	}
	for ; i < len(s); i++ {
		if s[i] != ' ' && s[i] != '\t' {
			return 0
		}
	}
	return c
}

// --- HTML blocks (§4.6) -----------------------------------------------------
//
// Seven kinds, each with its own start and end condition. Types 1–5 end on a
// line *containing* their closing string (checked in incorporateLine after the
// line is added); types 6 and 7 end at a blank line (checked in the html_block
// continuation condition). Type 7 alone may not interrupt a paragraph.

// Tag-shape fragments shared by HTML block type 7 (§6.6 raw HTML). Prefixed
// because inline.go ports fragments of the same TypeScript names into the same
// Go package scope, and the two genuinely differ: the block forms allow only
// spaces and tabs where the inline forms allow any whitespace.
const (
	blockTagName            = `[A-Za-z][A-Za-z0-9-]*`
	blockAttributeName      = `[a-zA-Z_:][a-zA-Z0-9:._-]*`
	blockUnquotedValue      = "[^\"'=<>`\\x00-\\x20]+"
	blockSingleQuotedValue  = `'[^']*'`
	blockDoubleQuotedValue  = `"[^"]*"`
	blockAttributeValue     = `(?:` + blockUnquotedValue + `|` + blockSingleQuotedValue + `|` + blockDoubleQuotedValue + `)`
	blockAttributeValueSpec = `(?:[ \t]*=[ \t]*` + blockAttributeValue + `)`
	blockAttribute          = `(?:[ \t]+` + blockAttributeName + blockAttributeValueSpec + `?)`
	blockOpenTag            = `<` + blockTagName + blockAttribute + `*[ \t]*/?>`
	blockCloseTag           = `</` + blockTagName + `[ \t]*>`
)

// blockTags is the type 6 tag list, verbatim from §4.6 (0.31.2 has `search`,
// not `source`).
const blockTags = `address|article|aside|base|basefont|blockquote|body|caption|` +
	`center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|` +
	`figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|` +
	`li|link|main|menu|menuitem|nav|noframes|ol|optgroup|option|p|param|search|` +
	`section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul`

// blockRawTextTags is the type 1 tag list — the four whose content is raw text.
const blockRawTextTags = `script|pre|textarea|style`

// asciiFoldPattern spells a lowercase regex literal in both cases, mapping
// each ASCII letter to a two-element class and passing everything else through
// (so `h[1-6]` becomes `[hH][1-6]` and `|` stays an alternation).
//
// This is what the TypeScript's `i` flag means and what Go's `(?i)` does not:
// Go folds case over all of Unicode, so `(?i)script` also matches `ſcript` and
// `(?i)[A-Za-z]` also matches U+017F LATIN SMALL LETTER LONG S and U+212A
// KELVIN SIGN, where JavaScript's non-unicode `i` deliberately never folds a
// non-ASCII code point onto an ASCII one. Left as `(?i)`, `<ſcript>` and
// `<linK>` opened an HTML block here and a paragraph there — the same trap
// inline.go's raw-HTML grammar already spells its way around.
func asciiFoldPattern(s string) string {
	var b strings.Builder
	b.Grow(4 * len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch {
		case 'a' <= c && c <= 'z':
			b.WriteByte('[')
			b.WriteByte(c)
			b.WriteByte(c - 32)
			b.WriteByte(']')
		case 'A' <= c && c <= 'Z':
			b.WriteByte('[')
			b.WriteByte(c + 32)
			b.WriteByte(c)
			b.WriteByte(']')
		default:
			b.WriteByte(c)
		}
	}
	return b.String()
}

// reHTMLBlockOpen is indexed by HTML block type 1–7; slot 0 is unused. Go's
// `^`/`$` match at the ends of the text (never at a line break) unless (?m) is
// set, which is what the JavaScript regexes mean too.
//
// Type 7 needs no case folding at all: every letter in blockOpenTag and
// blockCloseTag is already written as a both-cases class, so the TypeScript's
// `i` flag has nothing to do there.
var reHTMLBlockOpen = []*regexp.Regexp{
	nil,
	regexp.MustCompile(`^<(?:` + asciiFoldPattern(blockRawTextTags) + `)(?:[ \t]|>|$)`),
	regexp.MustCompile(`^<!--`),
	regexp.MustCompile(`^<[?]`),
	regexp.MustCompile(`^<![A-Za-z]`),
	regexp.MustCompile(`^<!\[CDATA\[`),
	regexp.MustCompile(`^</?(?:` + asciiFoldPattern(blockTags) + `)(?:[ \t]|/?>|$)`),
	regexp.MustCompile(`^(?:` + blockOpenTag + `|` + blockCloseTag + `)[ \t]*$`),
}

var reHTMLBlockCloseRawText = regexp.MustCompile(`</(?:` + asciiFoldPattern(blockRawTextTags) + `)>`)

// htmlBlockClose holds the end conditions for types 1–5; types 6 and 7 end at
// a blank line instead. The TypeScript keeps five regexes; only type 1 is
// case-insensitive and needs one here, the rest are substring searches.
var htmlBlockClose = []func(string) bool{
	nil,
	func(s string) bool { return reHTMLBlockCloseRawText.MatchString(s) },
	func(s string) bool { return strings.Contains(s, "-->") },
	func(s string) bool { return strings.Contains(s, "?>") },
	func(s string) bool { return strings.Contains(s, ">") },
	func(s string) bool { return strings.Contains(s, "]]>") },
}

// --- small lexical helpers --------------------------------------------------

// peek is the byte at pos, or -1 past the end of the line. Every caller
// compares against an ASCII constant, which no UTF-8 continuation byte can
// equal, so a byte here is as good as the TypeScript's code unit.
func peek(ln string, pos int) int {
	if 0 <= pos && pos < len(ln) {
		return int(ln[pos])
	}
	return -1
}

func isSpaceOrTab(c int) bool {
	return ' ' == c || '\t' == c
}

// charCount is the number of characters in s. `offset` is a byte index in this
// port; sourcepos columns count characters, so the conversion happens here.
func charCount(s string) int {
	return utf8.RuneCountInString(s)
}

// sourceColumn is the number of characters of currentLine before byte offset —
// i.e. charCount(p.currentLine[:offset]), which is what a sourcepos column
// needs.
//
// The TypeScript writes this as plain `offset`, because there an offset is
// already a UTF-16 index. Counting it here is the one place the byte-offset
// port would change the *complexity* rather than just the code: addChild calls
// this once per container opened, so a naive count from the start of the line
// makes a line of n nested containers — `> - > - > - …` — cost O(n²). The
// canonical runtime is linear on that input, and untrusted Markdown decides n.
//
// So the count is carried forward from the last offset already counted.
// addChild's offsets advance monotonically as a line is consumed, which makes
// this amortized O(1); a smaller offset than the anchor is still handled
// correctly, just by counting from the start again.
func (p *blockParser) sourceColumn(offset int) int {
	if offset < p.colAnchorOffset {
		p.colAnchorOffset = 0
		p.colAnchorChars = 0
	}
	p.colAnchorChars += charCount(p.currentLine[p.colAnchorOffset:offset])
	p.colAnchorOffset = offset
	return p.colAnchorChars
}

// jsTrim, which cuts a fenced code block's info string identically in both
// runtimes, lives in common.go \u2014 inline.go needs the same set to decide hard
// line breaks, and normalizeReference needs it for \u00A74.7 label matching.

// trailingBlankRunStart is the index at which the trailing /(\n *)+/ of b
// begins, or len(b) when there is none — i.e. the offset the JavaScript
// `.replace(/(\n *)+$/, ...)` would rewrite from.
func trailingBlankRunStart(b []byte) int {
	end := len(b)
	for {
		j := end
		for j > 0 && b[j-1] == ' ' {
			j--
		}
		if j > 0 && b[j-1] == '\n' {
			end = j - 1
			continue
		}
		return end
	}
}

// --- GFM tables (extension) -------------------------------------------------
//
// Everything the extension needs from a line is here: how a row splits into
// cells, and whether a line is a delimiter row. Both are pure functions of one
// line of text, which is what lets the block start below stay a few lines
// long.
//
// Byte scanning throughout, as everywhere else in this file: every character
// these functions test for — space, tab, `|`, `\`, `-`, `:` — is ASCII, and no
// UTF-8 continuation byte can equal one, so a byte index is as good as the
// canonical runtime's UTF-16 index. The cell text itself is only ever *sliced*
// at those positions, never decoded character by character, so a multi-byte
// rune inside a cell is carried through whole.

// isASCIISpace is the set cmark_isspace trims from a table cell. Not
// isJSSpace: this is ASCII whitespace only, which is what the extension
// trims, and it is a byte test rather than a rune test for the reason above.
func isASCIISpace(c byte) bool {
	return 32 == c || (9 <= c && c <= 13)
}

func trimASCIISpace(s string) string {
	start := 0
	end := len(s)
	for start < end && isASCIISpace(s[start]) {
		start++
	}
	for end > start && isASCIISpace(s[end-1]) {
		end--
	}
	return s[start:end]
}

// splitTableRow splits one row of a table into its cell texts.
//
// Cells are separated by *unescaped* pipes; a leading and a trailing pipe are
// both optional and may differ from row to row, so they are stripped before
// the split rather than producing empty cells at the ends. A trailing pipe is
// only a delimiter if it is itself unescaped, which is why the backslash run
// before it is counted.
//
// `\|` is resolved to a literal `|` *here*, as the extension requires ("include
// a pipe in a cell's content by escaping it, including inside other inline
// spans"). Doing it at split time is the whole point: the inline phase never
// sees the backslash, so a code span in the cell yields a real pipe —
// "b `\|` az" is "b <code>|</code> az", which no amount of unescaping *after*
// inline parsing could produce. Every other backslash escape is left exactly as
// written for the inline phase to handle in the usual way, so no cell text is
// unescaped twice.
//
// Scanning skips the byte after any backslash, so `\\` cannot shield the pipe
// that follows it: `a\\|b` is two cells, the first holding `a\\`. Skipping one
// byte rather than one rune is deliberate and safe: the byte after a backslash
// is either ASCII, in which case it is the escaped character, or a UTF-8 lead
// byte, in which case the continuation bytes that follow it can equal neither
// `\` nor `|` and are simply copied.
//
// A row of nothing but whitespace has no cells at all, which is how a header
// candidate that is really empty fails the cell-count test instead of matching
// a one-column delimiter row.
func splitTableRow(line string) []string {
	start := 0
	end := len(line)
	for start < end && isASCIISpace(line[start]) {
		start++
	}
	for end > start && isASCIISpace(line[end-1]) {
		end--
	}

	if start == end {
		return nil
	}

	// Optional leading pipe.
	if '|' == line[start] {
		start++
	}

	// Optional trailing pipe — but an escaped one is content, not a delimiter.
	if start < end && '|' == line[end-1] {
		bs := end - 2
		for start <= bs && '\\' == line[bs] {
			bs--
		}
		// An even-length backslash run leaves the pipe unescaped.
		if 0 == (end-2-bs)%2 {
			end--
		}
	}

	cells := make([]string, 0, 4)
	// `cur` holds the parts of the current cell that have already been
	// rewritten; `seg` is the start of the run since then, so the common case
	// (no escapes) costs one slice per cell rather than one per byte. A
	// strings.Builder rather than the TypeScript's `+=`, since Go has no rope.
	var cur strings.Builder
	seg := start
	i := start

	for i < end {
		c := line[i]
		if '\\' == c && i+1 < end {
			if '|' == line[i+1] {
				cur.WriteString(line[seg:i])
				cur.WriteByte('|')
				i += 2
				seg = i
			} else {
				i += 2
			}
			continue
		}
		if '|' == c {
			cells = append(cells, trimASCIISpace(joinCell(&cur, line[seg:i])))
			cur.Reset()
			i++
			seg = i
			continue
		}
		i++
	}
	cells = append(cells, trimASCIISpace(joinCell(&cur, line[seg:end])))

	return cells
}

// joinCell is `cur + tail` where cur is usually empty — the no-escapes case,
// which must not copy the tail.
func joinCell(cur *strings.Builder, tail string) string {
	if 0 == cur.Len() {
		return tail
	}
	cur.WriteString(tail)
	return cur.String()
}

// reTableDelimiterCell is the TypeScript's RE_TABLE_DELIMITER_CELL,
// /^(:?)-+(:?)$/: a delimiter cell is hyphens with an optional leading and/or
// trailing colon, and nothing else. One `+`, one space in the middle, one
// stray character and the line is not a delimiter row.
//
// A plain RE2 pattern — no lookahead, no input-derived repeat count — so
// unlike the fence matchers above it needs no hand-coding. Go's `^`/`$` match
// at the ends of the text only, which is what the JavaScript literal means too.
var reTableDelimiterCell = regexp.MustCompile(`^(:?)-+(:?)$`)

// parseDelimiterRow reads a line as a table's delimiter row, returning one
// alignment per column, or nil if it is not one.
func parseDelimiterRow(line string) []TableAlign {
	cells := splitTableRow(line)
	if 0 == len(cells) {
		return nil
	}

	align := make([]TableAlign, 0, len(cells))
	for _, cell := range cells {
		m := reTableDelimiterCell.FindStringSubmatch(cell)
		if nil == m {
			return nil
		}
		left := ":" == m[1]
		right := ":" == m[2]
		switch {
		case left && right:
			align = append(align, AlignCenter)
		case left:
			align = append(align, AlignLeft)
		case right:
			align = append(align, AlignRight)
		default:
			align = append(align, AlignNone)
		}
	}
	return align
}

// maxAutocompletedCells caps the empty cells inserted to pad short body rows,
// across a whole document — cmark-gfm's MAX_AUTOCOMPLETED_CELLS, and for the
// same reason.
//
// Padding is the one place where a table's node count is not bounded by the
// size of its source: a 10000-column header followed by 10000 one-cell rows is
// 60 KB of input asking for 10^8 cells. Every other part of the extension is
// linear in the input, so this single budget is what keeps the whole of it so.
// Reaching it needs input that is already pathological; ordinary tables are
// nowhere near.
const maxAutocompletedCells = 0x80000

// §5.3: two markers make the same list only if type, bullet and delimiter agree.
func listsMatch(listData *ListData, itemData *ListData) bool {
	if listData == nil || itemData == nil {
		return false
	}
	return listData.Type == itemData.Type &&
		listData.Delimiter == itemData.Delimiter &&
		listData.BulletChar == itemData.BulletChar
}

// endsWithBlankLine answers the §5.3 looseness input: does this block end with
// a blank line? For lists and items the question recurses into the last child,
// since the blank line is recorded on the deepest block that saw it. The
// LastLineChecked flag makes the recursion linear over the whole document
// rather than quadratic.
func endsWithBlankLine(block *MdNode) bool {
	for cur := block; cur != nil; {
		if cur.LastLineBlank {
			return true
		}
		t := cur.Type
		if !cur.LastLineChecked && (NodeList == t || NodeItem == t) {
			cur.LastLineChecked = true
			cur = cur.LastChild
		} else {
			cur.LastLineChecked = true
			return false
		}
	}
	return false
}

// --- per-block behaviour ----------------------------------------------------

// continueResult: 0 = matched, 1 = not matched, 2 = line fully consumed
// (fenced code close), stop processing this line entirely.
type continueResult int

const (
	continueMatched    continueResult = 0
	continueNotMatched continueResult = 1
	continueLineDone   continueResult = 2
)

type blockHandler struct {
	// continueBlock: can this block absorb the current line? Named `continue`
	// in the TypeScript, which is a keyword here.
	continueBlock func(p *blockParser, block *MdNode) continueResult
	// finalize is called once when the block is closed.
	finalize   func(p *blockParser, block *MdNode)
	canContain func(t NodeType) bool
	// acceptsLines marks the leaves that accumulate raw text: code blocks,
	// HTML blocks, paragraphs, tables.
	acceptsLines bool
	// verbatim marks the blocks whose lines are literal text, so Appendix A
	// step 2 does not look for block starts while one is the last matched
	// container.
	//
	// True for exactly the two blocks the spec names — code blocks and HTML
	// blocks. Paragraphs and GFM tables also accept lines, but a block start
	// may still interrupt either: that is how `> bar` ends a table, and how
	// `# h` ends a paragraph.
	verbatim bool
}

func notItem(t NodeType) bool { return NodeItem != t }

func noChildren(NodeType) bool { return false }

// blockHandlers is the TypeScript's BLOCK_HANDLERS record. It is filled in
// init rather than in the declaration because the handlers reach the parser's
// finalize method, which reaches handlerFor, which reads this variable — a
// static initialisation cycle Go rejects.
var blockHandlers map[NodeType]*blockHandler

func init() {
	blockHandlers = map[NodeType]*blockHandler{
		NodeDocument: {
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueMatched },
			finalize:      func(*blockParser, *MdNode) {},
			canContain:    notItem,
			acceptsLines:  false,
			verbatim:      false,
		},

		NodeList: {
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueMatched },
			finalize: func(_ *blockParser, block *MdNode) {
				// §5.3: the list is loose if any item is followed by a blank
				// line, or if any item directly contains two blocks separated
				// by one. A blank line at the very end of the last item does
				// not count — hence the item.Next / subitem.Next guards.
				for item := block.FirstChild; item != nil; item = item.Next {
					if endsWithBlankLine(item) && item.Next != nil {
						if block.ListData != nil {
							block.ListData.Tight = false
						}
						break
					}
					for subitem := item.FirstChild; subitem != nil; subitem = subitem.Next {
						if endsWithBlankLine(subitem) && (item.Next != nil || subitem.Next != nil) {
							if block.ListData != nil {
								block.ListData.Tight = false
							}
							break
						}
					}
				}
				if block.LastChild != nil {
					block.SourcePos[1] = block.LastChild.SourcePos[1]
				}
			},
			canContain:   func(t NodeType) bool { return NodeItem == t },
			acceptsLines: false,
			verbatim:     false,
		},

		NodeBlockQuote: {
			continueBlock: func(p *blockParser, _ *MdNode) continueResult {
				ln := p.currentLine
				if !p.indented && '>' == peek(ln, p.nextNonspace) {
					p.advanceNextNonspace()
					p.advanceOffset(1, false)
					// §5.1: one optional space of indentation after `>`, tab-aware.
					if isSpaceOrTab(peek(ln, p.offset)) {
						p.advanceOffset(1, true)
					}
					return continueMatched
				}
				return continueNotMatched
			},
			finalize:     func(*blockParser, *MdNode) {},
			canContain:   notItem,
			acceptsLines: false,
			verbatim:     false,
		},

		NodeItem: {
			continueBlock: func(p *blockParser, container *MdNode) continueResult {
				data := container.ListData
				if nil == data {
					return continueNotMatched
				}

				if p.blank {
					// An item whose first line is blank cannot be continued by
					// a second blank line (§5.2 rule 3: at most one leading
					// blank line).
					if nil == container.FirstChild {
						return continueNotMatched
					}
					p.advanceNextNonspace()
					return continueMatched
				}

				// §5.2: subsequent lines belong to the item if indented to the
				// content column derived from the marker.
				if p.indent >= data.MarkerOffset+data.Padding {
					p.advanceOffset(data.MarkerOffset+data.Padding, true)
					return continueMatched
				}
				return continueNotMatched
			},
			finalize: func(_ *blockParser, block *MdNode) {
				if block.LastChild != nil {
					block.SourcePos[1] = block.LastChild.SourcePos[1]
				} else if block.ListData != nil {
					block.SourcePos[1] = [2]int{
						block.SourcePos[0][0],
						block.ListData.MarkerOffset + block.ListData.Padding,
					}
				}
			},
			canContain:   notItem,
			acceptsLines: false,
			verbatim:     false,
		},

		NodeHeading: {
			// A heading is always exactly one line (setext headings are
			// converted from an already-complete paragraph).
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueNotMatched },
			finalize:      func(*blockParser, *MdNode) {},
			canContain:    noChildren,
			acceptsLines:  false,
			verbatim:      false,
		},

		NodeThematicBreak: {
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueNotMatched },
			finalize:      func(*blockParser, *MdNode) {},
			canContain:    noChildren,
			acceptsLines:  false,
			verbatim:      false,
		},

		NodeCodeBlock: {
			continueBlock: func(p *blockParser, container *MdNode) continueResult {
				ln := p.currentLine
				indent := p.indent

				if container.IsFenced {
					// §4.5: the closing fence is the same character, at least
					// as long, at most 3 columns indented, and followed only by
					// spaces/tabs.
					matchLen := 0
					if indent <= 3 && p.nextNonspace < len(ln) && ln[p.nextNonspace] == container.FenceChar {
						matchLen = matchClosingCodeFence(ln[p.nextNonspace:])
					}
					if matchLen > 0 && matchLen >= container.FenceLength {
						p.lastLineLength = charCount(ln[:p.offset]) + indent + matchLen
						p.finalize(container, p.lineNumber)
						return continueLineDone
					}
					// Content is de-indented by up to the opening fence's own indent.
					i := container.FenceOffset
					for 0 < i && isSpaceOrTab(peek(ln, p.offset)) {
						p.advanceOffset(1, true)
						i--
					}
					return continueMatched
				}

				// §4.4 indented code: four columns, or a blank line (which is kept).
				if indent >= codeIndent {
					p.advanceOffset(codeIndent, true)
					return continueMatched
				}
				if p.blank {
					p.advanceNextNonspace()
					return continueMatched
				}
				return continueNotMatched
			},
			finalize: func(_ *blockParser, block *MdNode) {
				if block.IsFenced {
					// The first accumulated line is the info string, not content.
					content := block.StringContent
					nl := bytes.IndexByte(content, '\n')
					firstLine := content
					if -1 != nl {
						firstLine = content[:nl]
					}
					block.Info = unescapeString(jsTrim(string(firstLine)))
					block.HasInfo = true
					if -1 == nl {
						block.Literal = ""
					} else {
						block.Literal = string(content[nl+1:])
					}
				} else {
					// §4.4: trailing blank lines are not part of an indented
					// code block. This is .replace(/(\n *)+$/, '\n').
					content := block.StringContent
					if start := trailingBlankRunStart(content); start < len(content) {
						block.Literal = string(content[:start]) + "\n"
					} else {
						block.Literal = string(content)
					}
				}
				block.StringContent = nil
			},
			canContain:   noChildren,
			acceptsLines: true,
			verbatim:     true,
		},

		NodeHTMLBlock: {
			continueBlock: func(p *blockParser, container *MdNode) continueResult {
				// Types 6 and 7 end at a blank line; 1–5 end on their closing
				// string, which incorporateLine checks after adding the line.
				t := p.htmlBlockType(container)
				if p.blank && (6 == t || 7 == t) {
					return continueNotMatched
				}
				return continueMatched
			},
			finalize: func(_ *blockParser, block *MdNode) {
				// .replace(/(\n *)+$/, '')
				content := block.StringContent
				block.Literal = string(content[:trailingBlankRunStart(content)])
				block.StringContent = nil
			},
			canContain:   noChildren,
			acceptsLines: true,
			verbatim:     true,
		},

		NodeParagraph: {
			continueBlock: func(p *blockParser, _ *MdNode) continueResult {
				if p.blank {
					return continueNotMatched
				}
				return continueMatched
			},
			finalize: func(p *blockParser, block *MdNode) {
				// §4.7: a paragraph may begin with link reference definitions.
				// Consume them from the front; if nothing is left, the
				// paragraph disappears.
				if p.consumeReferenceDefs(block) && isBlank(string(block.StringContent)) {
					block.Unlink()
				}
			},
			canContain:   noChildren,
			acceptsLines: true,
			verbatim:     false,
		},

		// GFM tables. The header row is built when the table opens (see
		// tryOpenTable); every later line is accumulated raw and split into a
		// body row at finalize, so the table behaves like a paragraph that
		// happens to print as a grid — it ends at a blank line, and any block
		// start interrupts it, because verbatim is false and canContain refuses
		// everything the block starts can open.
		NodeTable: {
			continueBlock: func(p *blockParser, _ *MdNode) continueResult {
				if p.blank {
					return continueNotMatched
				}
				return continueMatched
			},
			finalize:     finalizeTable,
			canContain:   func(t NodeType) bool { return NodeTableRow == t },
			acceptsLines: true,
			verbatim:     false,
		},

		// Rows and cells are created closed, and are never the parser's tip, so
		// nothing here is reached in a normal parse. They exist so that
		// handlerFor — which falls back to an inert leaf — cannot be the way a
		// malformed tree surfaces.
		NodeTableRow: {
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueNotMatched },
			finalize:      func(*blockParser, *MdNode) {},
			canContain:    func(t NodeType) bool { return NodeTableCell == t },
			acceptsLines:  false,
			verbatim:      false,
		},

		NodeTableCell: {
			continueBlock: func(*blockParser, *MdNode) continueResult { return continueNotMatched },
			finalize:      func(*blockParser, *MdNode) {},
			canContain:    noChildren,
			acceptsLines:  false,
			verbatim:      false,
		},
	}
}

// addTableRow appends one row of `cells` to `table`, padded with empty cells or
// truncated so that it is exactly as wide as the delimiter row said.
//
// Rows and cells are built closed: they are not part of the open spine, and the
// line walk in incorporateLine must stop at the table itself.
func addTableRow(
	p *blockParser,
	table *MdNode,
	cells []string,
	isHeader bool,
	lineNumber int,
	endColumn int,
) {
	width := len(table.TableAlign)

	// See maxAutocompletedCells: padding is the only unbounded part of a table.
	limit := width
	if len(cells) < width {
		budget := maxAutocompletedCells - p.autocompletedCells
		padding := width - len(cells)
		if padding > budget {
			limit = len(cells)
			if 0 < budget {
				limit += budget
			}
		}
		p.autocompletedCells += limit - len(cells)
	}

	// A row and its cells occupy one source line. Columns within that line are
	// not tracked: nothing reads them (the AST drops sourcepos and the renderer
	// ignores it), and threading offsets through the cell split to invent them
	// would be cost with no reader. SourcePos is an array value here, so every
	// assignment copies — the TypeScript has to rebuild the span per node
	// because its sourcepos is a shared mutable array.
	span := SourcePos{{lineNumber, 1}, {lineNumber, endColumn}}

	row := NewNode(NodeTableRow)
	row.SourcePos = span
	row.IsHeaderRow = isHeader
	row.Open = false
	table.AppendChild(row)

	for i := 0; i < limit; i++ {
		cell := NewNode(NodeTableCell)
		cell.SourcePos = span
		cell.Open = false
		// Consumed by the inline phase, exactly as a paragraph's content is.
		if i < len(cells) {
			cell.StringContent = []byte(cells[i])
		}
		row.AppendChild(cell)
	}
}

// finalizeTable closes a table: it turns the raw lines the table accumulated
// into body rows.
//
// The first accumulated line is the delimiter row that opened the table. It
// carries no data — its alignments are already on the node — so it is skipped
// here, which is also what keeps the source line numbering of the rows below it
// straightforward: the table accepts exactly one line per source line, and a
// blank line closes it, so there are no gaps.
func finalizeTable(p *blockParser, table *MdNode) {
	lines := bytes.Split(table.StringContent, []byte{'\n'})
	table.StringContent = nil

	// Line 0 is the delimiter row; the last element is the empty string after
	// the final newline.
	headerLine := table.SourcePos[0][0]
	for i := 1; i < len(lines); i++ {
		if 0 == len(lines[i]) {
			continue
		}
		line := string(lines[i])
		addTableRow(p, table, splitTableRow(line), false, headerLine+i+1, charCount(line))
	}
}

// missingHandler stands in for a node type with no block behaviour. The
// TypeScript throws; Go must not panic on any input, and an inert leaf is the
// safe reading — no continuation, no children, no lines.
var missingHandler = &blockHandler{
	continueBlock: func(*blockParser, *MdNode) continueResult { return continueNotMatched },
	finalize:      func(*blockParser, *MdNode) {},
	canContain:    noChildren,
	acceptsLines:  false,
	verbatim:      false,
}

func handlerFor(t NodeType) *blockHandler {
	if handler, ok := blockHandlers[t]; ok {
		return handler
	}
	return missingHandler
}

// --- list markers (§5.2) ----------------------------------------------------

// parseListMarker parses a list marker at nextNonspace and derives the item's
// content indent.
//
// The content indent is marker width + the spaces that follow it, with two
// exceptions that both fall back to marker width + 1:
//
//   - rule 2, item starting with indented code: 5+ spaces after the marker
//     means only the first space belongs to the marker, the rest is code;
//   - rule 3, item starting with a blank line: nothing follows the marker at
//     all, so there are no spaces to measure.
//
// Returns nil when this is not a list item start. Advances the parser to the
// item's content column when it is.
func parseListMarker(p *blockParser, container *MdNode) *ListData {
	// Four columns of indentation is indented code, never a list marker.
	if p.indent >= codeIndent {
		return nil
	}

	rest := p.currentLine[p.nextNonspace:]

	data := &ListData{
		Type:         ListBullet,
		Tight:        true, // lists are tight until finalize proves otherwise
		Start:        1,
		Delimiter:    "",
		BulletChar:   "",
		Padding:      0,
		MarkerOffset: p.indent,
	}

	markerLen := 0
	if isBulletListMarker(rest) {
		data.Type = ListBullet
		data.BulletChar = string(rest[0])
		markerLen = 1
	} else {
		digits, delimiter := matchOrderedListMarker(rest)
		if 0 == delimiter {
			return nil
		}
		start, err := strconv.Atoi(rest[:digits])
		if err != nil {
			// Unreachable: at most nine digits, which always fit.
			return nil
		}
		// §5.2 exception 1(b): an ordered item interrupting a paragraph must
		// start at 1.
		if NodeParagraph == container.Type && 1 != start {
			return nil
		}
		data.Type = ListOrdered
		data.Start = start
		data.Delimiter = string(delimiter)
		markerLen = digits + 1
	}

	// At least one space or tab (or the end of the line) must follow the marker.
	nextc := peek(p.currentLine, p.nextNonspace+markerLen)
	if !(-1 == nextc || '\t' == nextc || ' ' == nextc) {
		return nil
	}

	// §5.2 exception 1(a): an item interrupting a paragraph may not start blank.
	if NodeParagraph == container.Type && isBlank(p.currentLine[p.nextNonspace+markerLen:]) {
		return nil
	}

	p.advanceNextNonspace()          // to the marker
	p.advanceOffset(markerLen, true) // past the marker

	spacesStartCol := p.column
	spacesStartOffset := p.offset
	for {
		p.advanceOffset(1, true)
		nextc = peek(p.currentLine, p.offset)
		if !(p.column-spacesStartCol < 5 && isSpaceOrTab(nextc)) {
			break
		}
	}

	blankItem := -1 == peek(p.currentLine, p.offset)
	spacesAfterMarker := p.column - spacesStartCol

	if spacesAfterMarker >= 5 || spacesAfterMarker < 1 || blankItem {
		data.Padding = markerLen + 1
		p.column = spacesStartCol
		p.offset = spacesStartOffset
		if isSpaceOrTab(peek(p.currentLine, p.offset)) {
			p.advanceOffset(1, true)
		}
	} else {
		data.Padding = markerLen + spacesAfterMarker
	}

	return data
}

// --- block starts -----------------------------------------------------------

// startResult: 0 = no match, 1 = container started, 2 = leaf started (stop
// looking).
type startResult int

const (
	startNone      startResult = 0
	startContainer startResult = 1
	startLeaf      startResult = 2
)

type blockStart func(p *blockParser, container *MdNode) startResult

var (
	reATXClosingWholeLine = regexp.MustCompile(`^[ \t]*#+[ \t]*$`)
	reATXClosingSequence  = regexp.MustCompile(`[ \t]+#+[ \t]*$`)
)

// blockStarts are tried in order, and the order is the spec's: a setext
// underline beats a thematic break (so `Foo\n---` is a heading), a thematic
// break beats a list item (so `- - -` is a break), and indented code comes last
// because any of the others may be indented up to three columns.
var blockStarts = []blockStart{
	// Block quote (§5.1).
	func(p *blockParser, _ *MdNode) startResult {
		if !p.indented && '>' == peek(p.currentLine, p.nextNonspace) {
			p.advanceNextNonspace()
			p.advanceOffset(1, false)
			if isSpaceOrTab(peek(p.currentLine, p.offset)) {
				p.advanceOffset(1, true)
			}
			p.closeUnmatchedBlocks()
			p.addChild(NodeBlockQuote, p.nextNonspace)
			return startContainer
		}
		return startNone
	},

	// ATX heading (§4.2).
	func(p *blockParser, _ *MdNode) startResult {
		if p.indented {
			return startNone
		}
		level, matchLen := matchATXHeadingMarker(p.currentLine[p.nextNonspace:])
		if 0 == matchLen {
			return startNone
		}

		p.advanceNextNonspace()
		p.advanceOffset(matchLen, false)
		p.closeUnmatchedBlocks()

		container := p.addChild(NodeHeading, p.nextNonspace)
		container.Level = level
		// An optional closing sequence of `#`s, preceded by spaces or tabs, is
		// dropped; a line of nothing but `#`s has empty content.
		content := p.currentLine[p.offset:]
		if reATXClosingWholeLine.MatchString(content) {
			content = ""
		} else {
			content = reATXClosingSequence.ReplaceAllString(content, "")
		}
		container.StringContent = []byte(content)
		p.advanceOffset(len(p.currentLine)-p.offset, false)
		return startLeaf
	},

	// Fenced code block (§4.5).
	func(p *blockParser, _ *MdNode) startResult {
		if p.indented {
			return startNone
		}
		rest := p.currentLine[p.nextNonspace:]
		fenceLength := matchCodeFence(rest)
		if 0 == fenceLength {
			return startNone
		}

		p.closeUnmatchedBlocks()
		container := p.addChild(NodeCodeBlock, p.nextNonspace)
		container.IsFenced = true
		container.FenceLength = fenceLength
		container.FenceChar = rest[0]
		container.FenceOffset = p.indent
		p.advanceNextNonspace()
		p.advanceOffset(fenceLength, false)
		// The rest of the line is added as the block's first line: the info string.
		return startLeaf
	},

	// HTML block (§4.6), types 1-7.
	func(p *blockParser, container *MdNode) startResult {
		if p.indented || '<' != peek(p.currentLine, p.nextNonspace) {
			return startNone
		}

		s := p.currentLine[p.nextNonspace:]
		for blockType := 1; blockType <= 7; blockType++ {
			if !reHTMLBlockOpen[blockType].MatchString(s) {
				continue
			}
			// Type 7 may not interrupt a paragraph — including a paragraph we
			// are only about to continue lazily.
			if 7 == blockType &&
				(NodeParagraph == container.Type ||
					(!p.allClosed && !p.blank && NodeParagraph == p.openTip().Type)) {
				continue
			}
			p.closeUnmatchedBlocks()
			// The offset is deliberately not advanced: leading spaces are part
			// of the raw HTML.
			block := p.addChild(NodeHTMLBlock, p.offset)
			p.setHtmlBlockType(block, blockType)
			return startLeaf
		}
		return startNone
	},

	// Setext heading underline (§4.3) — converts the whole open paragraph.
	func(p *blockParser, container *MdNode) startResult {
		if p.indented || NodeParagraph != container.Type {
			return startNone
		}
		underline := matchSetextHeadingLine(p.currentLine[p.nextNonspace:])
		if 0 == underline {
			return startNone
		}

		p.closeUnmatchedBlocks()

		// §4.7: the paragraph may still have led with reference definitions;
		// only what remains becomes the heading. If nothing remains this is not
		// a setext underline at all (it falls through to thematic break /
		// paragraph).
		p.consumeReferenceDefs(container)
		if 0 == len(container.StringContent) {
			return startNone
		}

		start := container.SourcePos[0]
		heading := NewNode(NodeHeading)
		heading.SourcePos = SourcePos{{start[0], start[1]}, {0, 0}}
		if '=' == underline {
			heading.Level = 1
		} else {
			heading.Level = 2
		}
		// Safe to share the backing array: the paragraph is unlinked here and
		// never accumulates another line.
		heading.StringContent = container.StringContent
		container.InsertAfter(heading)
		container.Unlink()
		p.tip = heading
		p.advanceOffset(len(p.currentLine)-p.offset, false)
		return startLeaf
	},

	// Thematic break (§4.1).
	func(p *blockParser, _ *MdNode) startResult {
		if p.indented {
			return startNone
		}
		if !isThematicBreak(p.currentLine[p.nextNonspace:]) {
			return startNone
		}
		p.closeUnmatchedBlocks()
		p.addChild(NodeThematicBreak, p.nextNonspace)
		p.advanceOffset(len(p.currentLine)-p.offset, false)
		return startLeaf
	},

	// List item (§5.2). parseListMarker rejects a marker indented four or more
	// columns on its own, so there is no `indented` test here.
	func(p *blockParser, container *MdNode) startResult {
		data := parseListMarker(p, container)
		if nil == data {
			return startNone
		}

		p.closeUnmatchedBlocks()

		// §5.3: a change of bullet character or ordered delimiter starts a new list.
		tip := p.openTip()
		if NodeList != tip.Type || nil == tip.ListData || !listsMatch(tip.ListData, data) {
			p.addChild(NodeList, p.nextNonspace).ListData = data
		}

		p.addChild(NodeItem, p.nextNonspace).ListData = data
		return startContainer
	},

	// Indented code block (§4.4) — may not interrupt a paragraph.
	func(p *blockParser, _ *MdNode) startResult {
		if !p.indented || NodeParagraph == p.openTip().Type || p.blank {
			return startNone
		}
		p.advanceOffset(codeIndent, true)
		p.closeUnmatchedBlocks()
		p.addChild(NodeCodeBlock, p.offset)
		return startLeaf
	},

	// GFM table (extension) — a delimiter row directly under an open paragraph.
	//
	// Last, and the position is load-bearing in both directions. It must come
	// after the setext underline so that `foo` over `---` stays an <h2> rather
	// than becoming a one-column table, and after the list marker so that
	// `- | -` stays a list item; cmark-gfm reaches its extensions from the same
	// place, once every built-in start has refused the line.
	func(p *blockParser, container *MdNode) startResult {
		if p.tryOpenTable(container) {
			return startLeaf
		}
		return startNone
	},
}

// --- the parser -------------------------------------------------------------

type blockParser struct {
	options Options
	refmap  RefMap

	doc *MdNode
	// tip is the deepest open block; nil only once the document itself is
	// finalized.
	tip *MdNode
	// oldtip is the tip as it was before the current line was walked.
	oldtip               *MdNode
	lastMatchedContainer *MdNode
	// allClosed is false while some open block failed its continuation condition.
	allClosed bool

	currentLine    string
	lineNumber     int
	lastLineLength int

	// offset is a byte index into currentLine (the TypeScript's is a UTF-16
	// index; see the file header).
	offset int
	// column is the tab-expanded display column matching offset (§2.2),
	// counted in characters.
	column int
	// partiallyConsumedTab is true when offset sits inside a tab whose columns
	// were partly consumed.
	partiallyConsumedTab bool

	// colAnchorOffset/colAnchorChars memoize the last byte offset in
	// currentLine whose character count is known, so sourcepos columns cost
	// amortized O(1) rather than a fresh count from the start of the line. Both
	// reset to 0 with each new line. See sourceColumn.
	colAnchorOffset int
	colAnchorChars  int

	nextNonspace       int
	nextNonspaceColumn int
	// indent is the columns between column and the next non-space character.
	indent   int
	indented bool
	blank    bool

	// autocompletedCells counts the empty cells inserted so far to pad short
	// table rows. See maxAutocompletedCells.
	autocompletedCells int

	// lastAddedLine is the text addLine last appended, and lastAddedTo the
	// block it went to.
	//
	// tryOpenTable needs the *last line* of an open paragraph, and reading it
	// back out of StringContent is not affordable in the canonical runtime:
	// that content is built by repeated `+=`, so every index into it flattens a
	// fresh rope — O(n) per line, O(n²) over a paragraph whose every other line
	// looks like a delimiter row, which untrusted input can write. Go appends
	// to a []byte and would not pay that, but the field is carried across
	// anyway: it is what makes the two runtimes the same function, and the
	// alternative here — scanning back for the previous '\n' — is its own O(n)
	// per line, with the same quadratic shape.
	//
	// The node is recorded alongside it because "the previous line went to this
	// very paragraph" is the extension's own rule, not a cache-validity trick:
	// a header row is by definition the line immediately above the delimiter
	// row. If some other block took that line, there is no header row.
	lastAddedLine string
	lastAddedTo   *MdNode

	// maybeSpecial is step 2's fast reject, widened for a table's delimiter row
	// when GFM is on. A field rather than a branch at the call site so that a
	// GFM-off parse runs exactly the function it always did.
	maybeSpecial func(ln string, pos int) bool

	// MdNode has no field for the HTML block type, and the tree contract is not
	// ours to change, so open HTML blocks carry it here.
	htmlBlockTypes map[*MdNode]int
}

func newBlockParser(options Options) *blockParser {
	doc := NewNode(NodeDocument)
	doc.SourcePos = SourcePos{{1, 1}, {0, 0}}
	fastReject := maybeSpecial
	if options.GFM {
		fastReject = maybeSpecialGFM
	}
	return &blockParser{
		options:              options,
		refmap:               RefMap{},
		doc:                  doc,
		tip:                  doc,
		oldtip:               doc,
		lastMatchedContainer: doc,
		allClosed:            true,
		maybeSpecial:         fastReject,
		htmlBlockTypes:       map[*MdNode]int{},
	}
}

// openTip is the deepest open block. Only meaningful while a line is being
// processed. The TypeScript throws when the tip is null; a nil tip means the
// document itself has been closed, which cannot happen mid-line, and Go must
// not panic on any input — so the document stands in.
func (p *blockParser) openTip() *MdNode {
	if nil == p.tip {
		return p.doc
	}
	return p.tip
}

func (p *blockParser) htmlBlockType(block *MdNode) int {
	return p.htmlBlockTypes[block]
}

func (p *blockParser) setHtmlBlockType(block *MdNode, blockType int) {
	p.htmlBlockTypes[block] = blockType
}

// advanceOffset advances `count` characters (columns false) or `count` columns
// (columns true), expanding tabs to the next 4-column tab stop (§2.2). A tab
// whose columns are only partly consumed leaves offset on the tab itself and
// sets partiallyConsumedTab, so addLine can re-emit the remainder as spaces.
func (p *blockParser) advanceOffset(count int, columns bool) {
	ln := p.currentLine
	for 0 < count && p.offset < len(ln) {
		if '\t' == ln[p.offset] {
			charsToTab := 4 - (p.column % 4)
			if columns {
				p.partiallyConsumedTab = charsToTab > count
				charsToAdvance := charsToTab
				if charsToTab > count {
					charsToAdvance = count
				}
				p.column += charsToAdvance
				if !p.partiallyConsumedTab {
					p.offset += 1
				}
				count -= charsToAdvance
			} else {
				p.partiallyConsumedTab = false
				p.column += charsToTab
				p.offset += 1
				count -= 1
			}
		} else {
			p.partiallyConsumedTab = false
			// One character, not one byte: offset is a byte index but column
			// counts characters. Block markers are all ASCII, so the two only
			// part company on the advance-to-end-of-line calls.
			_, size := utf8.DecodeRuneInString(ln[p.offset:])
			p.offset += size
			p.column += 1
			count -= 1
		}
	}
}

func (p *blockParser) advanceNextNonspace() {
	p.offset = p.nextNonspace
	p.column = p.nextNonspaceColumn
	p.partiallyConsumedTab = false
}

// findNextNonspace locates the next non-space character and the indent leading
// up to it.
func (p *blockParser) findNextNonspace() {
	ln := p.currentLine
	i := p.offset
	cols := p.column

	for i < len(ln) {
		if ' ' == ln[i] {
			i++
			cols++
		} else if '\t' == ln[i] {
			i++
			cols += 4 - (cols % 4)
		} else {
			break
		}
	}

	p.blank = i >= len(ln) || '\n' == ln[i] || '\r' == ln[i]
	p.nextNonspace = i
	p.nextNonspaceColumn = cols
	p.indent = p.nextNonspaceColumn - p.column
	p.indented = p.indent >= codeIndent
}

// addLine adds the rest of the current line to the tip's raw content.
func (p *blockParser) addLine() {
	tip := p.openTip()
	var text string
	if p.partiallyConsumedTab {
		p.offset += 1 // step over the tab
		charsToTab := 4 - (p.column % 4)
		text = strings.Repeat(" ", charsToTab) + p.currentLine[p.offset:]
	} else {
		text = p.currentLine[p.offset:]
	}
	tip.StringContent = append(tip.StringContent, text...)
	tip.StringContent = append(tip.StringContent, '\n')
	p.lastAddedLine = text
	p.lastAddedTo = tip
}

// addChild opens a block of `tag` as a child of the tip, closing blocks that
// cannot contain it first.
func (p *blockParser) addChild(tag NodeType, offset int) *MdNode {
	// The document accepts everything but an item, and the list-item start
	// always opens a list first, so this terminates at the document at the
	// latest. Stopping there rather than closing the document keeps the parser
	// total where the TypeScript would throw on the next openTip.
	for tip := p.openTip(); tip != p.doc && !handlerFor(tip.Type).canContain(tag); tip = p.openTip() {
		p.finalize(tip, p.lineNumber-1)
	}

	columnNumber := p.sourceColumn(offset) + 1 // offset 0 is column 1
	node := NewNode(tag)
	node.SourcePos = SourcePos{{p.lineNumber, columnNumber}, {0, 0}}
	p.openTip().AppendChild(node)
	p.tip = node
	return node
}

// tryOpenTable is the GFM tables block start: read the current line as the
// delimiter row of a table whose header row is the last line of the open
// paragraph `container`.
//
// The two rows must agree on the number of cells; if they do not, there is no
// table and the line is nothing special (spec example 6 keeps the whole thing a
// paragraph). The header row is the paragraph's **last** line only, so a
// paragraph that had earlier lines is split: those lines stay a paragraph —
// finalized here, which is what still lets them contribute link reference
// definitions — and the table is opened after it.
//
// On success the table is the tip and the caller returns "leaf started", so
// step 3 adds the delimiter row itself to the table's raw content. It is
// skipped again by finalizeTable; keeping it costs nothing and makes the
// table's accumulated lines line up one-for-one with the source lines.
func (p *blockParser) tryOpenTable(container *MdNode) bool {
	if !p.options.GFM || p.indented || NodeParagraph != container.Type {
		return false
	}

	align := parseDelimiterRow(p.currentLine[p.nextNonspace:])
	if nil == align {
		return false
	}

	// The header row is the line directly above, which is the line addLine
	// last wrote — to this paragraph, or there is no header row at all.
	if p.lastAddedTo != container {
		return false
	}
	headerCells := splitTableRow(p.lastAddedLine)
	if len(headerCells) != len(align) {
		return false
	}

	p.closeUnmatchedBlocks()

	headerLine := p.lineNumber - 1
	// StringContent ends with that same line plus the newline addLine put after
	// it, so the header row starts this far in — no scan needed, and the
	// content is only ever touched on the split branch, at most once per table.
	content := container.StringContent
	headerStart := len(content) - len(p.lastAddedLine) - 1
	if 0 < headerStart {
		// Split: everything above the header row stays a paragraph.
		container.StringContent = content[:headerStart]
		p.finalize(container, headerLine-1)
	} else {
		parent := container.Parent
		container.Unlink()
		p.tip = parent
	}

	table := p.addChild(NodeTable, p.nextNonspace)
	// The table begins at the header row, one line above the delimiter row that
	// addChild dated it from.
	table.SourcePos[0][0] = headerLine
	table.TableAlign = align

	addTableRow(p, table, headerCells, true, headerLine, p.lastLineLength)
	return true
}

// closeUnmatchedBlocks closes every block that failed its continuation
// condition on this line.
func (p *blockParser) closeUnmatchedBlocks() {
	if p.allClosed {
		return
	}
	for p.oldtip != p.lastMatchedContainer {
		parent := p.oldtip.Parent
		p.finalize(p.oldtip, p.lineNumber-1)
		if nil == parent {
			break
		}
		p.oldtip = parent
	}
	p.allClosed = true
}

// finalize closes a block, records its end position, and runs its finalizer.
func (p *blockParser) finalize(block *MdNode, lineNumber int) {
	above := block.Parent
	block.Open = false
	block.SourcePos[1] = [2]int{lineNumber, p.lastLineLength}
	handlerFor(block.Type).finalize(p, block)
	delete(p.htmlBlockTypes, block)
	p.tip = above
}

// consumeReferenceDefs strips leading link reference definitions (§4.7) from a
// block's raw content, adding each to the refmap, and reports whether any were
// consumed. The TypeScript inlines this loop at both call sites; here it is one
// helper so the []byte -> string conversion parseReference needs happens once
// rather than once per definition.
func (p *blockParser) consumeReferenceDefs(block *MdNode) bool {
	s := string(block.StringContent)
	consumed := 0
	for consumed < len(s) && '[' == s[consumed] {
		pos := parseReference(s[consumed:], p.refmap)
		if 0 == pos {
			break
		}
		consumed += pos
	}
	if 0 == consumed {
		return false
	}
	block.StringContent = block.StringContent[consumed:]
	return true
}

// incorporateLine folds one line into the tree — the three steps of Appendix A.
func (p *blockParser) incorporateLine(ln string) {
	allMatched := true
	container := p.doc

	p.oldtip = p.openTip()
	p.offset = 0
	p.column = 0
	p.blank = false
	p.partiallyConsumedTab = false
	p.lineNumber += 1
	p.currentLine = ln
	p.colAnchorOffset = 0
	p.colAnchorChars = 0

	// Step 1: walk the open blocks, consuming continuation markers. Stop at the
	// first block that does not match; `container` is then the last matched one.
	lastChild := container.LastChild
	for lastChild != nil && lastChild.Open {
		container = lastChild
		p.findNextNonspace()

		switch handlerFor(container.Type).continueBlock(p, container) {
		case continueNotMatched:
			allMatched = false
		case continueLineDone:
			// A fenced code block was closed by this line; nothing else to do.
			return
		}

		if !allMatched {
			if parent := container.Parent; parent != nil {
				container = parent
			}
			break
		}

		lastChild = container.LastChild
	}

	p.allClosed = container == p.oldtip
	p.lastMatchedContainer = container

	// Step 2: look for new block starts under the last matched container —
	// unless its lines are literal text, in which case there is nothing to look
	// for.
	matchedLeaf := handlerFor(container.Type).verbatim

	for !matchedLeaf {
		p.findNextNonspace()

		if !p.indented && !p.maybeSpecial(ln, p.nextNonspace) {
			p.advanceNextNonspace()
			break
		}

		i := 0
		for i < len(blockStarts) {
			result := blockStarts[i](p, container)
			if startContainer == result {
				container = p.openTip()
				break
			}
			if startLeaf == result {
				container = p.openTip()
				matchedLeaf = true
				break
			}
			i++
		}

		if i == len(blockStarts) {
			// Nothing matched: the rest of the line is plain text.
			p.advanceNextNonspace()
			break
		}
	}

	// Step 3: whatever is left of the line is text for the deepest open block.
	if !p.allClosed && !p.blank && NodeParagraph == p.openTip().Type {
		// Lazy continuation (§5.1 rule 2): the markers are missing but an open
		// paragraph absorbs the line anyway, and its containers stay open.
		p.addLine()
		p.lastLineLength = charCount(ln)
		return
	}

	p.closeUnmatchedBlocks()

	if p.blank && container.LastChild != nil {
		container.LastChild.LastLineBlank = true
	}

	t := container.Type

	// A block quote line is never blank (it has a `>`), blank lines inside
	// fenced code do not affect list looseness, and an item's own first blank
	// line is part of the item, not a separator.
	lastLineBlank := p.blank &&
		!(NodeBlockQuote == t ||
			(NodeCodeBlock == t && container.IsFenced) ||
			(NodeItem == t &&
				nil == container.FirstChild &&
				container.SourcePos[0][0] == p.lineNumber))

	// Propagate up: a container whose last line was blank ends with a blank line.
	for cont := container; cont != nil; cont = cont.Parent {
		cont.LastLineBlank = lastLineBlank
	}

	if handlerFor(t).acceptsLines {
		p.addLine()
		// §4.6: HTML block types 1-5 end on the line that contains their
		// closing string, which may be the line that opened them.
		if NodeHTMLBlock == t {
			blockType := p.htmlBlockType(container)
			if 1 <= blockType && blockType <= 5 &&
				htmlBlockClose[blockType](p.currentLine[p.offset:]) {
				p.lastLineLength = charCount(ln)
				p.finalize(container, p.lineNumber)
			}
		}
	} else if p.offset < len(ln) && !p.blank {
		// Nowhere else for the text to go: open a paragraph for it.
		p.addChild(NodeParagraph, p.offset)
		p.advanceNextNonspace()
		p.addLine()
	}

	p.lastLineLength = charCount(ln)
}

func (p *blockParser) parse(input string) (*MdNode, RefMap) {
	// §2.3: insecure characters are replaced before anything else looks at them.
	text := input
	if strings.IndexByte(text, 0) >= 0 {
		text = strings.ReplaceAll(text, "\x00", "\uFFFD")
	}

	lines := splitLines(text)
	length := len(lines)
	// A final line ending does not introduce a trailing blank line.
	if 0 < len(text) && "" == lines[length-1] {
		length -= 1
	}

	for i := 0; i < length; i++ {
		p.incorporateLine(lines[i])
	}

	for p.tip != nil {
		p.finalize(p.tip, length)
	}

	p.doc.GFM = p.options.GFM
	if p.options.GFM {
		markTaskListItems(p.doc)
	}

	return p.doc, p.refmap
}

// --- GFM task list items ----------------------------------------------------

// taskListMarker implements the TypeScript's RE_TASK_LIST_MARKER,
// /^ *\[([ \tXx])\][ \t]+/: optional spaces, `[`, either a whitespace
// character or `x`/`X`, `]`, and then at least one whitespace character before
// any other content. It returns the character between the brackets and the
// byte length of the whole marker.
//
// The trailing whitespace is required by the extension ("…and at least one
// whitespace character before any other content"), so `- [x]` with nothing
// after it is an ordinary item whose text is `[x]`. It is matched as a space
// or a tab rather than as any whitespace: a line ending there would mean the
// content starts on the *next* line, and consuming it would swallow the
// paragraph's first soft break.
//
// Hand-coded rather than compiled as a regexp, for the reason the fence
// patterns above are: this runs once per list item, and a match that is only
// ever attempted at offset 0 is the only shape that keeps a document of many
// items linear.
func taskListMarker(s []byte) (state byte, length int, ok bool) {
	i := 0
	for i < len(s) && ' ' == s[i] {
		i++
	}
	if i >= len(s) || '[' != s[i] {
		return 0, 0, false
	}
	i++
	if i >= len(s) {
		return 0, 0, false
	}
	state = s[i]
	if ' ' != state && '\t' != state && 'x' != state && 'X' != state {
		return 0, 0, false
	}
	i++
	if i >= len(s) || ']' != s[i] {
		return 0, 0, false
	}
	i++

	spaces := i
	for i < len(s) && (' ' == s[i] || '\t' == s[i]) {
		i++
	}
	if i == spaces {
		return 0, 0, false
	}
	return state, i, true
}

// taskListContainers is the set of node types markTaskListItems descends into.
var taskListContainers = map[NodeType]bool{
	NodeDocument:   true,
	NodeBlockQuote: true,
	NodeList:       true,
	NodeItem:       true,
}

// markTaskListItems implements GFM task list items: mark every item whose
// first block is a paragraph beginning with a task list item marker, and
// consume the marker so it does not survive into the text.
//
// Run at the end of the block phase, over StringContent — before the inline
// phase has seen it. That is what makes the marker win over anything the
// inline scanner would otherwise make of the brackets: with a `[x]: /url`
// definition in the document, `- [x] foo` is still a task item rather than a
// link. It also means the item's checked state is settled before ast.go or
// html.go looks at the tree.
//
// Iterative rather than recursive: container nesting depth is chosen by the
// input, and strings.Repeat("> ", 20000) is a document this parser otherwise
// handles.
func markTaskListItems(doc *MdNode) {
	stack := []*MdNode{doc}

	for 0 < len(stack) {
		node := stack[len(stack)-1]
		stack = stack[:len(stack)-1]

		if NodeItem == node.Type {
			first := node.FirstChild
			if nil != first && NodeParagraph == first.Type {
				if state, length, ok := taskListMarker(first.StringContent); ok {
					// Whitespace between the brackets is unchecked; `x`/`X` is
					// checked.
					node.Checked = ' ' != state && '\t' != state
					node.HasChecked = true
					first.StringContent = first.StringContent[length:]
				}
			}
		}

		if taskListContainers[node.Type] {
			for c := node.LastChild; nil != c; c = c.Prev {
				stack = append(stack, c)
			}
		}
	}
}

// parseBlocks runs phase 1: build the block tree and collect link reference
// definitions. Paragraph and heading text is left raw in StringContent for
// phase 2.
func parseBlocks(input string, opts Options) (*MdNode, RefMap) {
	return newBlockParser(opts).parse(input)
}
