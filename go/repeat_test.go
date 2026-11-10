// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// repeat_test.go — every repetition is a replace loop, never a push chain
// (alchemy's AGENTS.md, "The reader repeats by replacement").
//
// A container's children are the children rule, which goes on by
// replacement (R) from one child to the next, so a document of ten thousand
// blocks, or a list of ten thousand items, or a paragraph of ten thousand
// inline nodes, runs at the rule depth one needs. Real nesting still nests:
// a block quote inside a block quote is a level deeper. The rule depth is
// the engine's D, read by a rule subscriber on every step of the parse.
// Mirrors ts/test/repeat.test.ts.

import (
	"strings"
	"testing"

	parser "github.com/tabnas/parser/go"
)

// maxDepth is the deepest rule a parse of src reaches.
func maxDepth(t *testing.T, src string) int {
	t.Helper()
	max := 0
	j := Make()
	j.Sub(nil, func(rule *parser.Rule, _ *parser.Context) {
		if rule.D > max {
			max = rule.D
		}
	})
	if _, err := j.Parse(src); err != nil {
		t.Fatalf("parse: %v", err)
	}
	return max
}

func TestRepeatTenThousandReachTheDepthOneDoes(t *testing.T) {
	const n = 10000
	for _, c := range []struct {
		what string
		one  string
		many string
	}{
		{"paragraphs", "a\n", strings.Repeat("a\n\n", n)},
		{"ATX headings", "# h\n", strings.Repeat("# h\n", n)},
		{"setext headings", "h\n=\n", strings.Repeat("h\n=\n\n", n)},
		{"thematic breaks", "***\n", strings.Repeat("***\n", n)},
		{"fenced code blocks", "```\nx\n```\n", strings.Repeat("```\nx\n```\n", n)},
		{"indented code blocks", "    x\n\na\n", strings.Repeat("    x\n\na\n\n", n)},
		{"HTML blocks", "<div>\n</div>\n", strings.Repeat("<div>\n</div>\n\n", n)},
		{"definitions", "[a]: /u\n", strings.Repeat("[a]: /u\n", n)},
		{"list items", "- a\n", strings.Repeat("- a\n", n)},
		{"loose list items", "- a\n", strings.Repeat("- a\n\n", n)},
		{"ordered list items", "1. a\n", strings.Repeat("1. a\n", n)},
		{"paragraphs in a block quote", "> a\n", strings.Repeat("> a\n>\n", n)},
		{"lines of a paragraph", "a\n", strings.Repeat("a\n", n)},
		{"lines of a code block", "```\nx\n```\n", "```\n" + strings.Repeat("x\n", n) + "```\n"},
		{"table rows", "| a |\n| - |\n| b |\n", "| a |\n| - |\n" + strings.Repeat("| b |\n", n)},
		{"table cells", "| a |\n| - |\n", strings.Repeat("| a ", n) + "|\n" + strings.Repeat("| - ", n) + "|\n"},
		{"emphasis in a paragraph", "*a*\n", strings.Repeat("*a* ", n) + "\n"},
		{"links in a paragraph", "[a](/u)\n", strings.Repeat("[a](/u) ", n) + "\n"},
		{"references in a paragraph", "[a]\n", strings.Repeat("[a] ", n) + "\n"},
		{"code spans in a paragraph", "`a`\n", strings.Repeat("`a` ", n) + "\n"},
		{"hard breaks in a paragraph", "a  \nb\n", strings.Repeat("a  \n", n) + "b\n"},
	} {
		d1 := maxDepth(t, c.one)
		dn := maxDepth(t, c.many)
		if d1 <= 0 || dn != d1 {
			t.Errorf("%s: one reaches %d, %d reach %d", c.what, d1, n, dn)
		}
	}
}

func TestRepeatNestingNests(t *testing.T) {
	// A block quote is a rule and its children's loop, two levels a quote.
	d := func(n int) int { return maxDepth(t, strings.Repeat("> ", n)+"a\n") }
	if got := d(2) - d(1); got != 2 {
		t.Errorf("a second block quote is %d levels deeper, not 2", got)
	}
	if got := d(100) - d(1); got != 2*99 {
		t.Errorf("a hundred block quotes are %d levels deeper, not %d", got, 2*99)
	}
	e := func(n int) int {
		return maxDepth(t, strings.Repeat("*", n)+"a"+strings.Repeat("*", n)+"\n")
	}
	if !(e(3) > e(1)) {
		t.Errorf("emphasis in strong is no deeper than emphasis")
	}
}
