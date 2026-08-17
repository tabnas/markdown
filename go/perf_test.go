package tabnasmarkdown

import (
	"strings"
	"testing"
	"time"

	parser "github.com/tabnas/parser/go"
)

// makeMarkdownParser builds a fresh engine instance with the Markdown plugin
// installed. Building the engine + applying the embedded markdown grammar is
// the expensive part; a parse of a tiny document is comparatively cheap.
func makeMarkdownParser() *parser.Tabnas {
	j := parser.Make()
	j.UseDefaults(Markdown, Defaults)
	return j
}

// TestParseReusesInstance guards against a performance regression in how the
// Markdown plugin is consumed. @tabnas/markdown is a PLUGIN with no
// package-level convenience Parse() — callers build an engine instance and
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
	const src = "# Title\n\npara with *em* and `code`\n\n- item one\n- item two"
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
			"Build the engine+Markdown instance once and reuse it (the grammar build dominates a parse).",
			n, reuse, rebuild, float64(reuse)/float64(rebuild))
	}
	t.Logf("rebuild-per-parse=%v  reuse-one=%v  rebuild/reuse=%.2fx",
		rebuild, reuse, float64(rebuild)/float64(reuse))
}

// TestEngineBlockLinearity pins the engine block driver's linearity: the
// plugin path lexes one #LB token per line and feeds it to the shared
// incorporateLine, so doubling the document must roughly double the parse
// time. A regression to per-token re-parsing (the pre-driver wiring ran the
// whole engine-free parse once per drained token) blows this ratio
// immediately. Per dx-report §26's lesson the assertion is a same-run ratio,
// never a wall-clock budget, and the slack is generous: linear is ~2x,
// quadratic is ~4x on the doubling alone and far worse in practice.
func TestEngineBlockLinearity(t *testing.T) {
	j := makeMarkdownParser()

	mk := func(n int) string {
		var b strings.Builder
		for i := 0; i < n; i++ {
			b.WriteString("para *em* `c` line\n\n- item [x] here\n> quote | cell\n\n")
		}
		return b.String()
	}
	small, large := mk(400), mk(800)

	// Warm both sizes so the comparison is steady-state.
	for i := 0; i < 3; i++ {
		if _, err := j.Parse(small); err != nil {
			t.Fatal(err)
		}
		if _, err := j.Parse(large); err != nil {
			t.Fatal(err)
		}
	}

	const iters = 5
	t0 := time.Now()
	for i := 0; i < iters; i++ {
		if _, err := j.Parse(small); err != nil {
			t.Fatal(err)
		}
	}
	dSmall := time.Since(t0)

	t1 := time.Now()
	for i := 0; i < iters; i++ {
		if _, err := j.Parse(large); err != nil {
			t.Fatal(err)
		}
	}
	dLarge := time.Since(t1)

	ratio := float64(dLarge) / float64(dSmall)
	if ratio > 6 {
		t.Errorf("engine block path is not linear: doubling the document made the parse %.1fx slower (limit 6x)", ratio)
	}
	t.Logf("small=%v large=%v large/small=%.2fx", dSmall, dLarge, ratio)
}
