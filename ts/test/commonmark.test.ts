/* Copyright (c) 2025-2026 Richard Rodger and other contributors, MIT License */

// CommonMark 0.31.2 conformance, over the vendored 652-example suite in
// `test/commonmark/spec.json`.
//
// This imports the built parser directly rather than through `markdown.ts`,
// so it exercises no engine code and stays runnable when `@tabnas/parser` is
// absent. `ts/tools/conformance.mjs` runs the same corpus straight off the
// TypeScript sources with no build step, and reports a per-section table;
// this file is the version that belongs to `npm test`.
//
// The suite is pure CommonMark, so GFM must be off — strikethrough and
// autolink literals change the expected output for several examples.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { parse } from '../dist/commonmark'
import { renderHTML } from '../dist/html'

type SpecCase = {
  markdown: string
  html: string
  example: number
  section: string
}

// At runtime this file is loaded from `dist-test/`, so hop up one level to
// reach the shared corpus in the repo root.
const specFile = join(__dirname, '..', '..', 'test', 'commonmark', 'spec.json')
const cases: SpecCase[] = JSON.parse(readFileSync(specFile, 'utf8'))

const OPTS = { gfm: false, breaks: false }

const sections = [...new Set(cases.map((c) => c.section))]

describe('commonmark-0.31.2', () => {
  test('corpus is intact', () => {
    assert.equal(cases.length, 652, 'expected the full 0.31.2 suite')
    assert.equal(sections.length, 26)
  })

  for (const section of sections) {
    describe(section, () => {
      for (const c of cases.filter((x) => x.section === section)) {
        test(`example ${c.example}`, () => {
          assert.equal(
            renderHTML(parse(c.markdown, OPTS), OPTS),
            c.html,
            'markdown: ' + JSON.stringify(c.markdown),
          )
        })
      }
    })
  }
})

describe('commonmark-options', () => {
  // The corpus only ever exercises {gfm:false, breaks:false}. Every other
  // combination has to at least parse without throwing, on every example.
  for (const gfm of [true, false]) {
    for (const breaks of [true, false]) {
      test(`no example throws with gfm:${gfm} breaks:${breaks}`, () => {
        for (const c of cases) {
          assert.doesNotThrow(
            () => renderHTML(parse(c.markdown, { gfm, breaks }), { gfm, breaks }),
            'example ' + c.example,
          )
        }
      })
    }
  }
})
