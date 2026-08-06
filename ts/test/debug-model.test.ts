/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

/*  debug-model.test.ts
 *  Composition test: the markdown prose grammar plugin layered with the
 *  official @tabnas/debug plugin. The probe is the structured
 *  model — not CSV's old record/list/text chain.
 */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { createRequire } from 'node:module'

import { Tabnas } from '@tabnas/parser'
import { Markdown } from '../dist/markdown'

const req = createRequire(__filename)

function loadDebug(): any {
  const candidates = [process.env.TABNAS_DEBUG_PATH, '@tabnas/debug'].filter(
    Boolean,
  ) as string[]
  for (const c of candidates) {
    try {
      return req(c).Debug
    } catch {
      /* try next */
    }
  }
  return null
}

const Debug = loadDebug()
const skip = Debug ? false : '@tabnas/debug not available (set TABNAS_DEBUG_PATH)'

describe('compose: markdown + @tabnas/debug', () => {
  test('parses normally with the debug plugin installed', { skip }, () => {
    const tn = new Tabnas().use(Markdown)
    tn.use(Debug, { print: false, trace: false })
    const doc = tn.parse('# hi\n\nHello *world*')
    assert.equal(doc.type, 'document')
    assert.equal(doc.children[0].type, 'heading')
    assert.equal(doc.children[1].type, 'paragraph')
  })

  test('debug.model() returns the structured grammar', { skip }, () => {
    const tn = new Tabnas().use(Markdown)
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()

    // Prose parser is bare-engine: only the `markdown` entry rule is
    // installed. The old CSV `record`/`list`/`text` rules are gone.
    assert.deepStrictEqual(
      m.rules.map((r: any) => r.name).sort(),
      ['markdown'],
    )

    // Entry rule is `markdown`.
    assert.equal(m.config.start, 'markdown')
    assert.ok(
      m.plugins.some((p: any) => p.name === 'Markdown'),
      'plugins should list Markdown',
    )

    const markdown = m.rules.find((r: any) => r.name === 'markdown')
    assert.ok(markdown, 'markdown rule should exist')
    // The markdown rule consumes the token stream via #AA repetition
    // so the engine's trailing-content check passes. The actual block
    // structure lives in JS (parseDocument), not in declarative alts.
    assert.ok(Array.isArray(markdown.open), 'markdown.open should exist')
    assert.ok(Array.isArray(markdown.close), 'markdown.close should exist')

    // JSON round-trip
    const grammar = {
      tokens: m.tokens,
      rules: m.rules,
      graph: m.graph,
      config: m.config,
      abnf: m.abnf,
    }
    assert.deepStrictEqual(JSON.parse(JSON.stringify(grammar)).rules, m.rules)
  })
})
