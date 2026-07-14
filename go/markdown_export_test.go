package tabnasmarkdown

import (
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// TestBuildMarkdownStringMatcherExported exercises the exported
// BuildMarkdownStringMatcher factory (TS: buildMarkdownStringMatcher)
// by registering the matcher on a plain jsonic instance and parsing a
// double-quote-escaped string.
func TestBuildMarkdownStringMatcherExported(t *testing.T) {
	j := jsonic.Make()
	j.SetOptions(jsonic.Options{Lex: &jsonic.LexOptions{
		Match: map[string]*jsonic.MatchSpec{
			"stringmarkdown": {Order: 1e5, Make: BuildMarkdownStringMatcher(map[string]any{
				"quote": `"`,
			})},
		},
	}})

	out, err := j.Parse(`"a""b"`)
	if err != nil {
		t.Fatalf("parse error: %v", err)
	}
	if got, want := out, `a"b`; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}
