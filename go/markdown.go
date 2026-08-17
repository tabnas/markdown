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
const VERSION = "0.6.2"

// Defaults mirrors ts/src/markdown.ts Markdown.defaults.
var Defaults = map[string]any{
	"gfm":    true,
	"breaks": false,
}

// ---------------------------------------------------------------------------
// Public parse API (engine-free)

// ParseDocument parses Markdown to the mdast-adjacent JSON AST.
func ParseDocument(src string, opts Options) map[string]any {
	return ToAST(Parse(src, opts), opts)
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
//
// The markdown rule's BO action reads the whole source from ctx.Src and hands
// it to the parser; the alts then consume the token stream so the engine's
// trailing-content check (lex must end at #ZZ) passes. TinAA is the ANY-token
// wildcard and R:"markdown" loops at the same depth, consuming one token per
// iteration until only #ZZ remains.

func boolPtr(b bool) *bool { return &b }

// Markdown installs the parser on a tabnas engine instance.
//
// The engine parse IS the parse: the mdLine custom matcher (engineblock.go)
// lexes one `#LB` token per physical line, parse.prepare seeds a fresh
// blockParser on ctx.U["md"], the `line` rule feeds each matched line to the
// shared incorporateLine, and the `markdown` rule's close action on `#ZZ`
// finalizes, runs the shared inline phase, and projects the public AST into
// the rule's node. No recognition or block decision happens here — the
// engine owns tokenization, dispatch and state carriage; the algorithm stays
// in the engine-free core (the anti-drift rule, dx-report §42), which is
// what keeps this path byte-identical to ParseDocument (asserted by the
// differential gate and the engine-path conformance harness).
func Markdown(j *parser.Tabnas, options map[string]any) error {
	opts := ResolveOptions(options)

	lbTin := j.Token("#LB")

	// Every built-in matcher is off — deliberate configuration, not defense.
	// Markdown's block alphabet is *lines*, so the one registered matcher is
	// the complete lexical description of this phase: mdLine, at an order
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
				"mdLine": {Order: 100000, Make: makeMdLineMatcher(lbTin)},
			},
		},
		Parse: &parser.ParseOptions{
			Prepare: map[string]func(*parser.Context){"md-reset": mdPrepare(opts)},
		},
		Rule: &parser.RuleOptions{Start: "markdown"},
	})

	// The inline phase's own engine instance — nested parsing. Built per
	// parse (inside the finish action) rather than per install: a plugin
	// instance may serve concurrent Parse calls from many goroutines, and
	// the engine's concurrency guarantee covers construction, not shared
	// parsing. See engineinline.go.
	runInlines := func(doc *MdNode, refmap RefMap) {
		parseInlinesEngine(makeInlineTn(opts), doc, refmap, opts)
	}

	j.Rule("markdown", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		rs.AddOpen(&parser.AltSpec{P: "line"})
		rs.AddClose(
			&parser.AltSpec{S: [][]parser.Tin{{parser.TinZZ}}, A: mdFinishAction(opts, runInlines)},
			&parser.AltSpec{})
	})

	// One `#LB` consumed per iteration, tail-recursing via R until only `#ZZ`
	// remains; the empty alts are the fall-through that hands control back to
	// markdown's close.
	//
	// The first alt is the GFM extension seam: tagged G "gfm", gated on the
	// token's tblArm bit, arming the shared table probe for this line. It is
	// injected exactly the way a downstream dialect would extend this
	// grammar — an alt ahead of the base one, subtractable by its group
	// tag — and gfm:false removes it below via rule Exclude, so the base
	// dialect is the grammar minus the tagged alts, visibly.
	j.Rule("line", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		rs.AddOpen(
			&parser.AltSpec{
				S: [][]parser.Tin{{lbTin}},
				C: func(_ *parser.Rule, ctx *parser.Context) bool {
					info, ok := ctx.T0.Use["md"].(*lineInfo)
					return ok && info.tblArm
				},
				R: "line",
				A: mdLineGfmAction,
				G: "gfm",
			},
			&parser.AltSpec{S: [][]parser.Tin{{lbTin}}, R: "line", A: mdLineAction},
			&parser.AltSpec{})
		rs.AddClose(&parser.AltSpec{})
	})

	// The GFM dialect is subtracted, not branched around: without the
	// option, the tagged alts above simply do not exist in this instance's
	// grammar.
	if !opts.GFM {
		j.SetOptions(parser.Options{Rule: &parser.RuleOptions{Exclude: "gfm"}})
	}

	return nil
}

// Make returns a tabnas engine with the Markdown plugin installed. It is the
// Go counterpart of `new Tabnas().use(Markdown)`.
func Make(options ...map[string]any) *parser.Tabnas {
	j := parser.Make()
	var opts map[string]any
	if len(options) > 0 {
		opts = options[0]
	}
	// Markdown never returns an error, so the plugin install cannot fail.
	_ = j.Use(Markdown, opts)
	return j
}
