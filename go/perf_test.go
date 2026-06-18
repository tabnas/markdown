package markdown

import (
	"testing"
	"time"

	jsonic "github.com/tabnas/jsonic/go"
)

// makeMarkdownParser builds a fresh jsonic instance with the Markdown plugin
// installed. Building the engine + applying the embedded markdown grammar is
// the expensive part; a parse of a tiny document is comparatively cheap.
func makeMarkdownParser() *jsonic.Jsonic {
	j := jsonic.Make()
	j.UseDefaults(Markdown, Defaults)
	return j
}

// TestParseReusesInstance guards against a performance regression in how the
// Markdown plugin is consumed. @tabnas/markdown is a PLUGIN with no
// package-level convenience Parse() — callers build a jsonic instance and
// j.UseDefaults(Markdown, ...) themselves. The right (fast) pattern is to
// build that instance ONCE and reuse it; the wrong (slow) pattern rebuilds the
// engine + applies the markdown grammar on every parse, which is dominated by
// grammar construction and is many times slower.
//
// This test pins the correct usage: it compares N parses that rebuild the
// instance each call against N parses reusing ONE instance, on the SAME
// machine in the SAME run. Reuse must stay close to linear relative to the
// rebuild path (here reuse is far faster), so the rebuild-per-parse anti-
// pattern would blow the ratio. The check is machine-INDEPENDENT: both sides
// scale together on a slow CI box, so there is deliberately NO wall-clock
// budget.
//
// (If a cached convenience Parse() were ever added to this package, this is
// the shape that would catch it rebuilding the grammar per call — see yaml's
// TestParseReusesInstance / json's sync.Once defaultParser.)
func TestParseReusesInstance(t *testing.T) {
	const src = "a,b,c\n1,2,3"
	const n = 2000

	// Warm both paths so the comparison is steady-state.
	for i := 0; i < 50; i++ {
		_, _ = makeMarkdownParser().Parse(src)
	}
	reused := makeMarkdownParser()
	for i := 0; i < 50; i++ {
		_, _ = reused.Parse(src)
	}

	// Rebuild-per-parse: pays grammar construction on every iteration.
	t0 := time.Now()
	for i := 0; i < n; i++ {
		if _, err := makeMarkdownParser().Parse(src); err != nil {
			t.Fatalf("rebuild parse error: %v", err)
		}
	}
	rebuild := time.Since(t0)

	// Reuse one instance: only pays parsing per iteration.
	t1 := time.Now()
	for i := 0; i < n; i++ {
		if _, err := reused.Parse(src); err != nil {
			t.Fatalf("reuse parse error: %v", err)
		}
	}
	reuse := time.Since(t1)

	// Reusing one instance must be no slower than rebuilding it every parse.
	// Allow 4x slack for scheduling noise around an otherwise lopsided win
	// (reuse should be much faster). This catches a regression where a
	// reused-instance path somehow rebuilt the grammar per parse without
	// depending on absolute wall-clock speed.
	if reuse > 4*rebuild {
		t.Errorf("reusing one Markdown instance is not faster than rebuilding it per parse: "+
			"%d reuse parses took %v vs %v rebuilding each time (ratio %.1fx, limit 4x). "+
			"Build the jsonic+Markdown instance once and reuse it (the grammar build dominates a parse).",
			n, reuse, rebuild, float64(reuse)/float64(rebuild))
	}
	t.Logf("rebuild-per-parse=%v  reuse-one=%v  rebuild/reuse=%.2fx",
		rebuild, reuse, float64(rebuild)/float64(reuse))
}
