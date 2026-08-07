/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// HTML renderer for the finished CommonMark tree. Port of ts/src/html.ts —
// keep the two in step.
//
// The conformance suite compares this output to the spec's expected HTML
// byte for byte, so newline placement is a correctness contract here, not a
// formatting preference. The discipline that makes it come out right is the
// reference renderer's:
//
//   - every block writes `cr()` before its opening tag and after its closing
//     tag, and `cr()` is a no-op when the buffer is already at the start of a
//     line;
//   - no block ever emits a newline on behalf of a neighbour or a child.
//
// Because a block only ever asks for "be at line start", nested blocks
// compose without doubling up: `<li>` deliberately does *not* end a line, and
// the newline in `<li>\n<p>…` is the paragraph's own leading `cr()`. A tight
// item, whose paragraph is skipped entirely, therefore renders as
// `<li>text</li>` with no interior newlines at all.
//
// The renderer reads only the finished tree — never StringContent, which the
// inline phase has already consumed.

import (
	"strconv"
	"strings"
)

// attr is an HTML attribute as name + already-escaped value. The TS uses a
// [string, string] tuple, which Go has no equivalent of.
type attr struct {
	name  string
	value string
}

// htmlRenderer holds the output buffer and the one bit of state cr() needs.
// The TS keeps these as closure variables inside renderHTML; Go has no cheap
// way to write nested closures over a strings.Builder, so they become a
// struct with the same three methods hanging off it.
type htmlRenderer struct {
	buf strings.Builder
	// atLineStart is exactly "the buffer is empty or ends with a newline",
	// maintained by lit and tag rather than re-read from the builder (a
	// strings.Builder cannot be indexed). An empty buffer counts as
	// at-line-start, so a leading cr() — every top-level block opens with one
	// — writes nothing.
	atLineStart bool
}

func (r *htmlRenderer) lit(s string) {
	// An empty write must not claim the line is dirty, or the next cr() would
	// insert a spurious newline.
	if "" == s {
		return
	}
	r.buf.WriteString(s)
	// Byte comparison, not rune: '\n' is ASCII, so it can never be a UTF-8
	// continuation byte and a trailing multi-byte rune can never end in it.
	r.atLineStart = '\n' == s[len(s)-1]
}

// cr appends a newline only if the buffer does not already end with one.
func (r *htmlRenderer) cr() {
	if !r.atLineStart {
		r.lit("\n")
	}
}

// tag writes an opening, closing or self-closing tag.
//
// The TS concatenates the whole tag into a string and hands it to lit();
// Go writes the pieces straight into the builder to avoid the intermediate
// allocation, then records the atLineStart that lit() would have derived — a
// tag always ends with '>', so the buffer is never at a line start after one.
func (r *htmlRenderer) tag(name string, attrs []attr, selfClosing bool) {
	r.buf.WriteByte('<')
	r.buf.WriteString(name)
	for _, a := range attrs {
		r.buf.WriteByte(' ')
		r.buf.WriteString(a.name)
		r.buf.WriteString(`="`)
		r.buf.WriteString(a.value)
		r.buf.WriteByte('"')
	}
	if selfClosing {
		r.buf.WriteString(" /")
	}
	r.buf.WriteByte('>')
	r.atLineStart = false
}

// --- GFM disallowed raw HTML (tagfilter) ------------------------------------

// disallowedTags is GFM's tagfilter set. These nine tags change how the *rest*
// of a document is interpreted (everything after <xmp> or <plaintext> stops
// being markup at all), so GFM neutralises them by rewriting the leading `<`
// as `&lt;` — opening and closing, in any case. Every other tag still passes
// through verbatim, as CommonMark requires.
//
// This is purely a rendering concern: nothing rewrites the node, so the
// literal in the tree and the `value` in the AST stay exactly as written.
//
// No name is a prefix of another, so the order here is immaterial — unlike the
// TypeScript's regex alternation, where it would be first-match-wins.
var disallowedTags = []string{
	"title", "textarea", "style", "xmp", "iframe",
	"noembed", "noframes", "script", "plaintext",
}

// isTagNameEnd is the TypeScript's lookahead, `(?=[\t\n\f\r />]|$)`: the tag
// name must be followed by whitespace, `/`, `>` or the end of the text, so
// `<scriptlet>` and `<titles>` are untouched. RE2 has no lookahead, so the
// whole pattern below is a hand-coded scan rather than a regexp.
func isTagNameEnd(c byte) bool {
	switch c {
	case '\t', '\n', '\f', '\r', ' ', '/', '>':
		return true
	}
	return false
}

// disallowedTagLen returns the byte length of the disallowed-tag match
// starting at s[i] (which must be `<`) — the `<`, an optional `/` and the tag
// name, exactly what the TypeScript's regex captures — or -1 for no match.
//
// Case folding is ASCII-only, deliberately. The TypeScript applies the `i`
// flag without `u`, which never folds a non-ASCII code point onto an ASCII one;
// Go's `(?i)` would, and would make `<ſcript>` a match here but not there.
func disallowedTagLen(s string, i int) int {
	j := i + 1
	if j < len(s) && '/' == s[j] {
		j++
	}

	for _, name := range disallowedTags {
		if len(s)-j < len(name) {
			continue
		}
		k := 0
		for ; k < len(name); k++ {
			c := s[j+k]
			if 'A' <= c && c <= 'Z' {
				c += 32
			}
			if c != name[k] {
				break
			}
		}
		if k < len(name) {
			continue
		}
		if end := j + len(name); end == len(s) || isTagNameEnd(s[end]) {
			return end - i
		}
	}

	return -1
}

func filterDisallowedTags(s string) string {
	// Cheap reject: the vast majority of raw HTML holds none of these.
	if strings.IndexByte(s, '<') < 0 {
		return s
	}

	var b strings.Builder
	matched := false
	last := 0

	for i := 0; i < len(s); {
		if '<' != s[i] {
			i++
			continue
		}
		n := disallowedTagLen(s, i)
		if n < 0 {
			i++
			continue
		}
		if !matched {
			matched = true
			b.Grow(len(s) + 8)
		}
		b.WriteString(s[last:i])
		// Only the leading `<` changes; the tag name is copied as written.
		b.WriteString("&lt;")
		b.WriteString(s[i+1 : i+n])
		last = i + n
		i = last
	}

	if !matched {
		return s
	}
	b.WriteString(s[last:])
	return b.String()
}

// rawHTML is raw HTML as written, minus the tags GFM's tagfilter neutralises.
func rawHTML(literal string, gfm bool) string {
	if gfm {
		return filterDisallowedTags(literal)
	}
	return literal
}

// RenderHTML renders doc (a tree that has been through both parse phases) as
// HTML.
//
// Two options reach the renderer. Breaks turns soft line breaks into hard
// ones. GFM selects one thing only — the disallowed-raw-HTML filter, which the
// extension defines at render time rather than at parse time. Everything else
// GFM adds is settled by then: by the time a del node or an item's checked
// state exists, the renderer just prints it.
//
// The TS takes a partial option object and resolves it here, defaulting `gfm`
// to what the *document* was parsed with so that renderHTML(tree) with no
// options renders the flavour it was parsed as. Go takes the already-resolved
// Options — what the public ToHTML in markdown.go holds by the time it calls
// in — so there is no absent case to default; a caller who wants that
// behaviour spells it as RenderHTML(tree, Options{GFM: tree.GFM}). MdNode.GFM
// carries the parse flag either way.
func RenderHTML(doc *MdNode, opts Options) string {
	// §6.9 soft line breaks: rendered as a newline, or as a hard break when
	// the caller asks for breaks.
	softbreak := "\n"
	if opts.Breaks {
		softbreak = "<br />\n"
	}

	r := &htmlRenderer{atLineStart: true}

	walker := doc.Walker()
	event := walker.Next()

	for nil != event {
		node := event.Node
		entering := event.Entering

		switch node.Type {
		case NodeDocument:
			// The document wrapper renders nothing of its own.

		case NodeParagraph:
			// §5.3: "if a list is tight, we remove the <p> tags from the item
			// contents". The paragraph's grandparent is the list — a paragraph
			// whose grandparent is a list is necessarily an item's direct
			// child.
			var grandparent *MdNode
			if nil != node.Parent {
				grandparent = node.Parent.Parent
			}
			tight := nil != grandparent &&
				NodeList == grandparent.Type &&
				nil != grandparent.ListData &&
				grandparent.ListData.Tight

			if entering {
				if !tight {
					r.cr()
					r.tag("p", nil, false)
				}
				// GFM task list item. The block phase consumed the `[x]`
				// marker and left the state on the item, so the checkbox is
				// written here, at the head of the item's first paragraph:
				// directly after <li> when the list is tight, and inside the
				// <p> when it is loose. The trailing space is the separator the
				// extension's own output shows between the checkbox and the
				// item text.
				item := node.Parent
				if nil != item && NodeItem == item.Type &&
					item.FirstChild == node && item.HasChecked {
					var attrs []attr
					if item.Checked {
						attrs = append(attrs, attr{"checked", ""})
					}
					attrs = append(attrs, attr{"disabled", ""})
					attrs = append(attrs, attr{"type", "checkbox"})
					r.tag("input", attrs, false)
					r.lit(" ")
				}
			} else if !tight {
				r.tag("/p", nil, false)
				r.cr()
			}

		case NodeHeading:
			name := "h" + strconv.Itoa(node.Level)
			if entering {
				r.cr()
				r.tag(name, nil, false)
			} else {
				r.tag("/"+name, nil, false)
				r.cr()
			}

		case NodeThematicBreak:
			r.cr()
			r.tag("hr", nil, true)
			r.cr()

		case NodeBlockQuote:
			r.cr()
			if entering {
				r.tag("blockquote", nil, false)
			} else {
				r.tag("/blockquote", nil, false)
			}
			r.cr()

		case NodeList:
			data := node.ListData
			ordered := nil != data && ListOrdered == data.Type
			name := "ul"
			if ordered {
				name = "ol"
			}
			if entering {
				var attrs []attr
				// §5.3: the start number is only rendered when it is not 1 —
				// and start="0" is meaningful, so this is an inequality, not a
				// truthiness test.
				if ordered && nil != data && 1 != data.Start {
					attrs = append(attrs, attr{"start", strconv.Itoa(data.Start)})
				}
				r.cr()
				r.tag(name, attrs, false)
			} else {
				r.cr()
				r.tag("/"+name, nil, false)
			}
			r.cr()

		case NodeItem:
			// No cr() after <li>: a tight item renders <li>text</li>, and a
			// loose one gets its newline from the leading cr() of the first
			// child block.
			if entering {
				r.tag("li", nil, false)
			} else {
				r.tag("/li", nil, false)
				r.cr()
			}

		case NodeCodeBlock:
			var attrs []attr
			// §4.5: "the first word of the info string is typically used to
			// specify the language". The info string arrives from the block
			// phase already backslash-unescaped and entity-decoded; only the
			// escaping for attribute context is left to do here.
			//
			// TS carries info as `string | null`; Go splits that into Info +
			// HasInfo, and null and "" render identically, so an absent info
			// is just the empty string here.
			info := ""
			if node.HasInfo {
				info = node.Info
			}
			firstWord := ""
			if "" != info {
				firstWord = firstInfoWord(info)
			}
			if "" != firstWord {
				attrs = append(attrs, attr{"class", "language-" + escapeXML(firstWord)})
			}
			r.cr()
			r.tag("pre", nil, false)
			r.tag("code", attrs, false)
			// The block phase guarantees the literal already ends with a
			// newline (or is empty, for an empty fenced block, which renders
			// as <pre><code></code></pre>).
			r.lit(escapeXML(node.Literal))
			r.tag("/code", nil, false)
			r.tag("/pre", nil, false)
			r.cr()

		case NodeHTMLBlock:
			// §4.6: raw HTML passes through verbatim, unescaped — except for
			// the nine tags GFM's tagfilter neutralises.
			r.cr()
			r.lit(rawHTML(node.Literal, opts.GFM))
			r.cr()

		case NodeText:
			r.lit(escapeXML(node.Literal))

		case NodeSoftbreak:
			r.lit(softbreak)

		case NodeLinebreak:
			r.tag("br", nil, true)
			r.cr()

		case NodeCode:
			r.tag("code", nil, false)
			r.lit(escapeXML(node.Literal))
			r.tag("/code", nil, false)

		case NodeHTMLInline:
			r.lit(rawHTML(node.Literal, opts.GFM))

		case NodeEmph:
			if entering {
				r.tag("em", nil, false)
			} else {
				r.tag("/em", nil, false)
			}

		case NodeStrong:
			if entering {
				r.tag("strong", nil, false)
			} else {
				r.tag("/strong", nil, false)
			}

		case NodeDel:
			if entering {
				r.tag("del", nil, false)
			} else {
				r.tag("/del", nil, false)
			}

		case NodeLink:
			if entering {
				// TS routes the destination through destinationOf() to collapse
				// null to ''; Go's Destination is a plain string, so the field
				// is used directly.
				attrs := []attr{{"href", escapeXML(normalizeURI(node.Destination))}}
				// An empty title is dropped: [link](/url "") renders as a bare
				// <a href="/url">. HasTitle is Go's stand-in for the TS null
				// check.
				if node.HasTitle && "" != node.Title {
					attrs = append(attrs, attr{"title", escapeXML(node.Title)})
				}
				r.tag("a", attrs, false)
			} else {
				r.tag("/a", nil, false)
			}

		case NodeImage:
			if entering {
				attrs := []attr{
					{"src", escapeXML(normalizeURI(node.Destination))},
					{"alt", escapeXML(altText(node))},
				}
				if node.HasTitle && "" != node.Title {
					attrs = append(attrs, attr{"title", escapeXML(node.Title)})
				}
				r.tag("img", attrs, true)
				// The children have been consumed into alt; jump the walk
				// straight to this image's exit event so they are not also
				// rendered as markup.
				walker.ResumeAt(node, false)
			}
		}

		event = walker.Next()
	}

	return r.buf.String()
}

// §6.4 images: "the image description gets represented as the alt attribute"
// — as plain text. Descend the subtree keeping the text and code-span
// literals and dropping the markup wrappers, so `![foo *bar*](/url)` yields
// alt="foo bar" and a nested link contributes only its text.
//
// Raw inline HTML and line breaks inside a description are not exercised by
// the spec suite and the two reference implementations disagree: cmark
// escapes the raw HTML into the attribute and turns breaks into spaces,
// commonmark.js emits the HTML verbatim (producing a malformed attribute) and
// turns breaks into newlines. We keep the literal — escaped, since the result
// is attribute-context text — and follow commonmark.js on breaks.
func altText(image *MdNode) string {
	var alt strings.Builder

	walker := image.Walker()
	walker.Next() // discard the image's own entering event

	for event := walker.Next(); nil != event && event.Node != image; event = walker.Next() {
		if !event.Entering {
			continue
		}
		switch event.Node.Type {
		case NodeText, NodeCode, NodeHTMLInline:
			alt.WriteString(event.Node.Literal)
		case NodeSoftbreak, NodeLinebreak:
			alt.WriteByte('\n')
		default:
			// Containers (emph, strong, link, a nested image) contribute
			// nothing themselves; the walk still descends into their children.
		}
	}

	return alt.String()
}

// firstInfoWord returns everything before the first whitespace run, which is
// what the TS gets from `info.split(/\s+/)[0]`.
//
// Hand-coded rather than regexp-based for two reasons: RE2's \s is ASCII-only
// where JavaScript's is not, and the empty result matters — an info string
// that *starts* with whitespace yields "" (JS split leaves an empty first
// element), which suppresses the class attribute entirely. The block phase
// trims the info string, but an entity such as `&#32;` unescapes to a space
// afterwards, so leading whitespace is reachable.
func firstInfoWord(info string) string {
	if sp, _ := jsSpaceIndex(info); sp >= 0 {
		return info[:sp]
	}
	return info
}
