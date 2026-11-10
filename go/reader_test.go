// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// reader_test.go — a parse from a reader (Tabnas.ParseReader, admin ADR-30).
//
// Make declares Lex.Window: the one matcher cuts a line through the lexer's
// window and fills it past the line's end, and nothing else reads the
// source, so a reader is read a window at a time. Every row of the shared
// fixtures and every document of both conformance corpora parses the same
// from a reader, handed over a few bytes at a time, as from its text. Go's
// small-window test hook is not exported, so the window's edge is met by a
// long document instead: lines of every kind, the long ones included, each
// placed across a multiple of 64 KiB, read in pieces of several sizes. A long
// document from a reader parses in time linear in its length.
// ts/test/reader.test.ts and rs/tests/reader_test.rs hold the other
// runtimes to the same.

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"testing"
	"time"

	parser "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

// window64 is the window's margin, 64 KiB: a read of the source reaches past
// the cursor by this much.
const window64 = 64 * 1024

type readerPieces struct {
	bytes    []byte
	at, size int
}

func (p *readerPieces) Read(buf []byte) (int, error) {
	if p.at == len(p.bytes) {
		return 0, io.EOF
	}
	n := min(p.size, len(buf), len(p.bytes)-p.at)
	copy(buf, p.bytes[p.at:p.at+n])
	p.at += n
	return n, nil
}

func inPieces(text string, size int) io.Reader {
	return &readerPieces{bytes: []byte(text), size: size}
}

// readOutcome is a parse's value as JSON text, or its failure.
func readOutcome(t *testing.T, value any, err error) string {
	t.Helper()
	if err != nil {
		return "err " + err.Error()
	}
	raw, merr := json.Marshal(value)
	if merr != nil {
		t.Fatalf("marshal: %v", merr)
	}
	return "ok " + string(raw)
}

func TestReaderMakeReadsAReaderAWindowAtATime(t *testing.T) {
	if !Make().Config().Window {
		t.Fatal("Make does not declare Lex.Window")
	}
	if !Make(map[string]any{"gfm": false}).Config().Window {
		t.Fatal("Make with options does not declare Lex.Window")
	}
	// The plugin leaves the window to the parser that installs it.
	j := parser.Make()
	if err := j.Use(Markdown, nil); err != nil {
		t.Fatal(err)
	}
	if j.Config().Window {
		t.Fatal("the plugin declares Lex.Window")
	}
}

func TestReaderEveryRowParsesTheSameFromAReader(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	compared := 0
	for _, file := range files {
		for _, row := range file.Rows {
			opts := map[string]any{}
			if raw := row.Named("opts"); "" != raw {
				if err := json.Unmarshal([]byte(raw), &opts); err != nil {
					t.Fatal(err)
				}
			}
			j := Make(opts)
			input := row.UnescNamed("input")
			value, err := j.Parse(input)
			whole := readOutcome(t, value, err)
			for _, size := range []int{1, 3, 64} {
				value, err := j.ParseReader(inPieces(input, size))
				if read := readOutcome(t, value, err); read != whole {
					t.Fatalf("%s in pieces of %d:\n  read  %s\n  whole %s", row.Where(), size, read, whole)
				}
				compared++
			}
		}
	}
	if compared < 300 {
		t.Fatalf("only %d parses compared", compared)
	}
}

func TestReaderEveryCorpusDocumentParsesTheSameFromAReader(t *testing.T) {
	for _, corpus := range []struct {
		name  string
		cases []specCase
		opts  map[string]any
	}{
		{"commonmark", loadSpecCases(t), map[string]any{"gfm": false}},
		{"gfm", loadGFMCases(t), nil},
	} {
		j := Make(corpus.opts)
		for i, c := range corpus.cases {
			value, err := j.Parse(c.Markdown)
			whole := readOutcome(t, value, err)
			for _, size := range []int{1, 3, 64} {
				value, err := j.ParseReader(inPieces(c.Markdown, size))
				if read := readOutcome(t, value, err); read != whole {
					t.Fatalf("%s example %d in pieces of %d:\n  read  %s\n  whole %s",
						corpus.name, c.Example, size, read, whole)
				}
			}
			if 0 == (i+1)%200 || i+1 == len(corpus.cases) {
				t.Logf("reader %s: %d of %d (%d%%)", corpus.name, i+1, len(corpus.cases), (i+1)*100/len(corpus.cases))
			}
		}
	}
}

// straddlers are the lines placed across the window's boundaries, by kind.
// The long ones are longer than the window's margin.
var straddlers = []struct{ kind, lines string }{
	{"a paragraph's line", "some *text* and a [ref] with `code` ñ€𝄞\n"},
	{"a line ending in CR LF", "a line ending in CR LF\r\n"},
	{"a line ending in CR", "a line ending in CR\r"},
	{"characters past ASCII", "ññññ€€€€𝄞𝄞𝄞𝄞ññññ€€€€𝄞𝄞𝄞𝄞\n"},
	{"a heading", "## A heading ñ €\n"},
	{"a block quote", "> quoted *a* [b]\n> more\n"},
	{"a list item", "- item one\n  - nested ñ\n"},
	{"a table", "| a | b |\n| - | :-: |\n| ñ | € |\n"},
	{"a fence", "```js\nlet x = 'ñ€𝄞'\n```\n"},
	{"blank lines", "\n\n\r\n\r\n"},
	{"a long line", strings.Repeat("ab é€𝄞 cd ", 9000) + "*x* [r]\n"},
	{"a long code block", "```\n" + strings.Repeat("code ñ€𝄞 line\n", 6000) + "```\n"},
	{"a long table row", "| a | b |\n| - | - |\n| " + strings.Repeat("cell ñ € ", 9000) + " | c |\n"},
	{"long blank lines", strings.Repeat("\n\r\n", 30000)},
}

// straddling is a long document in which a line of every kind spans a
// multiple of 64 KiB: each kind in turn is placed across the next
// multiple, its middle on it, with paragraphs of every kind of inline
// between, each kind a block of its own. Positions are bytes.
func straddling(t *testing.T) string {
	t.Helper()
	var b strings.Builder
	record := 0
	next := func() string {
		record++
		return fmt.Sprintf("Para %d *em* **st** `c` [l](/u) [r] ñ€𝄞\n\n", record)
	}
	for _, s := range straddlers {
		half := len(s.lines) / 2
		boundary := window64 * ((b.Len() + half + 4 + window64 - 1) / window64)
		for r := next(); b.Len()+len(r)+half+2 < boundary; r = next() {
			b.WriteString(r)
		}
		// Blank lines up to where the kind's middle falls on the multiple.
		for b.Len()+half < boundary {
			b.WriteByte('\n')
		}
		start := b.Len()
		b.WriteString(s.lines)
		if !(start < boundary && boundary < b.Len()) {
			t.Fatalf("%s: %d..%d does not span %d", s.kind, start, b.Len(), boundary)
		}
		b.WriteString("\n")
	}
	b.WriteString(next() + "[r]: /u \"t\"\n")
	return b.String()
}

func TestReaderLinesAcrossTheWindowsBoundariesParseTheSameFromAReader(t *testing.T) {
	doc := straddling(t)
	if len(doc) < 2*window64 {
		t.Fatalf("the document is %d bytes, under two windows", len(doc))
	}
	j := Make()
	value, err := j.Parse(doc)
	whole := readOutcome(t, value, err)
	if !strings.HasPrefix(whole, "ok ") {
		t.Fatalf("whole: %.80s", whole)
	}
	// Pieces of 64 and 4 KiB, and of 1 MiB, which a read takes 64 KiB at a
	// time, put a read's end on every multiple; 64 KiB and 7 does not.
	for _, size := range []int{64, 4096, window64 + 7, 1 << 20} {
		value, err := j.ParseReader(inPieces(doc, size))
		if read := readOutcome(t, value, err); read != whole {
			t.Fatalf("in pieces of %d: the read differs from the whole", size)
		}
		t.Logf("reader straddling: pieces of %d the same", size)
	}
}

func TestReaderALongDocumentParsesInLinearTime(t *testing.T) {
	unit := "# Heading\n\nSome *text* and a [ref] with `code`.\n\n" +
		"- a\n- b\n  > quoted\n\n" +
		"| x | y |\n| - | - |\n| 1 | 2 |\n\n" +
		"```js\nlet x = 1\n```\n\n[ref]: /u \"t\"\n\n"
	j := Make()
	timed := func(n int) time.Duration {
		doc := strings.Repeat(unit, n)
		start := time.Now()
		if _, err := j.ParseReader(inPieces(doc, 4096)); err != nil {
			t.Fatal(err)
		}
		return time.Since(start)
	}
	timed(100) // warm
	small := timed(1000)
	large := timed(10000)
	t.Logf("reader: 1000 units %v, 10000 units %v", small, large)
	// Ten times the document, about ten times the time; well under the
	// hundred a quadratic parse would take.
	if large > 30*small {
		t.Fatalf("1000 units %v, 10000 units %v", small, large)
	}
}
