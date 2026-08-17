/* Copyright (c) 2026 Richard Rodger, MIT License */

// The engine-facing inline driver: a second, package-private engine instance
// whose lexer IS the inline scanner. Each custom matcher is a thin adapter
// over one `InlineParser` scanner method — the same methods the engine-free
// path runs — so the anti-drift rule holds: recognition, node building, the
// delimiter stack and the bracket stack all live once, in `inline.ts`.
//
// Two engine features carry the design:
//
// * **Context-sensitive matchers.** Every adapter reaches the per-parse
//   state through `lex.ctx.u.inl`. That is what makes the `]` matcher the
//   cursor owner: `parseCloseBracket` consumes a successful link tail
//   directly from the subject, and because that happens at *lex* time, the
//   engine's point advances past the tail and everything after it is lexed
//   fresh. The corpus-blind straddle cases (a backtick inside a successful
//   destination or title, followed by a code span — see test/spec/mixed.tsv)
//   are correct by construction: no token is ever cut across a tail
//   boundary, so no re-lex or resynchronization machinery exists at all.
//
// * **Matcher order as precedence.** At `<`, the autolink adapter runs
//   before the raw-HTML adapter, exactly as the hand scanner's dispatch
//   tries them; the text-run adapter and the single-character literal
//   fallback come last. The registered order *is* §6.3's precedence table,
//   visible in the instance's configuration instead of buried in a switch.
//
// The one-token effects are applied at lex time on purpose. The engine's
// rule loop reads lookahead tokens before earlier alts' actions run, so any
// state a matcher consults — the tree for break trimming, the bracket stack
// for `]` — must be current when the *lexer* reaches it, not when the
// parser does. The `inline` rule therefore carries no per-token actions:
// the tokens are the observable record of the scan (subscribe with
// `tn.sub({lex})` and watch), and the rule's close action finalizes the
// block exactly as the engine-free `parse` tail does.

import { Tabnas } from '@tabnas/parser'
import type { Context, MakeLexMatcher } from '@tabnas/parser'

import { InlineParser, forEachInlineBlock } from './inline.ts'
import type { MdNode } from './node.ts'
import type { ParserOptions, RefMap } from './options.ts'

const C_NEWLINE = 10
const C_BANG = 33
const C_AMPERSAND = 38
const C_ASTERISK = 42
const C_LESSTHAN = 60
const C_OPEN_BRACKET = 91
const C_BACKSLASH = 92
const C_CLOSE_BRACKET = 93
const C_UNDERSCORE = 95
const C_BACKTICK = 96
const C_TILDE = 126

/** The per-parse state carried on `ctx.u.inl` — seeded by `parse.prepare`. */
export type InlineState = {
  parser: InlineParser
  block: MdNode
}

/** The inline token alphabet, in the order the rule's alt lists them. */
export const INLINE_TOKENS = [
  '#IBK', // line break (soft or hard, decided against the tree)
  '#IES', // backslash escape
  '#ICS', // code span (or an unmatched literal backtick run)
  '#IDL', // emphasis/strikethrough delimiter run
  '#IOB', // [
  '#IBG', // ![
  '#ICB', // ] — including a consumed inline link tail
  '#IAL', // angle autolink
  '#IHT', // raw HTML tag
  '#IEN', // entity reference
  '#ITX', // ordinary text run
  '#ILI', // single literal character nothing else claimed
]

/**
 * Adapt one `InlineParser` scanner method into a lexer matcher: sync the
 * parser to the engine's point, run the method (which appends nodes and
 * moves `pos`), and emit a token covering exactly what it consumed.
 * `name` may depend on what was consumed (`!` vs `![`).
 */
function adapt(
  guard: (cc: number, opts: ParserOptions) => boolean,
  run: (p: InlineParser, block: MdNode) => boolean,
  name: (p: InlineParser, start: number) => string,
  opts: ParserOptions,
): any {
  return (lex: any) => {
    const st: InlineState | undefined = lex.ctx && lex.ctx.u && lex.ctx.u.inl
    if (undefined === st) return undefined

    const pnt = lex.pnt
    const src: string = lex.src
    if (pnt.sI >= src.length) return undefined
    if (!guard(src.charCodeAt(pnt.sI), opts)) return undefined

    const p = st.parser
    const start = pnt.sI
    p.pos = start
    if (!run(p, st.block)) return undefined

    const tkn = lex.token(name(p, start), undefined, src.slice(start, p.pos), pnt)
    pnt.cI += p.pos - start
    pnt.sI = p.pos
    return tkn
  }
}

/** Build the inline matcher set for one option set. */
function inlineMatchers(opts: ParserOptions): Record<string, { order: number; make: MakeLexMatcher }> {
  const fixed = (n: string) => () => n
  const m = (
    order: number,
    guard: (cc: number, o: ParserOptions) => boolean,
    run: (p: InlineParser, block: MdNode) => boolean,
    name: (p: InlineParser, start: number) => string,
  ) => ({ order, make: (() => adapt(guard, run, name, opts)) as MakeLexMatcher })

  return {
    inlBreak: m(1.00e5, (c) => C_NEWLINE === c, (p, b) => p.parseNewline(b), fixed('#IBK')),
    inlEscape: m(1.01e5, (c) => C_BACKSLASH === c, (p, b) => p.parseBackslash(b), fixed('#IES')),
    inlCode: m(1.02e5, (c) => C_BACKTICK === c, (p, b) => p.parseBackticks(b), fixed('#ICS')),
    inlDelim: m(
      1.03e5,
      (c, o) => C_ASTERISK === c || C_UNDERSCORE === c || (true === o.gfm && C_TILDE === c),
      (p, b) => p.handleDelim(p.subject.charCodeAt(p.pos), b),
      fixed('#IDL'),
    ),
    inlOpenBracket: m(1.04e5, (c) => C_OPEN_BRACKET === c, (p, b) => p.parseOpenBracket(b), fixed('#IOB')),
    // `!` introduces an image only before `[`; otherwise it is a literal.
    inlBang: m(
      1.05e5,
      (c) => C_BANG === c,
      (p, b) => p.parseBang(b),
      (p, start) => (2 === p.pos - start ? '#IBG' : '#ILI'),
    ),
    // The cursor owner: on a successful inline link tail, `parseCloseBracket`
    // has already consumed it when this token is emitted.
    inlCloseBracket: m(1.06e5, (c) => C_CLOSE_BRACKET === c, (p, b) => p.parseCloseBracket(b), fixed('#ICB')),
    // At `<`: autolink first, raw HTML second — matcher order is precedence.
    inlAutolink: m(1.07e5, (c) => C_LESSTHAN === c, (p, b) => p.parseAutolink(b), fixed('#IAL')),
    inlHtmlTag: m(1.08e5, (c) => C_LESSTHAN === c, (p, b) => p.parseHtmlTag(b), fixed('#IHT')),
    inlEntity: m(1.09e5, (c) => C_AMPERSAND === c, (p, b) => p.parseEntity(b), fixed('#IEN')),
    inlText: m(1.10e5, () => true, (p, b) => p.parseString(b), fixed('#ITX')),
    // Nothing claimed the character: one literal char, same as the hand
    // scanner's dispatch fallback.
    inlLiteral: m(1.11e5, () => true, (p, b) => p.parseLiteralChar(b), fixed('#ILI')),
  }
}

/**
 * Build the package-private inline engine instance for one option set. All
 * built-in matchers are disabled; the matcher set above is the complete
 * lexical description of the inline phase.
 */
export function makeInlineTn(opts: ParserOptions): Tabnas {
  const tn = new Tabnas()

  tn.options({ rule: { start: 'inline' } })
  tn.options({
    space: { lex: false },
    line: { lex: false },
    string: { lex: false },
    comment: { lex: false },
    number: { lex: false },
    value: { lex: false },
    text: { lex: false },
    lex: { match: inlineMatchers(opts) },
    parse: {
      prepare: {
        'inl-reset': (_tn: Tabnas, ctx: Context, meta?: any) => {
          ;(ctx.u as any).inl =
            meta && meta.md ? { parser: meta.md.inl, block: meta.md.block } : undefined
        },
      },
    },
  })

  // One OR-position over the whole alphabet: any inline token loops the
  // rule; the empty alts fall through to the close on #ZZ. The names are
  // minted to tins here — the same table the matchers' lex.token calls use.
  const toks = INLINE_TOKENS.map((n) => tn.token(n))

  tn.rule('inline', (rs) => {
    return rs
      .clear()
      .open([{ s: [toks], r: 'inline' }, {}])
      .close([
        {
          s: '#ZZ',
          a: (_r: any, ctx: Context) => {
            const st: InlineState = (ctx.u as any).inl
            st.parser.finishBlock(st.block)
          },
        },
        {},
      ])
  })

  return tn
}

/**
 * The engine-path inline phase: the same block walk as `parseInlines`, with
 * each block's scan driven by the engine instance's lexer. An empty subject
 * never reaches the engine (its empty-source path returns before the rules
 * run), and finishes the block directly — a no-op resolve, exactly as the
 * hand scanner's `parse` does.
 */
export function parseInlinesEngine(
  inlineTn: Tabnas,
  doc: MdNode,
  refmap: RefMap,
  options: ParserOptions,
): void {
  const parser = new InlineParser(refmap, options)

  forEachInlineBlock(doc, options, (block) => {
    if (!parser.beginBlock(block)) {
      parser.finishBlock(block)
      return
    }
    inlineTn.parse(parser.subject, { md: { inl: parser, block } })
  })
}
