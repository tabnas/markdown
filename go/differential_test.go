// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// differential_test.go — the differential gate: the plugin path must agree
// with the engine-free path.
//
// ParseDocument(src, opts) (engine-free) and Make(opts).Parse(src) (plugin
// path) are required to produce the same AST for every input, compared after
// JSON flattening. Today the two paths share every line of code, so this
// suite is trivially green — that is the point. It exists so the
// engine-substrate stages (dx-report §42) cannot land a divergence silently:
// the conformance corpora are blind to several legal-input behaviors (see
// test/spec/edge.tsv), so "all suites green" is necessary but not sufficient
// once the plugin path stops sharing the drivers.
//
// The seeded document generator is identical to the one in
// ts/test/differential.test.ts — same fragments, same xorshift32, same
// documents — so a reproduction case can be named by its seed in either
// runtime.

import (
	"encoding/json"
	"reflect"
	"strconv"
	"testing"

	parser "github.com/tabnas/parser/go"
	support "github.com/tabnas/support/go"
)

var differentialOptionSets = []map[string]any{
	{},
	{"gfm": false},
}

// diffRigs builds one engine instance per option set, reused across every
// parse — the supported pattern (instance construction is what
// go/perf_test.go pins as the dominant cost).
func diffRigs() []*parser.Tabnas {
	rigs := make([]*parser.Tabnas, len(differentialOptionSets))
	for i, opts := range differentialOptionSets {
		rigs[i] = Make(opts)
	}
	return rigs
}

func checkDifferential(t *testing.T, rigs []*parser.Tabnas, src string, where string) {
	t.Helper()
	for i, opts := range differentialOptionSets {
		direct := ParseDocument(src, ResolveOptions(opts))

		plugin, err := rigs[i].Parse(src)
		if err != nil {
			t.Fatalf("%s opts=%v: plugin path error: %v", where, opts, err)
		}

		if !reflect.DeepEqual(jsonFlatten(plugin), jsonFlatten(direct)) {
			gotJSON, _ := json.Marshal(plugin)
			wantJSON, _ := json.Marshal(direct)
			t.Errorf("%s opts=%v: plugin path diverges from engine-free path\n plugin: %s\n direct: %s",
				where, opts, gotJSON, wantJSON)
		}
	}
}

func TestDifferentialCommonMark(t *testing.T) {
	rigs := diffRigs()
	for _, c := range loadSpecCases(t) {
		checkDifferential(t, rigs, c.Markdown, "commonmark example "+strconv.Itoa(c.Example))
	}
}

func TestDifferentialGFM(t *testing.T) {
	rigs := diffRigs()
	for _, c := range loadGFMCases(t) {
		checkDifferential(t, rigs, c.Markdown, "gfm example "+strconv.Itoa(c.Example))
	}
}

func TestDifferentialFixtures(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	rigs := diffRigs()
	for _, spec := range files {
		for _, row := range spec.Rows {
			checkDifferential(t, rigs, row.UnescNamed("input"), spec.Name+" "+row.Where())
		}
	}
}

// --- seeded pseudo-random documents -----------------------------------------

// differentialFragments is the fragment pool — byte-identical to FRAGMENTS in
// ts/test/differential.test.ts; a change here must land there in the same
// commit.
var differentialFragments = []string{
	"# H1\n",
	"para text with *emph* and _under_\n",
	"- item one\n",
	"- [x] task\n",
	"1. ordered\n",
	"> quoted line\n",
	"> a | b\n> --- | ---\n",
	"```\ncode\n```\n",
	"a | b\n--- | ---\nc | d\n",
	"[l](u \"t`t\")\n",
	"[a](b`c) `d`\n",
	"[not a `link](/foo`)\n",
	"`code` span\n",
	"hard  \nbreak\n",
	"soft \nbreak\n",
	"[ref]: /url \"title\"\n",
	"use [ref] and [missing] here\n",
	"~~del~~ and ~one~ tilde\n",
	"<div>\nhtml block\n</div>\n",
	"inline <em>html</em> tag\n",
	"escaped \\* star \\| pipe\n",
	"&amp; entity &#65; and &nosuch;\n",
	"bare www.example.com literal\n",
	"scheme https://x.example/z?q=1 literal\n",
	"mail a@b.co literal\n",
	"***\n",
	"Setext\n===\n",
	"Setext two\n---\n",
	"    indented code\n",
	"> [a](b`c) `d`\n",
	"![img](/i.png \"alt`tick\")\n",
	"tab\there\n",
	"\n",
}

// nextRand is xorshift32 — identical to nextRand in
// ts/test/differential.test.ts.
func nextRand(state uint32) uint32 {
	state ^= state << 13
	state ^= state >> 17
	state ^= state << 5
	return state
}

// fuzzDoc builds the deterministic document for a seed; TS builds the same
// one.
func fuzzDoc(seed uint32) string {
	state := seed * 2654435761
	if state == 0 {
		state = 1
	}
	state = nextRand(state)
	count := 3 + int(state%12)
	doc := ""
	for i := 0; i < count; i++ {
		state = nextRand(state)
		doc += differentialFragments[int(state)%len(differentialFragments)]
	}
	return doc
}

const differentialFuzzDocs = 200

func TestDifferentialFuzz(t *testing.T) {
	rigs := diffRigs()
	for seed := uint32(1); seed <= differentialFuzzDocs; seed++ {
		checkDifferential(t, rigs, fuzzDoc(seed), "fuzz seed "+strconv.Itoa(int(seed)))
	}
}
