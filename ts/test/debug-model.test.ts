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

    // The engine block driver's grammar: `markdown` pushes `children`, the
    // loop of a container's children, and every node of the AST is a rule
    // of its own, named for its type (see engine-block.ts).
    assert.deepStrictEqual(
      m.rules.map((r: any) => r.name).sort(),
      [
        'blockquote',
        'break',
        'children',
        'code',
        'definition',
        'delete',
        'emphasis',
        'heading',
        'html',
        'image',
        'imageReference',
        'inlineCode',
        'link',
        'linkReference',
        'list',
        'listItem',
        'markdown',
        'paragraph',
        'strong',
        'table',
        'tableCell',
        'tableRow',
        'text',
        'thematicBreak',
      ],
    )

    // Entry rule is `markdown`.
    assert.equal(m.config.start, 'markdown')
    assert.ok(
      m.plugins.some((p: any) => p.name === 'Markdown'),
      'plugins should list Markdown',
    )

    const markdown = m.rules.find((r: any) => r.name === 'markdown')
    assert.ok(markdown, 'markdown rule should exist')
    assert.ok(Array.isArray(markdown.open), 'markdown.open should exist')
    assert.ok(Array.isArray(markdown.close), 'markdown.close should exist')

    // The `children` loop is where the engine drives the parse: one open
    // alt per node (each pushing the node's rule), one for a line, and the
    // fall-through; its close replaces it with the next iteration. The
    // block *decisions* are the shared engine-free core's, by design
    // (dx-report §42); the rules open and close each block on them.
    const children = m.rules.find((r: any) => r.name === 'children')
    assert.ok(children, 'children rule should exist')
    assert.ok(Array.isArray(children.open), 'children.open should exist')
    assert.equal(children.open.length, 24)
    assert.equal(children.close.length, 2)
  })

  test('gfm:false subtracts the tagged alts from the grammar', { skip }, () => {
    // The GFM dialect is a group-tagged extension, removed by rule.exclude —
    // the base grammar visibly has no table and no strikethrough to push,
    // not a disabled branch.
    const tn = new Tabnas().use(Markdown, { gfm: false })
    tn.use(Debug, { print: false, trace: false })
    const m = tn.debug.model()
    const children = m.rules.find((r: any) => r.name === 'children')
    assert.ok(children, 'children rule should exist')
    assert.equal(children.open.length, 20)

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
