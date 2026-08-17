// Copyright (c) 2026 Richard Rodger, MIT License

package tabnasmarkdown

// engineblock.go — the engine-facing block driver: a custom lexer matcher
// that turns the engine's scan into one `#LB` token per physical line, and
// the actions that feed those lines to the shared blockParser.
//
// This is the seam of the anti-drift rule (dx-report §42): nothing here
// recognizes anything. Line cutting and NUL replacement come from
// segmentNextLine, blank detection from isBlank, and every block decision —
// continuation, starts, lazy continuation, finalization — from the same
// blockParser the engine-free path runs. The engine owns tokenization,
// dispatch, per-parse state carriage and observability; the algorithm stays
// in the shared core. Mirrors ts/src/engine-block.ts.

import (
	"strings"

	parser "github.com/tabnas/parser/go"
)

// lineInfo is the `#LB` token's payload: one physical line, pre-classified.
// One pointer under a single Use key rather than three map entries, so a
// token costs one allocation of payload, not four.
type lineInfo struct {
	// text is the line's text, terminator excluded, NULs already replaced.
	text string
	// blank is true when the line holds nothing but spaces and tabs.
	blank bool
	// tblArm is the GFM table arming bit: true when the line contains a `|`
	// anywhere. A deliberate superset of "is a delimiter row" — the
	// delimiter row of a table nested in a block quote only matches at the
	// container-adjusted offset, which only the block algorithm knows.
	// tryOpenTable re-verifies; this bit exists so a gfm-gated alt has
	// something honest to read (see test/spec/mixed.tsv).
	tblArm bool
}

// mdParseState is the per-parse state carried on ctx.U["md"] — seeded by the
// parse.prepare hook.
type mdParseState struct {
	bp   *blockParser
	meta map[string]any
}

// makeMdLineMatcher builds the `mdLine` matcher: each invocation consumes
// one physical line — terminator included — and emits a single `#LB` token
// carrying the line as a lineInfo. The matcher owns the engine's point: SI
// advances past the terminator, RI counts rows, and CI resets per line (the
// engine's column is not used for indentation — the shared blockParser
// computes spec §2.2 tab-expanded columns itself).
func makeMdLineMatcher(lbTin parser.Tin) parser.MakeLexMatcher {
	return func(_ *parser.LexConfig, _ *parser.Options) parser.LexMatcher {
		return func(lex *parser.Lex, _ *parser.Rule) *parser.Token {
			pnt := lex.Cursor()

			text, next, ok := segmentNextLine(lex.Src, pnt.SI)
			if !ok {
				return nil
			}

			tkn := lex.Token("#LB", lbTin, text, lex.Src[pnt.SI:next])
			tkn.Use = map[string]any{"md": &lineInfo{
				text:   text,
				blank:  isBlank(text),
				tblArm: strings.IndexByte(text, '|') >= 0,
			}}

			// Row/column bookkeeping is observability data (#ZZ position,
			// traces): a consumed terminator starts a new row; an
			// unterminated final line just widens the current one.
			terminated := next > pnt.SI+len(text)
			pnt.SI = next
			if terminated {
				pnt.RI++
				pnt.CI = 1
			} else {
				pnt.CI += len(text)
			}

			return tkn
		}
	}
}

// mdPrepare is the parse.prepare hook: fresh block-parser state for every
// parse, with the parse's meta captured for the keepTree handshake.
func mdPrepare(opts Options) func(ctx *parser.Context) {
	return func(ctx *parser.Context) {
		ctx.U["md"] = &mdParseState{bp: newBlockParser(opts), meta: ctx.Meta}
	}
}

// mdLineAction is the `line` open alt's action: feed the matched `#LB` line
// to the shared algorithm.
func mdLineAction(r *parser.Rule, ctx *parser.Context) {
	state := ctx.U["md"].(*mdParseState)
	info := r.O0.Use["md"].(*lineInfo)
	state.bp.incorporateLine(info.text)
}

// mdFinishAction is the `markdown` close alt's action on `#ZZ`: finalize
// blocks, run the shared inline phase, and project the public AST into the
// rule's node. When the caller passed meta["md"]["keepTree"], the native
// tree is handed back on the meta map — the engine-path conformance harness
// renders it.
func mdFinishAction(opts Options) parser.AltAction {
	return func(r *parser.Rule, ctx *parser.Context) {
		state := ctx.U["md"].(*mdParseState)
		doc, refmap := state.bp.finish()
		parseInlines(doc, refmap, opts)

		if md, ok := state.meta["md"].(map[string]any); ok {
			if keep, _ := md["keepTree"].(bool); keep {
				md["tree"] = doc
			}
		}

		r.Node = ToAST(doc, opts)
	}
}
