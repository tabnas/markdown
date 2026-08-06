/* Copyright (c) 2025-2026 Richard Rodger and other contributors, MIT License */

package tabnasmarkdown

// Adversarial input. Markdown is frequently parsed from untrusted sources, so
// nothing here may panic, hang, or take super-linear time.
//
// Two of these guard specific defects the previous implementation had:
//
//   - A code fence of >1000 characters panicked, because the closing-fence
//     regex was built by interpolating the fence length into a `{n,}` bounded
//     repeat, and RE2 caps those at 1000. Untrusted input could abort the host
//     process. Fence runs are counted with a loop now.
//   - Non-ASCII text was corrupted because the inline scanner appended
//     `string(text[i])` — a *byte* — so every UTF-8 continuation byte widened
//     into its own code point. TestNonASCII covers the round trip.

import (
	"strings"
	"testing"
	"time"
)

func TestNoPanicOnAdversarialInput(t *testing.T) {
	cases := []struct {
		name string
		in   string
	}{
		{"long backtick fence", strings.Repeat("`", 2000) + "\nx\n"},
		{"long tilde fence", strings.Repeat("~", 2000) + "\nx\n"},
		{"fence at rune boundary", "```" + strings.Repeat("é", 2000) + "\nx\n"},
		{"deep brackets", strings.Repeat("[", 20000) + "a" + strings.Repeat("]", 20000)},
		{"delimiter run", strings.Repeat("*", 5000) + "x" + strings.Repeat("*", 5000)},
		{"unclosed backticks", strings.Repeat("`", 5000) + "x"},
		{"deep quotes", strings.Repeat(">", 2000) + " x"},
		{"unclosed autolink", "<" + strings.Repeat("a", 50000)},
		{"entity storm", strings.Repeat("&amp;", 20000)},
		{"setext storm", strings.Repeat("a\n=\n", 5000)},
		{"unbalanced link parens", strings.Repeat("[a](b", 10000)},
		{"nul bytes", "a\x00b\x00c"},
		{"invalid utf8", "a\xff\xfeb"},
		{"lone continuation byte", "a\x80b"},
		{"crlf mix", "a\r\nb\rc\nd"},
		{"empty", ""},
		{"only newlines", "\n\n\n\n"},
		{"tabs everywhere", strings.Repeat("\ta\tb\n", 5000)},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			done := make(chan struct{})
			go func() {
				defer func() {
					if r := recover(); r != nil {
						t.Errorf("panic: %v", r)
					}
					close(done)
				}()
				for _, gfm := range []bool{true, false} {
					opts := Options{GFM: gfm}
					_ = ToHTML(c.in, opts)
					_ = ParseDocument(c.in, opts)
				}
			}()
			select {
			case <-done:
			case <-time.After(30 * time.Second):
				t.Fatal("timed out — likely super-linear")
			}
		})
	}
}

// TestLinkDestinationIsNotQuadratic pins the paren-nesting bound in
// parseLinkDestination. Without it, an unbalanced `[a](b` run makes every `]`
// scan to end of input: 4x the input cost ~16x the time.
func TestLinkDestinationIsNotQuadratic(t *testing.T) {
	measure := func(n int) time.Duration {
		src := strings.Repeat("[a](b", n)
		start := time.Now()
		_ = Parse(src, Options{})
		return time.Since(start)
	}

	// Warm up, then compare a 4x size step against a generous linear bound.
	measure(1000)
	small := measure(5000)
	large := measure(20000)

	if small < time.Millisecond {
		small = time.Millisecond
	}
	if ratio := float64(large) / float64(small); ratio > 10 {
		t.Errorf("4x input took %.1fx time (%v -> %v); expected roughly linear",
			ratio, small, large)
	}
}

// TestNestedContainersAreNotQuadratic pins the sourcepos column anchor in
// block.go. A sourcepos column counts characters, but this port's offsets are
// byte indices, so addChild has to convert — and counting from the start of the
// line each time made a line of n nested containers cost O(n²), where the
// canonical runtime is linear. `> - > - > - …` is cheap to write and untrusted
// input decides n, so this is a denial-of-service bound, not a micro-benchmark.
func TestNestedContainersAreNotQuadratic(t *testing.T) {
	measure := func(n int) time.Duration {
		src := strings.Repeat("> - ", n) + "x"
		start := time.Now()
		_ = Parse(src, Options{})
		return time.Since(start)
	}

	// Warm up, then compare a 4x size step against a generous linear bound.
	measure(1000)
	small := measure(5000)
	large := measure(20000)

	if small < time.Millisecond {
		small = time.Millisecond
	}
	if ratio := float64(large) / float64(small); ratio > 10 {
		t.Errorf("4x input took %.1fx time (%v -> %v); expected roughly linear",
			ratio, small, large)
	}
}

func TestNonASCII(t *testing.T) {
	for _, c := range []struct{ in, want string }{
		{"café naïve *ém*\n", "<p>café naïve <em>ém</em></p>\n"},
		{"**日本語**\n", "<p><strong>日本語</strong></p>\n"},
		{"[link](/ü)\n", "<p><a href=\"/%C3%BC\">link</a></p>\n"},
		{"*héllo* wörld 🎉\n", "<p><em>héllo</em> wörld 🎉</p>\n"},
		// Emphasis flanking is decided on Unicode character classes, so it must
		// see runes and not bytes.
		{"*«a»*\n", "<p><em>«a»</em></p>\n"},
	} {
		if got := ToHTML(c.in, Options{GFM: true}); got != c.want {
			t.Errorf("in %q\n got  %q\n want %q", c.in, got, c.want)
		}
	}
}

// TestSourcePosColumnsCountCharacters pins the unit of a sourcepos column.
//
// Columns count characters. Go offsets are byte indices and the canonical
// runtime's are UTF-16 indices, so both runtimes have to convert, and both
// got it wrong in opposite directions at first: `😀 *x*` reported column 5
// here and 6 there. The AST carries no sourcepos, so the AST-level parity
// check between the runtimes cannot catch a regression in this.
func TestSourcePosColumnsCountCharacters(t *testing.T) {
	endColumn := func(src string) int {
		w := Parse(src, Options{}).Walker()
		for e := w.Next(); e != nil; e = w.Next() {
			if e.Entering && e.Node.Type == NodeParagraph {
				return e.Node.SourcePos[1][1]
			}
		}
		return -1
	}

	for _, c := range []struct {
		in   string
		want int
		why  string
	}{
		{"abc *x*\n", 7, "ascii"},
		{"é *x*\n", 5, "BMP: two bytes, one character"},
		{"😀 *x*\n", 5, "astral: four bytes, still one character"},
		{"𝔘𝔘 *x*\n", 6, "two astral characters"},
	} {
		if got := endColumn(c.in); got != c.want {
			t.Errorf("%s: %q end column = %d, want %d", c.why, c.in, got, c.want)
		}
	}
}

// TestDestinationsDecodedInAST pins where percent-encoding happens.
//
// normalizeURI belongs to rendering. Applying it at parse time was invisible
// in the HTML — it is idempotent over its own output, so conformance stayed at
// 652/652 — but it put "/%C3%A4" in the public AST where mdast, and every
// previous release, promises "/ä". No .tsv fixture held a non-ASCII URL, so
// nothing caught it; both runtimes were wrong in exactly the same way, which
// also hid it from the cross-runtime comparison.
func TestDestinationsDecodedInAST(t *testing.T) {
	first := func(src string) map[string]any {
		doc := ParseDocument(src, Options{})
		para := doc["children"].([]any)[0].(map[string]any)
		return para["children"].([]any)[0].(map[string]any)
	}

	for _, c := range []struct{ in, wantURL, wantHTML string }{
		{"[l](/ä)", "/ä", "<p><a href=\"/%C3%A4\">l</a></p>\n"},
		{"![i](/ö)", "/ö", "<p><img src=\"/%C3%B6\" alt=\"i\" /></p>\n"},
		{"<https://x.com/ä>", "https://x.com/ä", "<p><a href=\"https://x.com/%C3%A4\">https://x.com/ä</a></p>\n"},
		// Already encoded: left alone, not double-encoded.
		{"[l](/a%20b)", "/a%20b", "<p><a href=\"/a%20b\">l</a></p>\n"},
	} {
		if got := first(c.in)["url"]; got != c.wantURL {
			t.Errorf("%q: AST url = %q, want %q", c.in, got, c.wantURL)
		}
		if got := ToHTML(c.in, Options{}); got != c.wantHTML {
			t.Errorf("%q: HTML = %q, want %q", c.in, got, c.wantHTML)
		}
	}
}
