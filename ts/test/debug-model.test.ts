/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

/*  debug-model.test.ts
 *  Composition test: the markdown grammar plugin layered with the official
 *  @tabnas/debug plugin. @tabnas/debug is a devDependency, but it is resolved
 *  dynamically and the test SKIPS when it is absent so the suite stays
 *  runnable outside the package; set TABNAS_DEBUG_PATH to point at a sibling
 *  checkout's built plugin.
 */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { createRequire } from 'node:module'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
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
    const tn = new Tabnas().use(jsonic).use(Markdown)
    tn.use(Debug, { print: false, trace: false })
    assert.deepEqual(tn.parse('a,b\nA,B'), [{ a: 'A', b: 'B' }])
  })

  test('debug.model() returns the structured grammar', { skip }, () => {
    const tn = new Tabnas().use(jsonic).use(Markdown)
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()

    // The structured rule set: the jsonic base (elem/list/map/pair/val) plus
    // the markdown-specific rules (markdown/newline/record/text).
    assert.deepStrictEqual(
      m.rules.map((r: any) => r.name).sort(),
      ['elem', 'list', 'map', 'markdown', 'newline', 'pair', 'record', 'text', 'val'],
    )

    // The markdown grammar overrides the entry rule to `markdown`.
    assert.equal(m.config.start, 'markdown')

    // The plugin chain is recorded, including the markdown plugin.
    assert.ok(
      m.plugins.some((p: any) => p.name === 'Markdown'),
      'plugins should list Markdown',
    )

    // Structural fact: the `markdown` entry rule opens into `newline` (blank
    // lines) and `record` (a row of fields).
    const markdown = m.rules.find((r: any) => r.name === 'markdown')
    assert.ok(markdown, 'markdown rule should exist')
    assert.ok(
      markdown.open.some((a: any) => a.push === 'record'),
      'markdown should push record',
    )
    assert.ok(
      markdown.open.some((a: any) => a.push === 'newline'),
      'markdown should push newline',
    )

    // Structural fact: a `record` opens by pushing the `list` rule (a record
    // is parsed as a comma-separated list of fields).
    const record = m.rules.find((r: any) => r.name === 'record')
    assert.ok(record, 'record rule should exist')
    assert.ok(
      record.open.some((a: any) => a.push === 'list'),
      'record should push list',
    )

    // The grammar portion is JSON-serialisable and round-trips.
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
