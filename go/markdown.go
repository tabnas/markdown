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

const Version = "0.4.2"

// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
const grammarText = `
# Markdown prose grammar — CommonMark/GFM subset
# See dx-report.md section 2 and ts/src/markdown.ts header.
# This grammar is intentionally trivial: the prose parser is implemented
# in JS inside the markdown rule bo action (parseDocument). The
# block structure therefore shows as a single markdown -> block fan-out
# in the railroad diagram, which is honest about where the complexity
# lives. The file is kept so ts/embed-grammar.js continues to embed a
# grammar verbatim into both runtimes (see AGENTS.md).
{
  rule: markdown: open: [
    { s: '#ZZ' }
  ]
  rule: markdown: close: [
    { s: '#ZZ' }
  ]
}
`

// --- END EMBEDDED markdown-grammar.jsonic ---

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
func Markdown(j *parser.Tabnas, options map[string]any) error {
	opts := ResolveOptions(options)

	// The parser reads ctx.Src directly rather than the token stream, so
	// disable the lexers that would corrupt Markdown syntax before it gets
	// there: backtick code spans lex as unterminated strings, `# heading` as a
	// comment, and `1. list` as a number.
	j.SetOptions(parser.Options{
		String:  &parser.StringOptions{Lex: boolPtr(false)},
		Comment: &parser.CommentOptions{Lex: boolPtr(false)},
		Number:  &parser.NumberOptions{Lex: boolPtr(false)},
		Value:   &parser.ValueOptions{Lex: boolPtr(false)},
		Lex: &parser.LexOptions{
			EmptyResult: map[string]any{"type": "document", "children": []any{}},
		},
		Rule: &parser.RuleOptions{Start: "markdown"},
	})

	// The embedded grammar is documentation of the entry rule, not a source of
	// behaviour: block structure is decided by the line-oriented algorithm in
	// block.go, not by declarative alts. It is kept because ts/embed-grammar.js
	// embeds it verbatim into both runtimes (see AGENTS.md).
	_ = grammarText

	j.Rule("markdown", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		rs.AddBO(func(r *parser.Rule, ctx *parser.Context) {
			r.Node = ParseDocument(ctx.Src, opts)
		})
		rs.AddOpen(&parser.AltSpec{S: [][]parser.Tin{{parser.TinAA}}, R: "markdown"}, &parser.AltSpec{})
		rs.AddClose(&parser.AltSpec{S: [][]parser.Tin{{parser.TinZZ}}}, &parser.AltSpec{})
	})

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
