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
import { parseDocument, toHtml } from '../dist/markdown'

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

describe('sourcepos', () => {
  // A column counts CHARACTERS, not UTF-16 units. `ln.length` would advance
  // the column by two for an astral character, which is how the Go port and
  // this one came to disagree on `😀 *x*` — Go counted 5, TypeScript 6. The
  // AST does not carry sourcepos, so an AST-level parity check cannot see a
  // regression here.
  const endColumn = (src: string) => {
    const w = parse(src, OPTS).walker()
    let e
    while ((e = w.next())) {
      if (e.entering && 'paragraph' === e.node.type) return e.node.sourcepos[1][1]
    }
    return -1
  }

  test('columns count characters, not UTF-16 units', () => {
    assert.equal(endColumn('abc *x*\n'), 7)
    assert.equal(endColumn('é *x*\n'), 5, 'BMP: one unit, one character')
    assert.equal(endColumn('😀 *x*\n'), 5, 'astral: two units, still one character')
    assert.equal(endColumn('𝔘𝔘 *x*\n'), 6)
  })

  test('nested containers do not cost quadratic time', () => {
    // Counting characters from the start of the line on every opened
    // container would be O(n^2), and untrusted input picks n.
    //
    // Timing assertions on shared CI runners need care. The first version of
    // this used Date.now() with a 1ms floor at sizes that ran in single-digit
    // milliseconds, so quantisation alone could produce a 12x "ratio" — which
    // is exactly how it failed on macOS. Sub-millisecond clock, best-of-5 to
    // shed scheduler and GC noise, and sizes large enough that the baseline is
    // tens of milliseconds rather than ones.
    const measure = (n: number) => {
      const src = '> - '.repeat(n) + 'x'
      let best = Infinity
      for (let i = 0; i < 5; i++) {
        const t0 = performance.now()
        parse(src, OPTS)
        best = Math.min(best, performance.now() - t0)
      }
      return best
    }

    measure(2000) // warm up the JIT
    const base = measure(10000)
    const large = measure(40000)

    // Below this the measurement is noise, not signal; assert nothing rather
    // than assert something unreliable.
    if (5 > base) return

    const ratio = large / base
    assert.ok(
      12 > ratio,
      `4x input took ${ratio.toFixed(1)}x time (${base.toFixed(1)}ms -> ${large.toFixed(1)}ms); ` +
        'linear is ~4x, quadratic ~16x',
    )
  })
})

describe('ast projection', () => {
  // Codex review on PR #28 caught both of these; neither was visible to the
  // conformance suite, which only ever compares HTML.

  test('destinations are decoded in the AST, encoded only in HTML', () => {
    // normalizeURI belongs to rendering. Applying it at parse time was
    // invisible in the HTML (it is idempotent over its own output) but put
    // `/%C3%A4` in the public AST where mdast promises `/ä`.
    const doc: any = parseDocument('[l](/ä)', { gfm: false })
    assert.equal(doc.children[0].children[0].url, '/ä')
    assert.equal(toHtml('[l](/ä)', { gfm: false }), '<p><a href="/%C3%A4">l</a></p>\n')

    const img: any = parseDocument('![i](/ö)', { gfm: false })
    assert.equal(img.children[0].children[0].url, '/ö')

    const auto: any = parseDocument('<https://x.com/ä>', { gfm: false })
    assert.equal(auto.children[0].children[0].url, 'https://x.com/ä')

    // An already-encoded destination is left alone, not double-encoded.
    const pre: any = parseDocument('[l](/a%20b)', { gfm: false })
    assert.equal(pre.children[0].children[0].url, '/a%20b')
    assert.equal(toHtml('[l](/a%20b)', { gfm: false }), '<p><a href="/a%20b">l</a></p>\n')
  })

  test('deep container nesting does not overflow the stack', () => {
    // 8 KB of Markdown. The block parser and the renderer always handled this;
    // the projection recursed once per container and threw RangeError at ~4000,
    // so untrusted input could crash the primary API.
    for (const depth of [4000, 20000]) {
      const src = '> '.repeat(depth) + 'x\n'
      assert.doesNotThrow(() => parseDocument(src, { gfm: false }), `depth ${depth}`)
    }
  })
})
