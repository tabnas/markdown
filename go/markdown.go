/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Package tabnasmarkdown is a CommonMark 0.31.2 parser, built as a tabnas
// plugin on the bare engine.
//
// This file is only the plugin wiring and the public surface. The parser
// itself is engine-free and lives in commonmark.go and the modules it pulls in
// (block.go, inline.go, html.go, common.go, node.go), so it can be exercised —
// and the conformance suite run — without the engine. Nothing under
// commonmark.go may import it.
//
// The parse result is a native CommonMark node tree; ast.go projects it to the
// map-based JSON AST this package has always returned, and html.go renders it
// for ToHTML.
//
// This is a port of ts/src/*.ts, which is canonical. See AGENTS.md.
package tabnasmarkdown

import (
	parser "github.com/tabnas/parser/go"
)

// VERSION is this module's version. It MUST equal ts/package.json
// "version": the release orchestrator rewrites both, and
// TestVersionMatchesPackageJSON fails the build if they drift.
const VERSION = "0.7.14"

// Defaults mirrors ts/src/markdown.ts Markdown.defaults.
var Defaults = map[string]any{
	"gfm":    true,
	"breaks": false,
}

// ---------------------------------------------------------------------------
// Public parse API (engine-free)

// ParseDocument parses Markdown to the mdast-adjacent JSON AST. References
// stay unresolved, as mdast keeps them: a bracketed label is a linkReference
// or an imageReference, and each definition a definition node where it
// stands. ResolveReferences resolves them.
func ParseDocument(src string, opts Options) map[string]any {
	return ToAST(parseKeepingReferences(src, opts), opts)
}

// ParseInline parses a single run of inline Markdown, returning the inline
// children of the resulting paragraph.
func ParseInline(text string, opts Options) []any {
	doc := ParseDocument(text, opts)
	children, _ := doc["children"].([]any)
	if len(children) == 0 {
		return []any{}
	}
	first, ok := children[0].(map[string]any)
	if !ok || first["type"] != "paragraph" {
		return []any{}
	}
	inlines, _ := first["children"].([]any)
	return inlines
}

// ToHTML parses Markdown and renders it to CommonMark-conformant HTML.
func ToHTML(src string, opts Options) string {
	return RenderHTML(Parse(src, opts), opts)
}

// ParseTree parses Markdown to the native CommonMark node tree.
func ParseTree(src string, opts Options) *MdNode {
	return Parse(src, opts)
}

// ---------------------------------------------------------------------------
// Plugin wiring

func boolPtr(b bool) *bool { return &b }

// Markdown installs the parser on a tabnas engine instance.
//
// The engine parse IS the parse, and its rules open and close every block:
// the mdBlock matcher (engineblock.go) cuts one #LB token per physical line
// through the lexer's window, runs the shared block algorithm over it, and
// hands out before it the tokens of the blocks that line closed and opened;
// each node of the AST is a rule that opens on its token and, for a
// container, closes on #EN; and the document's children, like every
// container's, are the children rule, a replace loop, so ten thousand blocks
// run at the depth one does. The block decisions are the shared engine-free
// core's (the anti-drift rule, dx-report §42), which is what keeps this
// path's AST identical to ParseDocument's (asserted by the differential gate
// and the engine-path conformance harness).
func Markdown(j *parser.Tabnas, options map[string]any) error {
	opts := ResolveOptions(options)

	tins := map[string]parser.Tin{
		lineToken: j.Token(lineToken),
		endToken:  j.Token(endToken),
	}
	slot := []parser.Tin{tins[lineToken]}
	for _, n := range nodeRules {
		tins[n.token] = j.Token(n.token)
		slot = append(slot, tins[n.token])
	}

	// Every built-in matcher is off — deliberate configuration, not defense.
	// Markdown's block alphabet is *lines*, so the one registered matcher is
	// the complete lexical description of this phase: mdBlock, at an order
	// ahead of every built-in, consuming each physical line whole. (The
	// built-ins would misread the syntax anyway: backtick code spans lex as
	// unterminated strings, `# heading` as a comment, `1. list` as a number.)
	j.SetOptions(parser.Options{
		Fixed:   &parser.FixedOptions{Lex: boolPtr(false)},
		Space:   &parser.SpaceOptions{Lex: boolPtr(false)},
		Line:    &parser.LineOptions{Lex: boolPtr(false)},
		Text:    &parser.TextOptions{Lex: boolPtr(false)},
		String:  &parser.StringOptions{Lex: boolPtr(false)},
		Comment: &parser.CommentOptions{Lex: boolPtr(false)},
		Number:  &parser.NumberOptions{Lex: boolPtr(false)},
		Value:   &parser.ValueOptions{Lex: boolPtr(false)},
		Lex: &parser.LexOptions{
			EmptyResult: map[string]any{"type": "document", "children": []any{}},
			Match: map[string]*parser.MatchSpec{
				"mdBlock": {Order: 100000, Make: makeMdBlockMatcher(opts, tins)},
			},
		},
		Parse: &parser.ParseOptions{
			Prepare: map[string]func(*parser.Context){"md-reset": mdPrepare(opts)},
		},
		Rule: &parser.RuleOptions{Start: "markdown"},
	})

	// The document: its children, then the end of the source.
	j.Rule("markdown", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		rs.AddOpen(&parser.AltSpec{P: "children", A: mdBegin})
		rs.AddClose(&parser.AltSpec{S: [][]parser.Tin{{parser.TinZZ}}, A: mdFinish(opts)})
	})

	// A container's children, one node or one line per iteration: a node's
	// token pushes that node's rule, a line is consumed, and the loop goes on
	// by replacement (R), never by a push chain, while the next token is
	// either; anything else (#EN, #ZZ) ends it. The GFM nodes are the gfm
	// group's alts, which gfm:false subtracts.
	j.Rule("children", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		for _, n := range nodeRules {
			alt := &parser.AltSpec{S: [][]parser.Tin{{tins[n.token]}}, B: 1, P: n.rule}
			if n.gfm {
				alt.G = "gfm"
			}
			rs.AddOpen(alt)
		}
		rs.AddOpen(&parser.AltSpec{S: [][]parser.Tin{{tins[lineToken]}}}, &parser.AltSpec{})
		rs.AddClose(&parser.AltSpec{S: [][]parser.Tin{slot}, B: 1, R: "children"}, &parser.AltSpec{})
	})

	// A rule per node: a container opens on its token, pushes its children,
	// and closes on #EN; a leaf opens on its token and closes at once.
	for _, n := range nodeRules {
		n := n
		j.Rule(n.rule, func(rs *parser.RuleSpec, _ *parser.Parser) {
			rs.Clear()
			if n.container {
				rs.AddOpen(&parser.AltSpec{S: [][]parser.Tin{{tins[n.token]}}, P: "children", A: mdOpen})
				rs.AddClose(&parser.AltSpec{S: [][]parser.Tin{{tins[endToken]}}, A: mdClose})
			} else {
				rs.AddOpen(&parser.AltSpec{S: [][]parser.Tin{{tins[n.token]}}, A: mdLeaf})
				rs.AddClose(&parser.AltSpec{})
			}
		})
	}

	// The GFM dialect is subtracted, not branched around: without the
	// option, the tagged alts above simply do not exist in this instance's
	// grammar.
	if !opts.GFM {
		j.SetOptions(parser.Options{Rule: &parser.RuleOptions{Exclude: "gfm"}})
	}

	return nil
}

// Make returns a tabnas engine with the Markdown plugin installed: the
// parser this package makes whole, the Go counterpart of
// `new Tabnas().use(Markdown)`, reading a source a window at a time. The one
// matcher cuts a line through the lexer's window and fills it past the
// line's end, and nothing else reads the source, so a parse from a reader
// (Tabnas.ParseReader, admin ADR-30) holds the text of the lines it has not
// finished with, and not the whole text. The option is set here, on the
// parser this package makes, not in the plugin.
func Make(options ...map[string]any) *parser.Tabnas {
	j := parser.Make()
	var opts map[string]any
	if len(options) > 0 {
		opts = options[0]
	}
	// Markdown never returns an error, so the plugin install cannot fail.
	_ = j.Use(Markdown, opts)
	window := true
	j.SetOptions(parser.Options{Lex: &parser.LexOptions{Window: &window}})
	return j
}
