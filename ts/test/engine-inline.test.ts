/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Pins for the engine inline driver's structural contract (engine-inline.ts).

import { test } from 'node:test'
import assert from 'node:assert'

import { makeInlineTn, INLINE_TOKENS } from '../dist/engine-inline'
import { resolveOptions } from '../dist/options'

test('inline rule alts declare single-token lookahead', () => {
  // The inline matchers apply their one-token effects at lex time, because
  // the engine's rule loop reads lookahead tokens before earlier alts'
  // actions run. That is only sound while no alt asks the lexer to run
  // further ahead than one token — an alt with deeper lookahead would have
  // the `]` matcher consult a bracket stack whose earlier effects had not
  // been applied yet. Pin sN <= 1 so the invariant cannot erode silently.
  const tn: any = makeInlineTn(resolveOptions({}))
  const rs: any = tn.rule('inline')
  const alts = [...(rs.def.open ?? []), ...(rs.def.close ?? [])]
  assert.ok(0 < alts.length, 'inline rule has alts')
  for (const alt of alts) {
    const s = alt && alt.s
    const positions = null == s ? 0 : 'string' === typeof s ? 1 : s.length
    assert.ok(positions <= 1, 'inline alt lookahead must be <= 1, got ' + positions)
  }
})

test('the token alphabet is complete and stable', () => {
  assert.deepStrictEqual(INLINE_TOKENS, [
    '#IBK', '#IES', '#ICS', '#IDL', '#IOB', '#IBG',
    '#ICB', '#IAL', '#IHT', '#IEN', '#ITX', '#ILI',
  ])
})
