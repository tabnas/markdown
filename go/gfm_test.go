/* Copyright (c) 2025-2026 Richard Rodger and other contributors, MIT License */

package tabnasmarkdown

// GFM extension conformance and behaviour, the Go twin of the GFM half of
// ts/test/commonmark.test.ts.
//
// Five extensions are implemented — tables, strikethrough, task list items,
// autolink literals and the disallowed-raw-HTML filter — and all five are
// gated on the single GFM option.
//
// The corpus is test/gfm/spec.json, the same 24 extension examples
// ts/tools/gfm-conformance.mjs reports on, so the two runtimes are held to one
// standard rather than to each other. Run just the table with:
//
//	go test -run TestGFMSpec -v ./...

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"
)

// gfmOpts is what these extensions are gated on; cmOpts is the CommonMark
// baseline the whole 652-example suite runs with.
var (
	gfmOpts = Options{GFM: true, Breaks: false}
	cmOpts  = Options{GFM: false, Breaks: false}
)

// implementedGFMSections must be complete — every vendored section.
var implementedGFMSections = map[string]bool{
	"Tables (extension)":              true,
	"Task list items (extension)":     true,
	"Strikethrough (extension)":       true,
	"Autolinks (extension)":           true,
	"Disallowed Raw HTML (extension)": true,
}

func loadGFMCases(t *testing.T) []specCase {
	t.Helper()
	path := filepath.Join("..", "test", "gfm", "spec.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	var cases []specCase
	if err := json.Unmarshal(raw, &cases); err != nil {
		t.Fatalf("parse %s: %v", path, err)
	}
	if len(cases) != 24 {
		t.Fatalf("expected the vendored 24-example extension corpus, got %d", len(cases))
	}
	return cases
}

func TestGFMSpec(t *testing.T) {
	cases := loadGFMCases(t)

	type tally struct{ pass, total int }
	bySection := map[string]*tally{}
	var order []string

	for _, c := range cases {
		s, ok := bySection[c.Section]
		if !ok {
			s = &tally{}
			bySection[c.Section] = s
			order = append(order, c.Section)
		}
		s.total++

		actual := ToHTML(c.Markdown, gfmOpts)
		if actual == c.HTML {
			s.pass++
			continue
		}
		if !implementedGFMSections[c.Section] {
			// Out of scope: reported in the table, never a failure.
			continue
		}
		t.Errorf("example %d [%s]\n  markdown: %q\n  expected: %q\n  actual:   %q",
			c.Example, c.Section, c.Markdown, c.HTML, actual)
	}

	for name := range implementedGFMSections {
		if nil == bySection[name] {
			t.Errorf("corpus has no %q section", name)
		}
	}

	total, passed := 0, 0
	sort.SliceStable(order, func(i, j int) bool {
		a, b := bySection[order[i]], bySection[order[j]]
		return float64(a.pass)/float64(a.total) < float64(b.pass)/float64(b.total)
	})
	for _, name := range order {
		s := bySection[name]
		total += s.total
		passed += s.pass
		mark := "   "
		if s.pass == s.total {
			mark = "OK "
		}
		t.Logf("  %s %-40s %3d/%-3d", mark, name, s.pass, s.total)
	}
	t.Logf("  TOTAL %d/%d", passed, total)
}

// --- task list items --------------------------------------------------------

func TestGFMTaskListItems(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}

	// A marker becomes a checkbox and is consumed. Note the exact attribute
	// order and the space after the tag.
	check("- [ ] foo\n",
		"<ul>\n<li><input disabled=\"\" type=\"checkbox\"> foo</li>\n</ul>\n", "unchecked")
	check("- [x] bar\n",
		"<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> bar</li>\n</ul>\n", "checked")
	// "either a whitespace character or the letter x in either lowercase or
	// uppercase" — and a tab between the brackets is whitespace, so unchecked.
	check("- [X] up\n",
		"<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> up</li>\n</ul>\n", "uppercase X")
	check("- [\t] tab\n",
		"<ul>\n<li><input disabled=\"\" type=\"checkbox\"> tab</li>\n</ul>\n", "tab is whitespace")

	// Every list flavour, and arbitrarily nestable.
	check("* [ ] a\n",
		"<ul>\n<li><input disabled=\"\" type=\"checkbox\"> a</li>\n</ul>\n", "star bullet")
	check("1. [x] a\n",
		"<ol>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> a</li>\n</ol>\n", "ordered")
	check("> - [x] quoted\n",
		"<blockquote>\n<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> quoted</li>\n</ul>\n</blockquote>\n",
		"inside a block quote")
	check("- - [x] deep\n",
		"<ul>\n<li>\n<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> deep</li>\n</ul>\n</li>\n</ul>\n",
		"nested list")

	// A loose item puts the checkbox inside the paragraph.
	check("- [x] a\n\n- [ ] b\n",
		"<ul>\n<li>\n<p><input checked=\"\" disabled=\"\" type=\"checkbox\"> a</p>\n</li>\n"+
			"<li>\n<p><input disabled=\"\" type=\"checkbox\"> b</p>\n</li>\n</ul>\n",
		"loose list")

	// What is not a marker. The extension requires "at least one whitespace
	// character before any other content", and only a space, an `x` or an `X`
	// between the brackets.
	check("- [x]no space\n", "<ul>\n<li>[x]no space</li>\n</ul>\n", "no trailing whitespace")
	check("- [x]\n", "<ul>\n<li>[x]</li>\n</ul>\n", "nothing after the marker")
	check("- [y] no\n", "<ul>\n<li>[y] no</li>\n</ul>\n", "not x or whitespace")
	check("- [xx] no\n", "<ul>\n<li>[xx] no</li>\n</ul>\n", "two characters")
	// Only the *first* block of the item, and only at its start.
	check("- a\n\n  [x] b\n", "<ul>\n<li>\n<p>a</p>\n<p>[x] b</p>\n</li>\n</ul>\n", "second block")
	check("[x] loose text\n", "<p>[x] loose text</p>\n", "not a list item at all")

	// The marker beats a link reference definition of the same label: it is
	// decided over the paragraph's raw text, before the inline phase runs.
	check("[x]: /url\n\n- [x] still a task\n",
		"<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> still a task</li>\n</ul>\n",
		"beats a reference definition")
}

func TestGFMTaskListItemsAST(t *testing.T) {
	doc := jsonRoundMD(ParseDocument("- [x] a\n- [ ] b\n- c\n", gfmOpts)).(map[string]any)
	list := doc["children"].([]any)[0].(map[string]any)
	items := list["children"].([]any)

	if 3 != len(items) {
		t.Fatalf("expected 3 items, got %d", len(items))
	}
	for i, want := range []any{true, false, nil} {
		item := items[i].(map[string]any)
		got, present := item["checked"]
		// mdast keeps the field on every item, null included.
		if !present {
			t.Errorf("item %d has no `checked` field", i)
		}
		if got != want {
			t.Errorf("item %d checked = %#v, want %#v", i, got, want)
		}
	}

	// The marker is gone from the text.
	want := []any{map[string]any{
		"type":     "paragraph",
		"children": []any{map[string]any{"type": "text", "value": "a"}},
	}}
	if got := items[0].(map[string]any)["children"]; !reflect.DeepEqual(got, want) {
		t.Errorf("first item children = %#v, want %#v", got, want)
	}

	// Nested items carry their own state.
	nested := jsonRoundMD(ParseDocument("- [x] a\n  - [ ] b\n", gfmOpts)).(map[string]any)
	outer := nested["children"].([]any)[0].(map[string]any)["children"].([]any)[0].(map[string]any)
	if true != outer["checked"] {
		t.Errorf("outer item checked = %#v, want true", outer["checked"])
	}
	innerList := outer["children"].([]any)[1].(map[string]any)
	inner := innerList["children"].([]any)[0].(map[string]any)
	if false != inner["checked"] {
		t.Errorf("inner item checked = %#v, want false", inner["checked"])
	}
}

// --- disallowed raw HTML ----------------------------------------------------

func TestGFMDisallowedRawHTML(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}

	// The nine tags lose their leading angle bracket, opening and closing.
	for _, tag := range disallowedTags {
		check("a <"+tag+"> b\n", "<p>a &lt;"+tag+"> b</p>\n", tag)
		check("a </"+tag+"> b\n", "<p>a &lt;/"+tag+"> b</p>\n", "/"+tag)
	}

	// Case-insensitive, block and inline.
	check("a <XMP> b\n", "<p>a &lt;XMP> b</p>\n", "uppercase")
	check("a </ScRiPt> b\n", "<p>a &lt;/ScRiPt> b</p>\n", "mixed case, closing")
	check("<script>\nx\n</script>\n", "&lt;script>\nx\n&lt;/script>\n", "html block")
	check("<blockquote>\n  <xmp> no\n</blockquote>\n",
		"<blockquote>\n  &lt;xmp> no\n</blockquote>\n", "inside an html block")

	// Everything else passes through verbatim, as CommonMark requires.
	check("a <em> b\n", "<p>a <em> b</p>\n", "not in the set")
	check("a <scriptlet> b\n", "<p>a <scriptlet> b</p>\n", "longer tag name")
	check("a <titles> b\n", "<p>a <titles> b</p>\n", "longer tag name")
	check("a <script-ish> b\n", "<p>a <script-ish> b</p>\n", "hyphenated tag name")
	// ASCII case folding only: Go's (?i) would fold U+017F onto `s`, and the
	// canonical runtime's `i` flag does not.
	check("a <\u017fcript> b\n", "<p>a &lt;\u017fcript&gt; b</p>\n", "long s is not a tag at all")
	// The filter is a rendering step; a code span is text, not raw HTML.
	check("`<script>`\n", "<p><code>&lt;script&gt;</code></p>\n", "code span")

	// The tree and the AST keep the original text.
	doc := jsonRoundMD(ParseDocument("<script>x</script>\n", gfmOpts)).(map[string]any)
	want := map[string]any{"type": "html", "value": "<script>x</script>"}
	if got := doc["children"].([]any)[0]; !reflect.DeepEqual(got, want) {
		t.Errorf("AST = %#v, want %#v", got, want)
	}
}

// --- autolink literals ------------------------------------------------------

func anchor(href, text string) string {
	return `<a href="` + href + `">` + text + `</a>`
}

func TestGFMAutolinkLiterals(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}
	links := func(src string) bool { return strings.Contains(ToHTML(src, gfmOpts), "<a href") }

	// www., the three schemes, and email.
	check("www.commonmark.org\n",
		"<p>"+anchor("http://www.commonmark.org", "www.commonmark.org")+"</p>\n", "www.")
	check("http://commonmark.org\n",
		"<p>"+anchor("http://commonmark.org", "http://commonmark.org")+"</p>\n", "http")
	check("https://commonmark.org\n",
		"<p>"+anchor("https://commonmark.org", "https://commonmark.org")+"</p>\n", "https")
	check("ftp://foo.bar.baz\n",
		"<p>"+anchor("ftp://foo.bar.baz", "ftp://foo.bar.baz")+"</p>\n", "ftp")
	check("foo@bar.baz\n",
		"<p>"+anchor("mailto:foo@bar.baz", "foo@bar.baz")+"</p>\n", "email")

	// Only at a line start, after whitespace, or after * _ ~ (.
	for _, before := range []string{"", " ", "*", "_", "~", "("} {
		if !links(before + "www.a.com\n") {
			t.Errorf("%q should open an autolink", before)
		}
	}
	for _, before := range []string{"x", "-", "/", "=", `"`, "&", ":"} {
		if links(before + "www.a.com\n") {
			t.Errorf("%q should not open an autolink", before)
		}
	}

	// A valid domain needs a period and no underscore in its last two segments.
	for _, c := range []struct {
		src  string
		want bool
		why  string
	}{
		{"x www.a.b.com y\n", true, "plain"},
		{"x www.foo_bar.com y\n", false, "second-to-last segment"},
		{"x www.a.foo_bar y\n", false, "last segment"},
		{"x www.foo_bar.a.com y\n", true, "earlier segments may hold _"},
		{"x http://foo_bar.com y\n", false, "scheme form, same rule"},
		{"x http://localhost/y z\n", false, "no period, no domain"},
	} {
		if got := links(c.src); got != c.want {
			t.Errorf("%s: %q linked = %v, want %v", c.why, c.src, got, c.want)
		}
	}

	// Extended autolink path validation: trailing punctuation is not part of
	// the link.
	check("Visit www.commonmark.org.\n",
		"<p>Visit "+anchor("http://www.commonmark.org", "www.commonmark.org")+".</p>\n",
		"trailing period")
	check("Visit www.commonmark.org/a.b.\n",
		"<p>Visit "+anchor("http://www.commonmark.org/a.b", "www.commonmark.org/a.b")+".</p>\n",
		"interior periods survive")
	for _, p := range []string{"?", "!", ".", ",", ":", "*", "~"} {
		if !strings.Contains(ToHTML("x www.a.com"+p+"\n", gfmOpts), ">www.a.com</a>") {
			t.Errorf("trailing %q is not part of the link", p)
		}
	}
	// `_` is the one that is also a domain character, so it reaches the domain
	// check before the trim can drop it and invalidates the last segment —
	// there is no link here at all, which is cmark-gfm's behaviour too.
	check("x www.a.com_\n", "<p>x www.a.com_</p>\n", "trailing underscore, in the domain")
	// Past the domain it is ordinary trailing punctuation again.
	check("x www.a.com/y_\n",
		"<p>x "+anchor("http://www.a.com/y", "www.a.com/y")+"_</p>\n",
		"trailing underscore, in the path")

	// Parentheses balance across the whole match, and only when it ends in `)`.
	link := anchor("http://www.google.com/search?q=Markup+(business)",
		"www.google.com/search?q=Markup+(business)")
	check("www.google.com/search?q=Markup+(business)\n", "<p>"+link+"</p>\n", "balanced")
	check("www.google.com/search?q=Markup+(business)))\n", "<p>"+link+"))</p>\n", "two extra")
	check("(www.google.com/search?q=Markup+(business))\n", "<p>("+link+")</p>\n", "wrapped")
	check("(www.google.com/search?q=Markup+(business)\n", "<p>("+link+"</p>\n", "open wrapper")
	// Interior parentheses alone trigger nothing.
	check("www.google.com/search?q=(business))+ok\n",
		"<p>"+anchor("http://www.google.com/search?q=(business))+ok",
			"www.google.com/search?q=(business))+ok")+"</p>\n",
		"does not end in )")

	// A trailing entity-like `;`, and `<` ends a link.
	check("www.google.com/search?q=commonmark&hl=en\n",
		"<p>"+anchor("http://www.google.com/search?q=commonmark&amp;hl=en",
			"www.google.com/search?q=commonmark&amp;hl=en")+"</p>\n",
		"& that is not an entity")
	check("www.google.com/search?q=commonmark&hl;\n",
		"<p>"+anchor("http://www.google.com/search?q=commonmark",
			"www.google.com/search?q=commonmark")+"&amp;hl;</p>\n",
		"entity-like trailing ;")
	check("www.commonmark.org/he<lp\n",
		"<p>"+anchor("http://www.commonmark.org/he", "www.commonmark.org/he")+"&lt;lp</p>\n",
		"< ends the link")

	// Email: `+` before the `@` only, and a trailing `-` or `_` invalidates it.
	check("hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.\n",
		"<p>hello@mail+xyz.example isn't valid, but "+
			anchor("mailto:hello+xyz@mail.example", "hello+xyz@mail.example")+" is.</p>\n",
		"+ before the @ only")
	check("a.b-c_d@a.b\n", "<p>"+anchor("mailto:a.b-c_d@a.b", "a.b-c_d@a.b")+"</p>\n", "local part")
	check("a.b-c_d@a.b.\n", "<p>"+anchor("mailto:a.b-c_d@a.b", "a.b-c_d@a.b")+".</p>\n", "trailing period")
	check("a.b-c_d@a.b-\n", "<p>a.b-c_d@a.b-</p>\n", "trailing hyphen invalidates")
	check("a.b-c_d@a.b_\n", "<p>a.b-c_d@a.b_</p>\n", "trailing underscore invalidates")
	check("foo@bar\n", "<p>foo@bar</p>\n", "the domain needs a period")

	// Never inside a link, a code span, or raw HTML.
	check("[label www.a.com](/dest)\n", "<p>"+anchor("/dest", "label www.a.com")+"</p>\n", "inline link label")
	check("[label www.a.com][ref]\n\n[ref]: /dest\n",
		"<p>"+anchor("/dest", "label www.a.com")+"</p>\n", "reference link label")
	check("<https://a.com/x>\n", "<p>"+anchor("https://a.com/x", "https://a.com/x")+"</p>\n",
		"a CommonMark autolink is already a link")
	check("`www.a.com`\n", "<p><code>www.a.com</code></p>\n", "code span")
	check("<b title=\"www.a.com\">x</b>\n", "<p><b title=\"www.a.com\">x</b></p>\n", "raw html")
	check("![alt www.a.com](/i)\n", "<p><img src=\"/i\" alt=\"alt www.a.com\" /></p>\n", "image alt text")

	// Inside emphasis, strikethrough, headings and list items.
	check("*www.a.com*\n", "<p><em>"+anchor("http://www.a.com", "www.a.com")+"</em></p>\n", "emphasis")
	check("~~www.a.com~~\n", "<p><del>"+anchor("http://www.a.com", "www.a.com")+"</del></p>\n", "strikethrough")
	check("# www.a.com\n", "<h1>"+anchor("http://www.a.com", "www.a.com")+"</h1>\n", "heading")
	check("- www.a.com\n", "<ul>\n<li>"+anchor("http://www.a.com", "www.a.com")+"</li>\n</ul>\n", "list item")
}

func TestGFMAutolinkAST(t *testing.T) {
	// `_` ends a text run in the inline scanner, so `a.b-c_d@a.b` reaches the
	// post-pass as three text siblings. Consolidating them is what makes the
	// whole address one link rather than `d@a.b`.
	doc := jsonRoundMD(ParseDocument("a.b-c_d@a.b\n", gfmOpts)).(map[string]any)
	want := []any{map[string]any{
		"type":     "link",
		"url":      "mailto:a.b-c_d@a.b",
		"title":    nil,
		"children": []any{map[string]any{"type": "text", "value": "a.b-c_d@a.b"}},
	}}
	got := doc["children"].([]any)[0].(map[string]any)["children"]
	if !reflect.DeepEqual(got, want) {
		t.Errorf("spanning match: got %#v want %#v", got, want)
	}

	// The AST carries the decoded destination; the renderer percent-encodes.
	doc = jsonRoundMD(ParseDocument("see www.a.com/ä now\n", gfmOpts)).(map[string]any)
	link := doc["children"].([]any)[0].(map[string]any)["children"].([]any)[1]
	wantLink := map[string]any{
		"type":     "link",
		"url":      "http://www.a.com/ä",
		"title":    nil,
		"children": []any{map[string]any{"type": "text", "value": "www.a.com/ä"}},
	}
	if !reflect.DeepEqual(link, wantLink) {
		t.Errorf("non-ASCII path: got %#v want %#v", link, wantLink)
	}
	if !strings.Contains(ToHTML("see www.a.com/ä now\n", gfmOpts), `href="http://www.a.com/%C3%A4"`) {
		t.Errorf("non-ASCII path is not percent-encoded in the HTML")
	}
}

// --- gfm:false --------------------------------------------------------------

// TestGFMDisabled is the regression that matters most: the whole 652-example
// CommonMark suite runs with GFM off, so every extension has to vanish.
func TestGFMDisabled(t *testing.T) {
	for _, c := range []struct{ src, want string }{
		{"- [ ] foo\n", "<ul>\n<li>[ ] foo</li>\n</ul>\n"},
		{"- [x] foo\n", "<ul>\n<li>[x] foo</li>\n</ul>\n"},
		{"<script>alert(1)</script>\n", "<script>alert(1)</script>\n"},
		{"a <title> b\n", "<p>a <title> b</p>\n"},
		{"www.commonmark.org\n", "<p>www.commonmark.org</p>\n"},
		{"http://commonmark.org\n", "<p>http://commonmark.org</p>\n"},
		{"foo@bar.baz\n", "<p>foo@bar.baz</p>\n"},
		{"~~x~~\n", "<p>~~x~~</p>\n"},
	} {
		if got := ToHTML(c.src, cmOpts); got != c.want {
			t.Errorf("ToHTML %q\n got  %q\n want %q", c.src, got, c.want)
		}
		// The tree records the parse flavour, so a caller rendering a tree can
		// ask for exactly what it was parsed as. This is the Go spelling of
		// the TypeScript's renderHTML(tree) with no options at all.
		tree := ParseTree(c.src, cmOpts)
		if tree.GFM {
			t.Errorf("ParseTree %q: doc.GFM = true, want false", c.src)
		}
		if got := RenderHTML(tree, Options{GFM: tree.GFM}); got != c.want {
			t.Errorf("RenderHTML(tree, tree.GFM) %q\n got  %q\n want %q", c.src, got, c.want)
		}
	}

	// …and an explicit option still wins over what the tree records.
	tree := ParseTree("<script>x</script>\n", cmOpts)
	if got := RenderHTML(tree, gfmOpts); got != "&lt;script>x&lt;/script>\n" {
		t.Errorf("explicit GFM:true over a CommonMark tree: got %q", got)
	}
	if tree := ParseTree("x\n", gfmOpts); !tree.GFM {
		t.Errorf("ParseTree with GFM on: doc.GFM = false")
	}

	// The AST carries no GFM shapes either.
	doc := jsonRoundMD(ParseDocument("- [x] www.a.com and a@b.co\n", cmOpts)).(map[string]any)
	item := doc["children"].([]any)[0].(map[string]any)["children"].([]any)[0].(map[string]any)
	if nil != item["checked"] {
		t.Errorf("checked = %#v, want nil", item["checked"])
	}
	want := []any{map[string]any{
		"type": "paragraph",
		"children": []any{
			map[string]any{"type": "text", "value": "[x] www.a.com and a@b.co"},
		},
	}}
	if got := item["children"]; !reflect.DeepEqual(got, want) {
		t.Errorf("item children = %#v, want %#v", got, want)
	}
}

// --- robustness -------------------------------------------------------------

// TestGFMAutolinkRobustness: nothing here may panic, and nothing may become
// super-linear. The inputs are adversarial, and untrusted input picks them.
func TestGFMAutolinkRobustness(t *testing.T) {
	t.Run("long run of www. prefixes", func(t *testing.T) {
		out := ToHTML(strings.Repeat("www.", 50000)+"\n", gfmOpts)
		// One link over the whole run; the final period is trailing punctuation.
		if !strings.HasPrefix(out, `<p><a href="http://www.www.`) {
			t.Errorf("prefix: %.60q", out)
		}
		if !strings.HasSuffix(out, "www</a>.</p>\n") {
			t.Errorf("suffix: %.60q", out[len(out)-60:])
		}
		if n := strings.Count(out, "<a href"); n != 1 {
			t.Errorf("expected one link, got %d", n)
		}
	})

	t.Run("deeply parenthesised URLs", func(t *testing.T) {
		open, closed := strings.Repeat("(", 20000), strings.Repeat(")", 20000)

		bal := ToHTML("www.a.com/"+open+closed+"\n", gfmOpts)
		if !strings.Contains(bal, `href="http://www.a.com/`+open+closed+`"`) {
			t.Errorf("balanced parens are not all part of the link")
		}

		unbal := ToHTML("www.a.com/x"+closed+"\n", gfmOpts)
		if !strings.HasPrefix(unbal, `<p><a href="http://www.a.com/x">www.a.com/x</a>)`) {
			t.Errorf("unbalanced prefix: %.60q", unbal)
		}
		if !strings.HasSuffix(unbal, closed+"</p>\n") {
			t.Errorf("unbalanced parens did not stay in the text")
		}
	})

	t.Run("candidates that fail late stay linear", func(t *testing.T) {
		// `_` both continues a domain and opens an autolink, so every
		// underscore here starts a candidate whose domain runs to the end of
		// the paragraph — the shape that goes quadratic without a bound on the
		// domain scan.
		measure := func(n int) time.Duration {
			src := strings.Repeat("_www.a_b.c", n) + "\n"
			start := time.Now()
			_ = ToHTML(src, gfmOpts)
			return time.Since(start)
		}

		measure(2000)
		small := bestOf(5, func() time.Duration { return measure(20000) })
		large := bestOf(5, func() time.Duration { return measure(80000) })

		if small < 2*time.Millisecond {
			return // too fast to measure reliably; assert nothing rather than flake
		}
		ratio := float64(large) / float64(small)
		t.Logf("4x input took %.1fx time (%v -> %v)", ratio, small, large)
		if ratio > 12 {
			t.Errorf("4x input took %.1fx time (%v -> %v); linear is ~4x, quadratic ~16x",
				ratio, small, large)
		}
	})

	t.Run("degenerate fragments", func(t *testing.T) {
		for _, src := range []string{
			"www.", "www..", "www...", "w", "@", "@@@", "www.@", "a@", "@b.c",
			"http://", "://", "www. x", "a@" + strings.Repeat(".", 500),
			"www.a.com/" + strings.Repeat(")", 500), strings.Repeat("&", 500) + ";",
			"- [", "- []", "- [ ", "<script", "</",
			// Byte-vs-rune: a multi-byte character next to every boundary the
			// pass computes.
			"www.é.com", "www.a.com/é.", "é@a.com", "a@é.com", "(www.a.com/é)",
			"www.a.com/\xff\xfe", "\xffwww.a.com",
		} {
			for _, opts := range []Options{gfmOpts, cmOpts} {
				func() {
					defer func() {
						if r := recover(); r != nil {
							t.Errorf("%q gfm=%v panicked: %v", src, opts.GFM, r)
						}
					}()
					_ = ToHTML(src+"\n", opts)
					_ = ParseDocument(src+"\n", opts)
				}()
			}
		}
	})
}
