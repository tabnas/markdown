/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Engine-free entry point for the CommonMark parser. Port of
// ts/src/commonmark.ts.
//
// Nothing reachable from here imports the tabnas engine, so the parser — and
// the conformance suite — run without it. The plugin wiring lives in
// markdown.go, which imports this; never the other way round.

// Parse runs the full two-phase parse (spec Appendix A): block structure
// first, then inlines over each leaf's accumulated string content, using the
// link reference map the block phase collected.
func Parse(input string, opts Options) *MdNode {
	doc, refmap := parseBlocks(input, opts)
	parseInlines(doc, refmap, opts)
	return doc
}
