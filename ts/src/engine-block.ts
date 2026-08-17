/* Copyright (c) 2026 Richard Rodger, MIT License */

// The engine-facing block driver: a custom lexer matcher that turns the
// engine's scan into one `#LB` token per physical line, and the actions that
// feed those lines to the shared `BlockParser`.
//
// This is the seam of the anti-drift rule (dx-report §42): nothing here
// recognizes anything. Line cutting and NUL replacement come from
// `segmentNextLine`, blank detection from `isBlank`, and every block
// decision — continuation, starts, lazy continuation, finalization — from
// the same `BlockParser` the engine-free path runs. The engine owns
// tokenization, dispatch, per-parse state carriage and observability; the
// algorithm stays in the shared core.
//
// This module may import engine types: it is reachable only from
// `markdown.ts`, never from `commonmark.ts`, so the engine-free layering
// rule holds (see AGENTS.md).

import type { Context, MakeLexMatcher, Rule, Tabnas } from '@tabnas/parser'

import { BlockParser, isBlank, segmentNextLine } from './block.ts'
import { toAst } from './ast.ts'
import type { MdNode } from './node.ts'
import type { ParserOptions } from './options.ts'

/** The `#LB` token's `use` payload: one physical line, pre-classified. */
export type LineInfo = {
  /** The line's text, terminator excluded, NULs already replaced. */
  text: string
  /** True when the line holds nothing but spaces and tabs. */
  blank: boolean
  /**
   * GFM table arming: true when the line contains a `|` anywhere. A
   * deliberate superset of "is a delimiter row" — the delimiter row of a
   * table nested in a block quote only matches at the container-adjusted
   * offset, which only the block algorithm knows. `tryOpenTable`
   * re-verifies; this bit exists so a `gfm`-gated alt has something honest
   * to read (see test/spec/mixed.tsv).
   */
  tblArm: boolean
}

/** The per-parse state carried on `ctx.u.md` — seeded by `parse.prepare`. */
export type MdParseState = {
  bp: BlockParser
  meta: Record<string, any> | undefined
}

/**
 * Build the `mdLine` matcher: each invocation consumes one physical line —
 * terminator included — and emits a single `#LB` token carrying the line as
 * `LineInfo`. The matcher owns the engine's point: `sI` advances past the
 * terminator, `rI` counts rows, and `cI` resets per line (the engine's
 * column is not used for indentation — the shared `BlockParser` computes
 * spec §2.2 tab-expanded columns itself).
 */
export const makeMdLineMatcher: MakeLexMatcher = (_cfg, _opts) => {
  return (lex: any, _rule: Rule) => {
    const pnt = lex.pnt
    const src: string = lex.src

    const seg = segmentNextLine(src, pnt.sI)
    if (null === seg) return undefined

    const info: LineInfo = {
      text: seg.text,
      blank: isBlank(seg.text),
      tblArm: -1 !== seg.text.indexOf('|'),
    }

    const tkn = lex.token('#LB', seg.text, src.slice(pnt.sI, seg.next), pnt, info)

    // Row/column bookkeeping is observability data (#ZZ position, traces):
    // a consumed terminator starts a new row; an unterminated final line
    // just widens the current one.
    const terminated = seg.next > pnt.sI + seg.text.length
    pnt.sI = seg.next
    if (terminated) {
      pnt.rI += 1
      pnt.cI = 1
    } else {
      pnt.cI += seg.text.length
    }

    return tkn
  }
}

/**
 * The three named actions of the `markdown`/`line` rules. Bound to resolved
 * options at plugin-install time, exactly as the engine-free entry points
 * are. `runInlines` is the phase-2 driver — the engine inline path from
 * `engine-inline.ts` — injected so this module stays a block-phase concern.
 */
export function makeMdActions(
  opts: ParserOptions,
  runInlines: (doc: MdNode, refmap: Record<string, any>) => void,
) {
  return {
    /** `parse.prepare`: fresh block-parser state for every parse. */
    prepare: (_tn: Tabnas, ctx: Context, meta?: any) => {
      const state: MdParseState = { bp: new BlockParser(opts), meta }
      ;(ctx.u as any).md = state
    },

    /** `line` open alt: feed the matched `#LB` line to the shared algorithm. */
    line: (r: Rule, ctx: Context) => {
      const state: MdParseState = (ctx.u as any).md
      state.bp.incorporateLine((r.o0 as any).use.text)
    },

    /**
     * `markdown` close alt (`#ZZ`): finalize blocks, run the shared inline
     * phase, and project the public AST into the rule's node. When the
     * caller passed `meta.md.keepTree`, the native tree is handed back on
     * the meta object — the engine-path conformance harness renders it.
     */
    finish: (r: Rule, ctx: Context) => {
      const state: MdParseState = (ctx.u as any).md
      const { doc, refmap } = state.bp.finish()
      runInlines(doc, refmap)

      const md = state.meta && state.meta.md
      if (md && true === md.keepTree) md.tree = doc as MdNode

      r.node = toAst(doc, opts)
    },
  }
}
