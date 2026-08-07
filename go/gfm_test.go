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

// --- tables -----------------------------------------------------------------
//
// The Go twin of ts/test/commonmark.test.ts `describe('gfm tables')` and
// `describe('gfm table robustness')`.

// A table's HTML is verbose enough that a literal expectation buries the thing
// being tested, so rows are assembled from their cells here. Newline placement
// is still byte-exact — that is what the corpus judges.
func trow(cells []string) string {
	return "<tr>\n" + strings.Join(cells, "\n") + "\n</tr>\n"
}

func thead(cells []string) string { return "<thead>\n" + trow(cells) + "</thead>\n" }

func tbody(rows [][]string) string {
	var b strings.Builder
	b.WriteString("<tbody>\n")
	for _, r := range rows {
		b.WriteString(trow(r))
	}
	b.WriteString("</tbody>\n")
	return b.String()
}

// tableHTML takes the <thead> block and zero or one <tbody> block: a table
// with no body rows has no <tbody> at all.
func tableHTML(head string, body ...string) string {
	return "<table>\n" + head + strings.Join(body, "") + "</table>\n"
}

func thCell(s string, align ...string) string {
	if 0 < len(align) {
		return `<th align="` + align[0] + `">` + s + "</th>"
	}
	return "<th>" + s + "</th>"
}

func tdCell(s string, align ...string) string {
	if 0 < len(align) {
		return `<td align="` + align[0] + `">` + s + "</td>"
	}
	return "<td>" + s + "</td>"
}

// --- the eight spec behaviours, each asserted on its own ---

func TestGFMTablesSpecBehaviours(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}

	// 1. header row, delimiter row, data row.
	check("| foo | bar |\n| --- | --- |\n| baz | bim |\n",
		tableHTML(
			thead([]string{thCell("foo"), thCell("bar")}),
			tbody([][]string{{tdCell("baz"), tdCell("bim")}}),
		),
		"1. header, delimiter, data")

	// 2. colons set alignment, and pipes may be inconsistent. Cell widths need
	// not match, and a leading/trailing pipe is optional on every row
	// independently.
	check("| abc | defghi |\n:-: | -----------:\nbar | baz\n",
		tableHTML(
			thead([]string{thCell("abc", "center"), thCell("defghi", "right")}),
			tbody([][]string{{tdCell("bar", "center"), tdCell("baz", "right")}}),
		),
		"2. alignment and inconsistent pipes")
	// All four delimiter shapes, and "none" means no attribute at all.
	check("| a | b | c | d |\n| :- | -: | :-: | --- |\n| 1 | 2 | 3 | 4 |\n",
		tableHTML(
			thead([]string{
				thCell("a", "left"), thCell("b", "right"),
				thCell("c", "center"), thCell("d"),
			}),
			tbody([][]string{{
				tdCell("1", "left"), tdCell("2", "right"),
				tdCell("3", "center"), tdCell("4"),
			}}),
		),
		"2. all four delimiter shapes")

	// 3. an escaped pipe is content, inside other inline spans too.
	check("| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| b **\\|** im |\n",
		tableHTML(
			thead([]string{thCell("f|oo")}),
			tbody([][]string{
				{tdCell("b <code>|</code> az")},
				{tdCell("b <strong>|</strong> im")},
			}),
		),
		"3. escaped pipes")

	// 4. another block-level structure breaks the table.
	check("| abc | def |\n| --- | --- |\n| bar | baz |\n> bar\n",
		tableHTML(
			thead([]string{thCell("abc"), thCell("def")}),
			tbody([][]string{{tdCell("bar"), tdCell("baz")}}),
		)+"<blockquote>\n<p>bar</p>\n</blockquote>\n",
		"4. a block quote breaks the table")

	// 5. a blank line breaks the table.
	check("| abc | def |\n| --- | --- |\n| bar | baz |\nbar\n\nbar\n",
		tableHTML(
			thead([]string{thCell("abc"), thCell("def")}),
			tbody([][]string{
				{tdCell("bar"), tdCell("baz")},
				{tdCell("bar"), tdCell("")},
			}),
		)+"<p>bar</p>\n",
		"5. a blank line breaks the table")

	// 6. header and delimiter must agree on the cell count — otherwise there is
	// no table at all and the whole thing stays one paragraph.
	check("| abc | def |\n| --- |\n| bar |\n",
		"<p>| abc | def |\n| --- |\n| bar |</p>\n",
		"6. cell counts disagree")
	// The mismatch is on the count, not the widths: 1-vs-1 matches.
	check("| abc |\n| --- |\n", tableHTML(thead([]string{thCell("abc")})), "6. 1-vs-1")
	if got := ToHTML("| abc | def |\n| --- | --- | --- |\n", gfmOpts); !strings.HasPrefix(got, "<p>") {
		t.Errorf("6. 2-vs-3 should stay a paragraph, got %q", got)
	}

	// 7. short rows are padded, long rows are truncated.
	check("| abc | def |\n| --- | --- |\n| bar |\n| bar | baz | boo |\n",
		tableHTML(
			thead([]string{thCell("abc"), thCell("def")}),
			tbody([][]string{
				{tdCell("bar"), tdCell("")},
				{tdCell("bar"), tdCell("baz")},
			}),
		),
		"7. padded and truncated rows")

	// 8. no body rows means no <tbody> at all.
	out := ToHTML("| abc | def |\n| --- | --- |\n", gfmOpts)
	if want := tableHTML(thead([]string{thCell("abc"), thCell("def")})); out != want {
		t.Errorf("8. no body rows:\n got  %q\n want %q", out, want)
	}
	if strings.Contains(out, "tbody") {
		t.Errorf("8. no body rows must emit no <tbody>: %q", out)
	}
}

// --- the rules those examples rest on ---

func TestGFMTablesRules(t *testing.T) {
	html := func(src string) string { return ToHTML(src, gfmOpts) }
	check := func(src, want, why string) {
		t.Helper()
		if got := html(src); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}

	// A delimiter cell is hyphens and at most two colons. Leading and trailing
	// pipes are optional here as everywhere. A row of nothing but hyphens (`-`,
	// `---`) is left out: it is a setext underline first, which the test below
	// covers.
	for _, delim := range []string{
		"| --- |", "| :-- |", "| --: |", "| :-: |", "| - |", "|-|", "|--", "--|",
		":-", "-:", ":-:", ":---:",
	} {
		if got := html("| a |\n" + delim + "\n"); !strings.HasPrefix(got, "<table>") {
			t.Errorf("%q should be a delimiter row, got %q", delim, got)
		}
	}
	for _, delim := range []string{
		"| === |", "| -+- |", "| - - |", "| :: |", "| :  |", "| a |", "|  |", "| ::- |",
		"| -:- |", "| *** |", "| — |",
	} {
		if got := html("| a |\n" + delim + "\n"); !strings.HasPrefix(got, "<p>") {
			t.Errorf("%q should not be a delimiter row, got %q", delim, got)
		}
	}

	// Spaces and tabs between pipes and content are trimmed.
	check("|   a   |\t b\t|\n| - | - |\n|\tx\t|   y   |\n",
		tableHTML(
			thead([]string{thCell("a"), thCell("b")}),
			tbody([][]string{{tdCell("x"), tdCell("y")}}),
		),
		"spaces and tabs are trimmed")

	// The header row is the paragraph's last line, so the paragraph splits.
	check("aaa\nbbb\n| a | b |\n| - | - |\n| c | d |\n",
		"<p>aaa\nbbb</p>\n"+tableHTML(
			thead([]string{thCell("a"), thCell("b")}),
			tbody([][]string{{tdCell("c"), tdCell("d")}}),
		),
		"the paragraph above the header row is kept")
	// The lines left behind are a real paragraph: their reference definitions
	// still register, because splitting finalizes them.
	check("[r]: /url\n| a |\n| - |\n\n[r]\n",
		tableHTML(thead([]string{thCell("a")}))+"<p><a href=\"/url\">r</a></p>\n",
		"a split paragraph still contributes reference definitions")

	// A setext underline still wins over a one-column delimiter row: `---` is
	// both, and the block starts are ordered so the heading wins, exactly as
	// cmark-gfm orders them. `| foo |` as heading text is the giveaway.
	check("foo\n---\n", "<h2>foo</h2>\n", "setext beats a delimiter row")
	check("| foo |\n---\n", "<h2>| foo |</h2>\n", "setext beats a delimiter row")
	// A list marker wins too: `- | -` is a bullet, not a two-column delimiter.
	check("| a | b |\n- | -\n",
		"<p>| a | b |</p>\n<ul>\n<li>| -</li>\n</ul>\n",
		"a list marker beats a delimiter row")

	// A delimiter row with no paragraph above it is just a paragraph.
	check("| - |\n", "<p>| - |</p>\n", "no paragraph above")
	check("\n| - |\n", "<p>| - |</p>\n", "no paragraph above")
	// …and it must be the line *directly* above.
	check("| a |\n\n| - |\n", "<p>| a |</p>\n<p>| - |</p>\n", "not the line directly above")

	// A table nests in a block quote and in a list item.
	check("> | a | b |\n> | - | - |\n> | c | d |\n",
		"<blockquote>\n"+tableHTML(
			thead([]string{thCell("a"), thCell("b")}),
			tbody([][]string{{tdCell("c"), tdCell("d")}}),
		)+"</blockquote>\n",
		"inside a block quote")
	check("- | a | b |\n  | - | - |\n  | c | d |\n",
		"<ul>\n<li>\n"+tableHTML(
			thead([]string{thCell("a"), thCell("b")}),
			tbody([][]string{{tdCell("c"), tdCell("d")}}),
		)+"</li>\n</ul>\n",
		"inside a list item")
	// A lazy continuation is not a delimiter row: the quote's marker is
	// missing, so the paragraph simply carries on.
	check("> | a | b |\n| - | - |\n",
		"<blockquote>\n<p>| a | b |\n| - | - |</p>\n</blockquote>\n",
		"a lazy continuation is not a delimiter row")

	// A table immediately followed by another table. Separated by a blank line:
	// two tables.
	check("| a |\n| - |\n| 1 |\n\n| b |\n| - |\n| 2 |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("1")}}))+
			tableHTML(thead([]string{thCell("b")}), tbody([][]string{{tdCell("2")}})),
		"two tables separated by a blank line")
	// Without one, the second "table" is body rows of the first — a delimiter
	// row is only special directly under a *paragraph*.
	check("| a |\n| - |\n| b |\n| - |\n",
		tableHTML(thead([]string{thCell("a")}),
			tbody([][]string{{tdCell("b")}, {tdCell("-")}})),
		"no blank line means body rows")

	// Inlines inside cells are parsed, and only inlines.
	check("| a |\n| - |\n| *e* ~~s~~ `c` [l](/u) www.x.com |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell(
			"<em>e</em> <del>s</del> <code>c</code> " +
				"<a href=\"/u\">l</a> <a href=\"http://www.x.com\">www.x.com</a>")}})),
		"inlines in cells, autolink literals included")
	// The tagfilter is a render-time step and still applies inside a cell.
	check("| a |\n| - |\n| <title>x |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("&lt;title>x")}})),
		"the tagfilter applies inside a cell")
}

// --- escaped pipes, in detail ---

func TestGFMTablesEscapedPipes(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}

	// Splitting is on unescaped pipes only: one cell, not two.
	check("| a\\|b |\n| - |\n", tableHTML(thead([]string{thCell("a|b")})),
		"an escaped pipe never separates")
	// `\\` is an escaped backslash, so the pipe after it *does* separate.
	check("| x | y |\n| - | - |\n| a\\\\ | b |\n",
		tableHTML(thead([]string{thCell("x"), thCell("y")}),
			tbody([][]string{{tdCell("a\\"), tdCell("b")}})),
		"a backslash cannot shield the pipe after it")
	// A trailing pipe that is itself escaped is content, not the optional
	// trailing delimiter.
	check("| a\\| |\n| - |\n", tableHTML(thead([]string{thCell("a|")})),
		"an escaped trailing pipe is content")

	// An escaped pipe becomes a literal pipe BEFORE inline parsing. This is the
	// whole point of resolving `\|` at split time: a code span is a literal, so
	// nothing downstream could turn `\|` into `|` inside one.
	check("| a |\n| - |\n| `\\|` |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("<code>|</code>")}})),
		"a code span sees a raw pipe")
	check("| a |\n| - |\n| **\\|** |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("<strong>|</strong>")}})),
		"strong emphasis around a raw pipe")
	// …and the cell text is not unescaped twice: every other escape is left for
	// the inline phase, which handles it exactly once.
	check("| a |\n| - |\n| \\*not em\\* |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("*not em*")}})),
		"other escapes are left to the inline phase")
	check("| a |\n| - |\n| \\\\ |\n",
		tableHTML(thead([]string{thCell("a")}), tbody([][]string{{tdCell("\\")}})),
		"an escaped backslash is unescaped exactly once")

	// Byte-vs-rune: a multi-byte character on either side of a split or a trim
	// must survive whole. A previous port mojibake'd every non-ASCII character
	// by appending a byte.
	check("| é | 日本語 |\n| - | - |\n|  🎉  | a\\|é |\n",
		tableHTML(
			thead([]string{thCell("é"), thCell("日本語")}),
			tbody([][]string{{tdCell("🎉"), tdCell("a|é")}}),
		),
		"non-ASCII cell content survives splitting and trimming")
}

// --- the AST projection ---

func TestGFMTablesAST(t *testing.T) {
	doc := jsonRoundMD(ParseDocument("| h1 | h2 |\n| :- | -: |\n| b1 | b2 |\n", gfmOpts)).(map[string]any)
	children := doc["children"].([]any)
	if 1 != len(children) {
		t.Fatalf("expected 1 block, got %d", len(children))
	}

	table := children[0].(map[string]any)
	if "table" != table["type"] {
		t.Fatalf("type = %v, want table", table["type"])
	}
	if want := []any{"left", "right"}; !reflect.DeepEqual(table["align"], want) {
		t.Errorf("align = %#v, want %#v", table["align"], want)
	}
	rows := table["children"].([]any)
	if 2 != len(rows) {
		t.Fatalf("expected 2 rows, got %d", len(rows))
	}

	// mdast has no header flag: the FIRST row is the header, by convention.
	cell := func(v string) any {
		return map[string]any{
			"type":     "tableCell",
			"children": []any{map[string]any{"type": "text", "value": v}},
		}
	}
	wantHead := map[string]any{"type": "tableRow", "children": []any{cell("h1"), cell("h2")}}
	if !reflect.DeepEqual(rows[0], wantHead) {
		t.Errorf("header row = %#v, want %#v", rows[0], wantHead)
	}
	wantBody := map[string]any{"type": "tableRow", "children": []any{cell("b1"), cell("b2")}}
	if !reflect.DeepEqual(rows[1], wantBody) {
		t.Errorf("body row = %#v, want %#v", rows[1], wantBody)
	}

	// align carries null for a column with no colon — one entry per column,
	// always, and `[null, ...]` rather than `[""]` or `null`.
	doc = jsonRoundMD(ParseDocument("| a | b | c |\n| --- | :-: | --: |\n", gfmOpts)).(map[string]any)
	table = doc["children"].([]any)[0].(map[string]any)
	if want := []any{nil, "center", "right"}; !reflect.DeepEqual(table["align"], want) {
		t.Errorf("align = %#v, want %#v", table["align"], want)
	}
	if n := len(table["children"].([]any)[0].(map[string]any)["children"].([]any)); 3 != n {
		t.Errorf("header row has %d cells, want 3", n)
	}

	// Padded cells are real, empty cells — `children: []`, not null.
	doc = jsonRoundMD(ParseDocument("| a | b |\n| - | - |\n| x |\n", gfmOpts)).(map[string]any)
	body := doc["children"].([]any)[0].(map[string]any)["children"].([]any)[1].(map[string]any)
	cells := body["children"].([]any)
	if 2 != len(cells) {
		t.Fatalf("padded row has %d cells, want 2", len(cells))
	}
	if want := map[string]any{"type": "tableCell", "children": []any{}}; !reflect.DeepEqual(cells[1], want) {
		t.Errorf("padded cell = %#v, want %#v", cells[1], want)
	}

	// Cells hold inline nodes.
	doc = jsonRoundMD(ParseDocument("| a |\n| - |\n| *x* |\n", gfmOpts)).(map[string]any)
	got := doc["children"].([]any)[0].(map[string]any)["children"].([]any)[1].(map[string]any)["children"].([]any)[0]
	want := map[string]any{
		"type": "tableCell",
		"children": []any{map[string]any{
			"type":     "emphasis",
			"children": []any{map[string]any{"type": "text", "value": "x"}},
		}},
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("cell = %#v, want %#v", got, want)
	}

	// A table nested in a list item projects in place.
	doc = jsonRoundMD(ParseDocument("- | a |\n  | - |\n", gfmOpts)).(map[string]any)
	item := doc["children"].([]any)[0].(map[string]any)["children"].([]any)[0].(map[string]any)
	blocks := item["children"].([]any)
	if 1 != len(blocks) {
		t.Fatalf("item has %d blocks, want 1", len(blocks))
	}
	if "table" != blocks[0].(map[string]any)["type"] {
		t.Errorf("item block type = %v, want table", blocks[0].(map[string]any)["type"])
	}

	// An empty align array marshals as [], never null. Unreachable from a real
	// parse — parseDelimiterRow rejects a zero-cell row — so it is asserted on
	// a hand-built node, which is where a nil slice would leak out.
	tree := Parse("| a |\n| - |\n", gfmOpts)
	tree.FirstChild.TableAlign = nil
	raw, err := json.Marshal(ToAST(tree, gfmOpts))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if !strings.Contains(string(raw), `"align":[]`) {
		t.Errorf("empty align must marshal as [], got %s", raw)
	}
}

// --- sourcepos ---

// tableStart is the sourcepos start of the first table in src.
func tableStart(src string) [2]int {
	w := ParseTree(src, gfmOpts).Walker()
	for e := w.Next(); e != nil; e = w.Next() {
		if e.Entering && NodeTable == e.Node.Type {
			return e.Node.SourcePos[0]
		}
	}
	return [2]int{-1, -1}
}

// TestGFMTableSourcePos covers a table opened by its delimiter row but which
// begins on the header row above it, the two rows being indented
// independently. Taking the start column from the delimiter row therefore
// reports a column belonging to a different line — visible through ParseTree,
// which is public.
func TestGFMTableSourcePos(t *testing.T) {
	for _, c := range []struct {
		src  string
		want [2]int
		why  string
	}{
		{"a | b\n| - | -\n", [2]int{1, 1}, "control: neither row indented"},
		{"  a | b\n| - | -\n", [2]int{1, 3}, "the header row is indented"},
		{"a | b\n  | - | -\n", [2]int{1, 1}, "only the delimiter row is"},
		{"  a | b\n  | - | -\n", [2]int{1, 3}, "both rows are"},
	} {
		if got := tableStart(c.src); got != c.want {
			t.Errorf("%s: %q start = %v, want %v", c.why, c.src, got, c.want)
		}
	}
}

// TestGFMTableSourcePosSplitParagraph puts the header row at the *end* of a
// longer paragraph, so the column cannot come from the paragraph node either —
// that one belongs to the first line, which stays a paragraph.
func TestGFMTableSourcePosSplitParagraph(t *testing.T) {
	for _, c := range []struct {
		src  string
		want [2]int
		why  string
	}{
		{"x\n  a | b\n| - | -\n", [2]int{2, 3}, "header row indented, paragraph's is not"},
		{"  x\na | b\n| - | -\n", [2]int{2, 1}, "paragraph indented, header row is not"},
		// A table inside a block quote counts columns from the start of the
		// line, marker included, exactly as every other block does.
		{"> a | b\n> | - | -\n", [2]int{1, 3}, "block quote"},
		{">   a | b\n> | - | -\n", [2]int{1, 5}, "block quote, indented header row"},
	} {
		if got := tableStart(c.src); got != c.want {
			t.Errorf("%s: %q start = %v, want %v", c.why, c.src, got, c.want)
		}
	}
}

// firstParagraphEnd is the sourcepos end of the first paragraph in src.
func firstParagraphEnd(src string) [2]int {
	w := ParseTree(src, gfmOpts).Walker()
	for e := w.Next(); e != nil; e = w.Next() {
		if e.Entering && NodeParagraph == e.Node.Type {
			return e.Node.SourcePos[1]
		}
	}
	return [2]int{-1, -1}
}

// TestGFMTableSplitParagraphEndsOnItsOwnLine covers the end of the paragraph a
// table splits. The split closes a block *two* lines back — the header row in
// between belongs to the table — so the end column cannot come from the
// previous line the way every other finalize takes it. Taking it from there
// reports the header row's length on a line that is not the header row.
func TestGFMTableSplitParagraphEndsOnItsOwnLine(t *testing.T) {
	// `aaaaaaaaaa` is 10 characters and the header row below it is 9, so the
	// wrong column there is a *shorter* one, which no clamp could explain;
	// `aa` under a 20-character header row is the other direction.
	for _, c := range []struct {
		src  string
		want [2]int
	}{
		{"aaaaaaaaaa\n| a | b |\n| - | - |\n", [2]int{1, 10}},
		{"aa\n| aaaaaaaaaaaa | b |\n| - | - |\n", [2]int{1, 2}},
	} {
		if got := firstParagraphEnd(c.src); got != c.want {
			t.Errorf("%q paragraph end = %v, want %v", c.src, got, c.want)
		}
	}

	// Every other interrupter already ends this paragraph at column 10; the
	// table must not be the odd one out.
	for _, after := range []string{"# h\n", "```\nx\n```\n", "> q\n", "- i\n", "***\n", "<div>\n", "\nx\n"} {
		src := "aaaaaaaaaa\n" + after
		if got := firstParagraphEnd(src); got != [2]int{1, 10} {
			t.Errorf("%q paragraph end = %v, want [1 10]", src, got)
		}
	}
}

// TestGFMTableRowsShareOneBasis covers rows spanning their own text, so that
// two rows of one table on equally indented lines report the same end column.
// The header row is built in tryOpenTable and the body rows in finalizeTable;
// measuring the header against the whole source line would put it two columns
// past the body rows inside a block quote.
func TestGFMTableRowsShareOneBasis(t *testing.T) {
	rowSpans := func(src string) []SourcePos {
		var out []SourcePos
		w := ParseTree(src, gfmOpts).Walker()
		for e := w.Next(); e != nil; e = w.Next() {
			if e.Entering && NodeTableRow == e.Node.Type {
				out = append(out, e.Node.SourcePos)
			}
		}
		return out
	}

	want := []SourcePos{{{1, 1}, {1, 9}}, {{3, 1}, {3, 9}}}
	for _, src := range []string{
		"| a | b |\n| - | - |\n| c | d |\n",
		"> | a | b |\n> | - | - |\n> | c | d |\n",
		"  | a | b |\n  | - | - |\n  | c | d |\n",
	} {
		got := rowSpans(src)
		if len(got) != len(want) {
			t.Errorf("%q rows = %d, want %d", src, len(got), len(want))
			continue
		}
		for i := range want {
			if got[i] != want[i] {
				t.Errorf("%q row %d = %v, want %v", src, i, got[i], want[i])
			}
		}
	}
}

// --- gfm:false ---

func TestGFMTablesDisabled(t *testing.T) {
	sources := []string{
		"| foo | bar |\n| --- | --- |\n| baz | bim |\n",
		"| abc | defghi |\n:-: | -----------:\nbar | baz\n",
		"| f\\|oo  |\n| ------ |\n| b `\\|` az |\n",
		"| abc | def |\n| --- | --- |\n",
		"aaa\n| a | b |\n| - | - |\n",
	}
	for _, src := range sources {
		out := ToHTML(src, cmOpts)
		if strings.Contains(out, "<table") {
			t.Errorf("%q: gfm:false produced a table: %q", src, out)
		}
		// Exactly what a paragraph of these lines renders as — the escaping and
		// the soft breaks included.
		if want := RenderHTML(Parse(src, cmOpts), cmOpts); out != want {
			t.Errorf("%q:\n got  %q\n want %q", src, out, want)
		}
		if !strings.HasPrefix(out, "<p>") {
			t.Errorf("%q: expected a paragraph, got %q", src, out)
		}
		for _, block := range ParseDocument(src, cmOpts)["children"].([]any) {
			if "table" == block.(map[string]any)["type"] {
				t.Errorf("%q: gfm:false produced a table node", src)
			}
		}
	}
	// The delimiter row is not even a paragraph break: it all stays one.
	if got, want := ToHTML("| a | b |\n| - | - |\n| c | d |\n", cmOpts),
		"<p>| a | b |\n| - | - |\n| c | d |</p>\n"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}

	// RenderHTML on a tree parsed with GFM off, spelled the way html.go
	// documents the canonical runtime's "renderHTML(tree) with no options":
	// the flavour follows the parse.
	off := Parse("| a |\n| - |\n", cmOpts)
	if got, want := RenderHTML(off, Options{GFM: off.GFM}), "<p>| a |\n| - |</p>\n"; got != want {
		t.Errorf("got %q, want %q", got, want)
	}
	on := Parse("| a |\n| - |\n", gfmOpts)
	if got := RenderHTML(on, Options{GFM: on.GFM}); !strings.HasPrefix(got, "<table>") {
		t.Errorf("got %q, want a table", got)
	}
}

// --- robustness -------------------------------------------------------------
//
// Nothing here may panic, and nothing may become super-linear: the inputs are
// adversarial, and untrusted input picks them.

func TestGFMTablesRobustness(t *testing.T) {
	// A 10000-column delimiter row.
	const cols = 10000
	src := "|" + strings.Repeat(" h |", cols) + "\n" +
		"|" + strings.Repeat(" --- |", cols) + "\n" +
		"|" + strings.Repeat(" b |", cols) + "\n"
	out := ToHTML(src, gfmOpts)
	if n := strings.Count(out, "<th>"); n != cols {
		t.Errorf("wide table: %d <th>, want %d", n, cols)
	}
	if n := strings.Count(out, "<td>"); n != cols {
		t.Errorf("wide table: %d <td>, want %d", n, cols)
	}
	table := ParseDocument(src, gfmOpts)["children"].([]any)[0].(map[string]any)
	if n := len(table["align"].([]any)); n != cols {
		t.Errorf("wide table: align has %d entries, want %d", n, cols)
	}
	if n := len(table["children"].([]any)[0].(map[string]any)["children"].([]any)); n != cols {
		t.Errorf("wide table: header row has %d cells, want %d", n, cols)
	}

	// 10000 body rows.
	const rows = 10000
	src = "| a | b |\n| - | - |\n" + strings.Repeat("| x | y |\n", rows)
	out = ToHTML(src, gfmOpts)
	if n := strings.Count(out, "<tr>"); n != rows+1 {
		t.Errorf("tall table: %d <tr>, want %d", n, rows+1)
	}
	table = ParseDocument(src, gfmOpts)["children"].([]any)[0].(map[string]any)
	if n := len(table["children"].([]any)); n != rows+1 {
		t.Errorf("tall table: %d rows, want %d", n, rows+1)
	}

	// A pipe repeated 50000 times. On its own it is one paragraph and nothing
	// more; as a header row over a matching delimiter row it is a very wide
	// table — n pipes with the leading and trailing one stripped is n-1 empty
	// cells.
	const n = 50000
	_ = ToHTML(strings.Repeat("|", n)+"\n", gfmOpts)
	_ = ToHTML(strings.Repeat("|", n)+"\n", cmOpts)
	out = ToHTML(strings.Repeat("|", n)+"\n|"+strings.Repeat(" - |", n-1)+"\n", gfmOpts)
	if c := strings.Count(out, "<th></th>"); c != n-1 {
		t.Errorf("pipe storm: %d empty cells, want %d", c, n-1)
	}
}

// TestGFMTablePaddingIsBounded pins maxAutocompletedCells.
//
// Padding short rows is the one shape whose node count is not bounded by the
// input: 10000 columns over 10000 one-cell rows asks for 10^8 cells. The
// autocomplete budget caps it, so quadrupling the rows must not grow the work.
func TestGFMTablePaddingIsBounded(t *testing.T) {
	header := "|" + strings.Repeat(" h |", 10000) + "\n" +
		"|" + strings.Repeat(" --- |", 10000) + "\n"
	measure := func(rows int) (time.Duration, int) {
		src := header + strings.Repeat("| x |\n", rows)
		start := time.Now()
		out := ToHTML(src, gfmOpts)
		return time.Since(start), len(out)
	}

	baseMs, baseLen := measure(5000)
	largeMs, largeLen := measure(20000)

	// The output is capped, not proportional to the row count.
	if ratio := float64(largeLen) / float64(baseLen); ratio >= 3 {
		t.Errorf("output grew %.1fx for 4x the rows", ratio)
	}
	if baseMs < 5*time.Millisecond {
		return // too fast to measure reliably
	}
	if ratio := float64(largeMs) / float64(baseMs); ratio >= 6 {
		t.Errorf("4x rows took %.1fx time (%v -> %v)", ratio, baseMs, largeMs)
	}
}

// TestGFMTableDelimiterRowIsLinear pins blockParser.lastAddedLine.
//
// Every second line is a syntactically valid delimiter row that fails the
// cell-count test, so the paragraph grows without bound and each line asks for
// its last line again. Reading that back out of the accumulated content —
// re-flattening a rope in the canonical runtime, scanning back for a newline
// here — is O(n) per line, and this is the input that shows it.
func TestGFMTableDelimiterRowIsLinear(t *testing.T) {
	measure := func(n int) time.Duration {
		src := strings.Repeat("| a | b |\n| --- |\n", n)
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
	if ratio := float64(large) / float64(small); ratio > 12 {
		t.Errorf("4x input took %.1fx time (%v -> %v); linear is ~4x, quadratic ~16x",
			ratio, small, large)
	}
}

func TestGFMTableDegenerateFragments(t *testing.T) {
	for _, src := range []string{
		"|", "||", "|||", "\\|", "|\\", "| - ", " - |", "|-|", ":", "::", ":-:", "-:",
		"a\n|", "a\n:", "a\n-:", "a\n|-", "a\n||", "a\n:-:|", "a|b\n-|-|-", "a\n\\|",
		"|a|\n|\\|", "a\n|\t-\t|", "|\n|", "| |\n| |", "a\n" + strings.Repeat("-", 10000),
		strings.Repeat("|", 200) + "\n" + strings.Repeat("-|", 200), "> a\n> |-|", "- a\n  |-|",
		"a\n---\n", "a\n- | -", "|a\n|-\n    x", "\\\\|\n-",
		// Byte-vs-rune: a delimiter row whose cells hold multi-byte characters,
		// and a header row that ends mid-rune's worth of pipes.
		"é|é\n-|-", "| 😀 |\n| - |\n| 😀 |", "—|—\n-|-",
	} {
		func() {
			defer func() {
				if r := recover(); r != nil {
					t.Errorf("panic on %q: %v", src, r)
				}
			}()
			for _, opts := range []Options{gfmOpts, cmOpts} {
				_ = ToHTML(src+"\n", opts)
				_ = ParseDocument(src+"\n", opts)
			}
		}()
	}
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

// TestGFMAutolinkBoundaryAtInlineNodeEdges covers offset 0 of a text node.
//
// The rule is about the SOURCE character immediately before the match, and the
// post-pass runs over a tree that no longer carries source offsets. At offset 0
// of a text node that character lives in the previous sibling, and the
// sibling's *type* names it exactly — so each row below states the type, the
// character it ends on, and whether an autolink may start after it.
//
// Rejecting offset 0 whenever there is any previous sibling is the obvious
// wrong fix: it breaks the four `true` rows in the middle of this table.
func TestGFMAutolinkBoundaryAtInlineNodeEdges(t *testing.T) {
	for _, c := range []struct {
		prevType string // previous sibling
		prefix   string // prefix that produces it
		lastChar string // its last source character
		links    bool
	}{
		{"(none)", "", "start of line", true},
		{"softbreak", "a\n", "start of line", true},
		{"linebreak", "a\\\n", "start of line", true},
		{"emph", "*a*", "*", true},
		{"strong", "**a**", "*", true},
		{"del", "~~a~~", "~", true},
		{"code", "`a`", "`", false},
		{"link", "[a](/u)", ")", false},
		{"image", "![a](/u)", ")", false},
		{"html_inline", "<b>", ">", false},
	} {
		t.Run(c.prevType, func(t *testing.T) {
			// The row's first column, asserted rather than assumed: with an
			// inert word in place of the URL, the paragraph ends in a text node
			// whose previous sibling is exactly the node this row names.
			para := Parse(c.prefix+"X\n", gfmOpts).FirstChild
			tail := para.LastChild
			if NodeText != tail.Type {
				t.Fatalf("tail type = %v, want text", tail.Type)
			}
			if "X" != tail.Literal {
				t.Fatalf("tail literal = %q, want %q", tail.Literal, "X")
			}
			gotPrev := "(none)"
			if nil != tail.Prev {
				gotPrev = string(tail.Prev.Type)
			}
			if gotPrev != c.prevType {
				t.Fatalf("previous sibling = %s, want %s", gotPrev, c.prevType)
			}

			out := ToHTML(c.prefix+"www.a.com\n", gfmOpts)
			if got := strings.Contains(out, `href="http://www.a.com"`); got != c.links {
				t.Errorf("after %s (source ends %s): linked = %v, want %v\n %q",
					c.prevType, c.lastChar, got, c.links, out)
			}
		})
	}
}

// TestGFMAutolinkAfterEmphasis calls out the emphasis case on its own because
// it is what the obvious wrong fix breaks, with its counterpart beside it.
func TestGFMAutolinkAfterEmphasis(t *testing.T) {
	check := func(src, want, why string) {
		t.Helper()
		if got := ToHTML(src, gfmOpts); got != want {
			t.Errorf("%s: %q\n got  %q\n want %q", why, src, got, want)
		}
	}
	check("*a*www.b.com\n", "<p><em>a</em>"+anchor("http://www.b.com", "www.b.com")+"</p>\n",
		"emphasis is still a delimiter an autolink may follow")
	check("`x`www.a.com\n", "<p><code>x</code>www.a.com</p>\n", "a code span is not")

	// The boundary applies to email addresses too.
	check("*a*b@c.de\n", "<p><em>a</em>"+anchor("mailto:b@c.de", "b@c.de")+"</p>\n", "email after emphasis")
	check("`x`b@c.de\n", "<p><code>x</code>b@c.de</p>\n", "email after a code span")
	check("[l](/u)b@c.de\n", "<p>"+anchor("/u", "l")+"b@c.de</p>\n", "email after a link")
}

// TestGFMAutolinkEmailLocalPartCap covers a local part of 65 characters or
// more, which is not recognised at all.
//
// The rewind that finds the start of a local part stops after 64 characters,
// which is what keeps this pass linear and is RFC 5321's own limit. Stopping
// there is a cap, not a boundary: when the character just outside it still
// belongs to the local part, the address is over-long and there is no address —
// linking the 64-character tail would invent one.
func TestGFMAutolinkEmailLocalPartCap(t *testing.T) {
	for _, c := range []struct {
		local  string
		linked bool
	}{
		{strings.Repeat("a", 63), true},
		{strings.Repeat("a", 64), true},
		{strings.Repeat("a", 65), false},
		{strings.Repeat("a", 100), false},
		// A leading `_` is itself a local-part character, so these are the same
		// four lengths shifted by one — 64 links, 65 does not.
		{"_" + strings.Repeat("a", 63), true},
		{"_" + strings.Repeat("a", 64), false},
		{"_" + strings.Repeat("a", 65), false},
		{"_" + strings.Repeat("a", 100), false},
	} {
		addr := c.local + "@b.co"
		got := ToHTML(addr+"\n", gfmOpts)
		want := "<p>" + addr + "</p>\n"
		if c.linked {
			want = "<p>" + anchor("mailto:"+addr, addr) + "</p>\n"
		}
		if got != want {
			t.Errorf("%d-character local part\n got  %q\n want %q", len(c.local), got, want)
		}
	}
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
