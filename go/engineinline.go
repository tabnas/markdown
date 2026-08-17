// Copyright (c) 2026 Richard Rodger, MIT License

package tabnasmarkdown

// engineinline.go — the engine-facing inline driver: a second engine
// instance whose lexer IS the inline scanner. Each custom matcher is a thin
// adapter over one inlineParser scanner method — the same methods the
// engine-free path runs — so the anti-drift rule holds: recognition, node
// building, the delimiter stack and the bracket stack all live once, in
// inline.go. Mirrors ts/src/engine-inline.ts, which documents the design:
// context-sensitive matchers (the `]` matcher owns the cursor, so a
// consumed link tail is never lexed twice and the straddle cases in
// test/spec/mixed.tsv are correct by construction), matcher order as
// precedence, and one-token effects applied at lex time because the rule
// loop reads lookahead tokens before earlier alts' actions run.
//
// Unlike the TypeScript side — where one inline instance per plugin install
// is safe because JS is single-threaded — the Go instance is built per
// parse, in mdFinishAction: a plugin instance may serve concurrent
// Parse calls from many goroutines, and the engine's concurrency guarantee
// covers construction, not shared parsing. The build cost is a few tens of
// microseconds against a whole-document parse.

import (
	parser "github.com/tabnas/parser/go"
)

// inlineState is the per-parse state carried on ctx.U["inl"] — seeded by
// the parse.prepare hook from the parse meta.
type inlineState struct {
	parser *inlineParser
	block  *MdNode
}

// inlineTokenNames is the inline token alphabet, in the order the rule's
// alt lists them. Mirrors INLINE_TOKENS in ts/src/engine-inline.ts.
var inlineTokenNames = []string{
	"#IBK", // line break (soft or hard, decided against the tree)
	"#IES", // backslash escape
	"#ICS", // code span (or an unmatched literal backtick run)
	"#IDL", // emphasis/strikethrough delimiter run
	"#IOB", // [
	"#IBG", // ![
	"#ICB", // ] — including a consumed inline link tail
	"#IAL", // angle autolink
	"#IHT", // raw HTML tag
	"#IEN", // entity reference
	"#ITX", // ordinary text run
	"#ILI", // single literal character nothing else claimed
}

// inlineAdapter adapts one inlineParser scanner method into a lexer
// matcher: sync the parser to the engine's point, run the method (which
// appends nodes and moves pos), and emit a token covering exactly what it
// consumed. name may depend on what was consumed (`!` vs `![`).
func inlineAdapter(
	tins map[string]parser.Tin,
	guard func(c byte, opts Options) bool,
	run func(p *inlineParser, block *MdNode) bool,
	name func(p *inlineParser, start int) string,
	opts Options,
) parser.LexMatcher {
	return func(lex *parser.Lex, _ *parser.Rule) *parser.Token {
		if lex.Ctx == nil {
			return nil
		}
		st, ok := lex.Ctx.U["inl"].(*inlineState)
		if !ok {
			return nil
		}

		pnt := lex.Cursor()
		src := lex.Src
		if pnt.SI >= len(src) {
			return nil
		}
		if !guard(src[pnt.SI], opts) {
			return nil
		}

		p := st.parser
		start := pnt.SI
		p.pos = start
		if !run(p, st.block) {
			return nil
		}

		n := name(p, start)
		tkn := lex.Token(n, tins[n], nil, src[start:p.pos])
		pnt.CI += p.pos - start
		pnt.SI = p.pos
		return tkn
	}
}

// makeInlineTn builds the inline engine instance for one option set. All
// built-in matchers are disabled; the registered matcher set is the
// complete lexical description of the inline phase, in precedence order.
func makeInlineTn(opts Options) *parser.Tabnas {
	j := parser.Make()

	tins := map[string]parser.Tin{}
	for _, n := range inlineTokenNames {
		tins[n] = j.Token(n)
	}

	fixedName := func(n string) func(*inlineParser, int) string {
		return func(*inlineParser, int) string { return n }
	}
	spec := func(order int,
		guard func(c byte, o Options) bool,
		run func(p *inlineParser, block *MdNode) bool,
		name func(p *inlineParser, start int) string,
	) *parser.MatchSpec {
		return &parser.MatchSpec{
			Order: order,
			Make: func(_ *parser.LexConfig, _ *parser.Options) parser.LexMatcher {
				return inlineAdapter(tins, guard, run, name, opts)
			},
		}
	}

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
			Match: map[string]*parser.MatchSpec{
				"inlBreak": spec(100000,
					func(c byte, _ Options) bool { return c == '\n' },
					func(p *inlineParser, b *MdNode) bool { return p.parseNewline(b) },
					fixedName("#IBK")),
				"inlEscape": spec(100100,
					func(c byte, _ Options) bool { return c == '\\' },
					func(p *inlineParser, b *MdNode) bool { return p.parseBackslash(b) },
					fixedName("#IES")),
				"inlCode": spec(100200,
					func(c byte, _ Options) bool { return c == '`' },
					func(p *inlineParser, b *MdNode) bool { return p.parseBackticks(b) },
					fixedName("#ICS")),
				"inlDelim": spec(100300,
					func(c byte, o Options) bool {
						return c == '*' || c == '_' || (o.GFM && c == '~')
					},
					func(p *inlineParser, b *MdNode) bool {
						return p.handleDelim(p.subject[p.pos], b)
					},
					fixedName("#IDL")),
				"inlOpenBracket": spec(100400,
					func(c byte, _ Options) bool { return c == '[' },
					func(p *inlineParser, b *MdNode) bool { return p.parseOpenBracket(b) },
					fixedName("#IOB")),
				// `!` introduces an image only before `[`; otherwise a literal.
				"inlBang": spec(100500,
					func(c byte, _ Options) bool { return c == '!' },
					func(p *inlineParser, b *MdNode) bool { return p.parseBang(b) },
					func(p *inlineParser, start int) string {
						if 2 == p.pos-start {
							return "#IBG"
						}
						return "#ILI"
					}),
				// The cursor owner: on a successful inline link tail,
				// parseCloseBracket has already consumed it when this token is
				// emitted.
				"inlCloseBracket": spec(100600,
					func(c byte, _ Options) bool { return c == ']' },
					func(p *inlineParser, b *MdNode) bool { return p.parseCloseBracket(b) },
					fixedName("#ICB")),
				// At `<`: autolink first, raw HTML second — matcher order is
				// precedence.
				"inlAutolink": spec(100700,
					func(c byte, _ Options) bool { return c == '<' },
					func(p *inlineParser, b *MdNode) bool { return p.parseAutolink(b) },
					fixedName("#IAL")),
				"inlHtmlTag": spec(100800,
					func(c byte, _ Options) bool { return c == '<' },
					func(p *inlineParser, b *MdNode) bool { return p.parseHtmlTag(b) },
					fixedName("#IHT")),
				"inlEntity": spec(100900,
					func(c byte, _ Options) bool { return c == '&' },
					func(p *inlineParser, b *MdNode) bool { return p.parseEntity(b) },
					fixedName("#IEN")),
				"inlText": spec(101000,
					func(_ byte, _ Options) bool { return true },
					func(p *inlineParser, b *MdNode) bool { return p.parseString(b) },
					fixedName("#ITX")),
				// Nothing claimed the character: one literal char, same as the
				// hand scanner's dispatch fallback.
				"inlLiteral": spec(101100,
					func(_ byte, _ Options) bool { return true },
					func(p *inlineParser, b *MdNode) bool { return p.parseLiteralChar(b) },
					fixedName("#ILI")),
			},
		},
		Parse: &parser.ParseOptions{
			Prepare: map[string]func(*parser.Context){
				"inl-reset": func(ctx *parser.Context) {
					if md, ok := ctx.Meta["md"].(map[string]any); ok {
						ctx.U["inl"] = &inlineState{
							parser: md["inl"].(*inlineParser),
							block:  md["block"].(*MdNode),
						}
					}
				},
			},
		},
		Rule: &parser.RuleOptions{Start: "inline"},
	})

	// One OR-position over the whole alphabet: any inline token loops the
	// rule; the empty alts fall through to the close on #ZZ.
	orSet := make([]parser.Tin, 0, len(inlineTokenNames))
	for _, n := range inlineTokenNames {
		orSet = append(orSet, tins[n])
	}

	j.Rule("inline", func(rs *parser.RuleSpec, _ *parser.Parser) {
		rs.Clear()
		rs.AddOpen(
			&parser.AltSpec{S: [][]parser.Tin{orSet}, R: "inline"},
			&parser.AltSpec{})
		rs.AddClose(
			&parser.AltSpec{S: [][]parser.Tin{{parser.TinZZ}}, A: func(_ *parser.Rule, ctx *parser.Context) {
				st := ctx.U["inl"].(*inlineState)
				st.parser.finishBlock(st.block)
			}},
			&parser.AltSpec{})
	})

	return j
}

// parseInlinesEngine is the engine-path inline phase: the same block walk as
// parseInlines, with each block's scan driven by the engine instance's
// lexer. An empty subject never reaches the engine (its empty-source path
// returns before the rules run), and finishes the block directly — a no-op
// resolve, exactly as the hand scanner's parse does.
func parseInlinesEngine(inlineTn *parser.Tabnas, doc *MdNode, refmap RefMap, opts Options) {
	p := &inlineParser{refmap: refmap, options: opts}

	forEachInlineBlock(doc, opts, func(block *MdNode) {
		if !p.beginBlock(block) {
			p.finishBlock(block)
			return
		}
		if _, err := inlineTn.ParseMeta(p.subject, map[string]any{
			"md": map[string]any{"inl": p, "block": block},
		}); err != nil {
			// CommonMark defines an output for every input; an engine error
			// here is an internal wiring defect, not a parse result.
			panic(err)
		}
	})
}
