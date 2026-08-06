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
