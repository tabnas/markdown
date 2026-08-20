// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// Token COLUMNS after a non-ASCII character, in both engine phases.
//
// This plugin brings its own matchers, and a plugin that does owns the
// arithmetic the engine's matchers do for it: SI advances in BYTES and CI
// in RUNES. Both phases charged bytes: the block phase (engineblock.go)
// added `len(text)` on an unterminated final line, and the inline phase
// (engineinline.go) added `len(consumed)` — or `len(consumed) - lastNl`
// after a line ending. A 2-byte `é` cost two columns, a 3-byte `€` three.
//
// The TypeScript half writes `text.length` and `consumed.length` in the
// same expressions over UTF-16 indices, where they DO count characters.
// The two lines look like the same line, which is why this survived port
// review: a transliteration is not a port when the two languages index
// strings differently.
//
// ts/test/engine-columns.test.ts asserts the same two tables. The astral
// rows are the only ones where the answers differ, and that is the
// recorded engine divergence — TypeScript counts UTF-16 units (an astral
// character is 2), Go counts runes (1). See parser/DIVERGENCE.md,
// "Column positions for astral characters".
//
// Found by the fleet parity probe; the same defect was repaired in
// tabnas/toml, tabnas/xml and tabnas/yaml.

import (
	"fmt"
	"strings"
	"testing"

	parser "github.com/tabnas/parser/go"
)

// lexColumns drives a token stream to its end and renders each token as
// `name@row:col`, which is the whole observable surface of the column
// arithmetic: neither phase puts these positions in the AST, so a pin that
// went through Parse would assert nothing about them.
func lexColumns(lex *parser.Lex) string {
	var out []string
	for i := 0; i < 32; i++ {
		tk := lex.Next()
		if tk == nil {
			break
		}
		out = append(out, fmt.Sprintf("%s@%d:%d", tk.Name, tk.RI, tk.CI))
		if tk.Name == "#ZZ" || tk.Name == "#BD" {
			break
		}
	}
	return strings.Join(out, " ")
}

func TestBlockColumnsCountRunesNotBytes(t *testing.T) {
	// Every case omits the trailing newline on purpose. A source that ends
	// in one takes the terminated branch, which resets CI to 1 and hides
	// the arithmetic — the reason this defect was first written off as
	// having no diagnostic surface.
	for _, c := range []struct {
		label string
		src   string
		want  string
		ts    string // what the TypeScript half asserts, for the reader
	}{
		// Controls. Pure ASCII, and a source that DOES end in a newline:
		// without them, "columns count characters" is also satisfied by
		// never counting.
		{"ascii-nl", "# xx\n", "#LB@1:1 #ZZ@2:1", "#LB@1:1 #ZZ@2:1"},
		{"ascii-nonl", "# xx", "#LB@1:1 #ZZ@1:5", "#LB@1:1 #ZZ@1:5"},

		// 2 and 3 bytes, 1 rune, 1 UTF-16 unit: both ports agree.
		{"latin1", "# é", "#LB@1:1 #ZZ@1:4", "#LB@1:1 #ZZ@1:4"},
		{"bmp", "# €", "#LB@1:1 #ZZ@1:4", "#LB@1:1 #ZZ@1:4"},
		{"para-latin1", "é text", "#LB@1:1 #ZZ@1:7", "#LB@1:1 #ZZ@1:7"},
		{"para-bmp", "€€ text", "#LB@1:1 #ZZ@1:8", "#LB@1:1 #ZZ@1:8"},
		{"emph-latin1", "*é* x", "#LB@1:1 #ZZ@1:6", "#LB@1:1 #ZZ@1:6"},
		{"link-bmp", "[€](u) x", "#LB@1:1 #ZZ@1:9", "#LB@1:1 #ZZ@1:9"},
		{"code-latin1", "`é` x", "#LB@1:1 #ZZ@1:6", "#LB@1:1 #ZZ@1:6"},
		{"html-latin1", "<b>é</b> x", "#LB@1:1 #ZZ@1:11", "#LB@1:1 #ZZ@1:11"},

		// A terminated line followed by an unterminated one: the row
		// advances on the first and the column counts on the second.
		{"multiline-latin1", "é a\nb c", "#LB@1:1 #LB@2:1 #ZZ@2:4", "#LB@1:1 #LB@2:1 #ZZ@2:4"},

		// 4 bytes, 1 rune, TWO UTF-16 units: the recorded divergence, and
		// the only block row where the two halves differ.
		{"astral", "# \U0001F600", "#LB@1:1 #ZZ@1:4", "#LB@1:1 #ZZ@1:5"},
	} {
		j := parser.Make(parser.Options{})
		if err := j.Use(Markdown, nil); err != nil {
			t.Fatalf("%s: use: %v", c.label, err)
		}
		got := lexColumns(parser.MakeLex(c.src, j.Config()))
		if got != c.want {
			t.Errorf("%s: %q\n got %s\nwant %s", c.label, c.src, got, c.want)
		}
	}
}

func TestInlineColumnsCountRunesNotBytes(t *testing.T) {
	// The inline phase is a second engine instance over one block's
	// content, and its positions never reach the AST — parseInlinesEngine
	// discards them. Driving its lexer directly is the level at which the
	// arithmetic is observable at all, so it is the level the pin asserts.
	opts := ResolveOptions(nil)
	for _, c := range []struct {
		label   string
		subject string
		want    string
		ts      string // what the TypeScript half asserts, for the reader
	}{
		// Control: pure ASCII.
		{"ascii", "a b c", "#ITX@1:1 #ZZ@1:6", "#ITX@1:1 #ZZ@1:6"},

		// 2 and 3 bytes, 1 rune, 1 UTF-16 unit: both ports agree.
		{"latin1", "é text", "#ITX@1:1 #ZZ@1:7", "#ITX@1:1 #ZZ@1:7"},
		{"bmp", "€€ text", "#ITX@1:1 #ZZ@1:8", "#ITX@1:1 #ZZ@1:8"},
		{"emph-latin1", "*é* x",
			"#IDL@1:1 #ITX@1:2 #IDL@1:3 #ITX@1:4 #ZZ@1:6",
			"#IDL@1:1 #ITX@1:2 #IDL@1:3 #ITX@1:4 #ZZ@1:6"},
		{"code-latin1", "`é` x",
			"#ICS@1:1 #ITX@1:4 #ZZ@1:6",
			"#ICS@1:1 #ITX@1:4 #ZZ@1:6"},
		{"link-bmp", "[€](u) x",
			"#IOB@1:1 #ITX@1:2 #ICB@1:3 #ITX@1:7 #ZZ@1:9",
			"#IOB@1:1 #ITX@1:2 #ICB@1:3 #ITX@1:7 #ZZ@1:9"},

		// The `newlines > 0` branch, which resets the column against the
		// last line ending rather than accumulating. A multiline code span
		// is the shortest input that reaches it; without these two rows
		// that branch has no reproduction and the pin proves nothing
		// about half the change.
		{"codespan-multiline-latin1", "`é\né` tail é",
			"#ICS@1:1 #ITX@2:3 #ZZ@2:10",
			"#ICS@1:1 #ITX@2:3 #ZZ@2:10"},
		{"codespan-multiline-bmp", "`€€\nx` tail €",
			"#ICS@1:1 #ITX@2:3 #ZZ@2:10",
			"#ICS@1:1 #ITX@2:3 #ZZ@2:10"},

		// 4 bytes, 1 rune, TWO UTF-16 units: the recorded divergence, on
		// both branches.
		{"astral", "\U0001F600 text", "#ITX@1:1 #ZZ@1:7", "#ITX@1:1 #ZZ@1:8"},
		{"softbreak-astral", "\U0001F600 a\n\U0001F600 b",
			"#ITX@1:1 #IBK@1:4 #ITX@2:1 #ZZ@2:4",
			"#ITX@1:1 #IBK@1:5 #ITX@2:1 #ZZ@2:5"},
	} {
		tn := makeInlineTn(opts)
		p := &inlineParser{refmap: RefMap{}, options: opts}
		p.subject = c.subject
		p.pos = 0
		lex := parser.MakeLex(c.subject, tn.Config())
		lex.Ctx = &parser.Context{U: map[string]any{
			"inl": &inlineState{parser: p, block: &MdNode{Type: NodeParagraph}},
		}}
		got := lexColumns(lex)
		if got != c.want {
			t.Errorf("%s: %q\n got %s\nwant %s", c.label, c.subject, got, c.want)
		}
	}
}
