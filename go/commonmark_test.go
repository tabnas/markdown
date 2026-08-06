/* Copyright (c) 2025-2026 Richard Rodger and other contributors, MIT License */

package tabnasmarkdown

// CommonMark 0.31.2 conformance, over the vendored 652-example suite in
// test/commonmark/spec.json — the same corpus ts/test/commonmark.test.ts runs,
// so the two runtimes are held to one standard rather than to each other.
//
// The suite is pure CommonMark, so GFM must be off: strikethrough changes the
// expected output for several examples.
//
// Run just this file's summary with:
//
//	go test -run TestCommonMarkSpec -v ./...

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"testing"
)

type specCase struct {
	Markdown string `json:"markdown"`
	HTML     string `json:"html"`
	Example  int    `json:"example"`
	Section  string `json:"section"`
}

func loadSpecCases(t *testing.T) []specCase {
	t.Helper()
	path := filepath.Join("..", "test", "commonmark", "spec.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	var cases []specCase
	if err := json.Unmarshal(raw, &cases); err != nil {
		t.Fatalf("parse %s: %v", path, err)
	}
	if len(cases) != 652 {
		t.Fatalf("expected the full 0.31.2 suite, got %d examples", len(cases))
	}
	return cases
}

func TestCommonMarkSpec(t *testing.T) {
	cases := loadSpecCases(t)
	opts := Options{GFM: false, Breaks: false}

	type tally struct{ pass, total int }
	bySection := map[string]*tally{}
	var order []string

	for _, c := range cases {
		s, ok := bySection[c.Section]
		if !ok {
			s = &tally{}
			bySection[c.Section] = s
			order = append(order, c.Section)
		}
		s.total++

		actual := ToHTML(c.Markdown, opts)
		if actual == c.HTML {
			s.pass++
			continue
		}
		t.Errorf("example %d [%s]\n  markdown: %q\n  expected: %q\n  actual:   %q",
			c.Example, c.Section, c.Markdown, c.HTML, actual)
	}

	total, passed := 0, 0
	sort.SliceStable(order, func(i, j int) bool {
		a, b := bySection[order[i]], bySection[order[j]]
		return float64(a.pass)/float64(a.total) < float64(b.pass)/float64(b.total)
	})
	for _, name := range order {
		s := bySection[name]
		total += s.total
		passed += s.pass
		mark := "   "
		if s.pass == s.total {
			mark = "OK "
		}
		t.Logf("  %s %-40s %3d/%-3d", mark, name, s.pass, s.total)
	}
	t.Logf("  TOTAL %d/%d  %.2f%%", passed, total, 100*float64(passed)/float64(total))
}

// TestCommonMarkOptionMatrix covers the combinations the corpus never
// exercises: it only ever runs {GFM:false, Breaks:false}. Every other
// combination must at least parse and render without panicking.
func TestCommonMarkOptionMatrix(t *testing.T) {
	cases := loadSpecCases(t)
	for _, gfm := range []bool{true, false} {
		for _, breaks := range []bool{true, false} {
			opts := Options{GFM: gfm, Breaks: breaks}
			name := fmt.Sprintf("gfm=%v/breaks=%v", gfm, breaks)
			t.Run(name, func(t *testing.T) {
				for _, c := range cases {
					func() {
						defer func() {
							if r := recover(); r != nil {
								t.Fatalf("example %d panicked: %v", c.Example, r)
							}
						}()
						_ = ToHTML(c.Markdown, opts)
						_ = ParseDocument(c.Markdown, opts)
					}()
				}
			})
		}
	}
}
