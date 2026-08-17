// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// engine_conformance_test.go — engine-path conformance: the full CommonMark
// 0.31.2 suite (652 examples, gfm:false) and the GFM extension corpus (24
// examples, gfm:true) run through Make(...).ParseMeta(...) — the engine
// lexing lines, the rules dispatching them — with byte-for-byte HTML
// comparison and an AST comparison against the engine-free path.
//
// commonmark_test.go asserts the same corpus over the engine-free modules;
// this suite is the other leg of the dual-path contract (dx-report §42):
// the conformance claim holds on the code path the plugin actually runs,
// not just on the reference implementation. The native tree comes back via
// meta["md"]["keepTree"] and is rendered with the same RenderHTML the
// engine-free path uses, so the comparison isolates the parse — a
// difference here is a parsing difference, never a rendering one. Mirrors
// ts/test/engine-conformance.test.ts.

import (
	"reflect"
	"testing"

	parser "github.com/tabnas/parser/go"
)

func checkEngineExample(t *testing.T, j *parser.Tabnas, c specCase, opts Options) {
	t.Helper()

	meta := map[string]any{"md": map[string]any{"keepTree": true}}
	ast, err := j.ParseMeta(c.Markdown, meta)
	if err != nil {
		t.Fatalf("example %d: engine parse error: %v", c.Example, err)
	}

	tree, _ := meta["md"].(map[string]any)["tree"].(*MdNode)
	if tree == nil {
		t.Fatalf("example %d: keepTree returned no native tree", c.Example)
	}

	if got := RenderHTML(tree, Options{GFM: tree.GFM}); got != c.HTML {
		t.Errorf("example %d: engine-path HTML\nmarkdown: %q\n     got: %q\n    want: %q",
			c.Example, c.Markdown, got, c.HTML)
	}

	direct := ParseDocument(c.Markdown, opts)
	if !reflect.DeepEqual(jsonFlatten(ast), jsonFlatten(direct)) {
		t.Errorf("example %d: engine-path AST diverges from engine-free path\nmarkdown: %q",
			c.Example, c.Markdown)
	}
}

func TestEngineCommonMarkSpec(t *testing.T) {
	cases := loadSpecCases(t)
	j := Make(map[string]any{"gfm": false})
	opts := ResolveOptions(map[string]any{"gfm": false})
	for _, c := range cases {
		checkEngineExample(t, j, c, opts)
	}
}

func TestEngineGFMSpec(t *testing.T) {
	cases := loadGFMCases(t)
	j := Make()
	opts := ResolveOptions(nil)
	for _, c := range cases {
		checkEngineExample(t, j, c, opts)
	}
}
