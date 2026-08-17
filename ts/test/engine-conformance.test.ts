/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Engine-path conformance: the full CommonMark 0.31.2 suite (652 examples,
// gfm:false) and the GFM extension corpus (24 examples, gfm:true) run
// through `new Tabnas().use(Markdown).parse(...)` — the engine lexing lines,
// the rules dispatching them — with byte-for-byte HTML comparison and an
// AST comparison against the engine-free path.
//
// `ts/test/commonmark.test.ts` asserts the same corpus over the engine-free
// modules; this suite is the other leg of the dual-path contract
// (dx-report §42): the conformance claim holds on the code path the plugin
// actually runs, not just on the reference implementation. The native tree
// comes back via `meta.md.keepTree` and is rendered with the same
// `renderHTML` the engine-free path uses, so the comparison isolates the
// parse — a difference here is a parsing difference, never a rendering one.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'

import { Markdown, parseDocument, renderHTML } from '../dist/markdown'

type SpecCase = {
  markdown: string
  html: string
  example: number
  section: string
}

const commonmarkCases: SpecCase[] = JSON.parse(
  readFileSync(join(__dirname, '..', '..', 'test', 'commonmark', 'spec.json'), 'utf8'),
)
const gfmCases: SpecCase[] = JSON.parse(
  readFileSync(join(__dirname, '..', '..', 'test', 'gfm', 'spec.json'), 'utf8'),
)

// One instance per option set, reused across the corpus (the supported
// pattern — see perf.test.ts).
const CM_OPTS = { gfm: false, breaks: false }
const cmInstance = new Tabnas().use(Markdown, CM_OPTS)
const gfmInstance = new Tabnas().use(Markdown, {})

function checkExample(tn: any, c: SpecCase, opts: Record<string, unknown>): void {
  const meta = { md: { keepTree: true, tree: null as any } }
  const ast = tn.parse(c.markdown, meta)

  assert.ok(null !== meta.md.tree, 'keepTree returned no native tree')
  assert.equal(
    renderHTML(meta.md.tree),
    c.html,
    'engine-path HTML, markdown: ' + JSON.stringify(c.markdown),
  )
  assert.deepStrictEqual(
    JSON.parse(JSON.stringify(ast)),
    JSON.parse(JSON.stringify(parseDocument(c.markdown, opts))),
    'engine-path AST, markdown: ' + JSON.stringify(c.markdown),
  )
}

describe('engine-path commonmark-0.31.2', () => {
  test('corpus is intact', () => {
    assert.equal(commonmarkCases.length, 652, 'expected the full 0.31.2 suite')
  })

  const sections = [...new Set(commonmarkCases.map((c) => c.section))]
  for (const section of sections) {
    describe(section, () => {
      for (const c of commonmarkCases.filter((x) => x.section === section)) {
        test(`example ${c.example}`, () => {
          checkExample(cmInstance, c, CM_OPTS)
        })
      }
    })
  }
})

describe('engine-path gfm extensions', () => {
  test('corpus is intact', () => {
    assert.equal(gfmCases.length, 24, 'expected the full extension corpus')
  })

  for (const c of gfmCases) {
    test(`example ${c.example} (${c.section})`, () => {
      checkExample(gfmInstance, c, {})
    })
  }
})
