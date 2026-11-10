/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// A parse from a reader (`tn.parseReader`, admin ADR-30).
//
// `make()` declares `lex.window`: the one matcher cuts a line through the
// lexer's window and fills it past the line's end, and nothing else reads the
// source, so a reader is read a window at a time. Every row of the shared
// fixtures and every document of both conformance corpora parses the same
// from a reader, handed over a few bytes at a time and under windows whose
// margin and block are a few units (`parseReaderWithSizes`, which makes the
// matcher meet the window's edge in every line), as from its text: the same
// value, member order included. So do long documents whose lines, code
// blocks and tables are longer than the window's margin, and a long document
// from a reader parses in time linear in its length. rs/tests/reader_test.rs
// and go/reader_test.go hold the other runtimes to the same.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { findSpecDir, loadSpecDir } from '@tabnas/support'

import { Markdown, make } from '../dist/markdown'

const specDir = findSpecDir(__dirname)

// The pieces a reader hands its bytes over in.
const PIECES = [1, 3, 64]

// Windows whose margin and block are a few units.
const WINDOWS: [number, number][] = [
  [1, 1],
  [3, 2],
  [64, 64],
]

type Reader = { read(buf: Uint8Array): number }

// A reader that hands its bytes over at most `size` at a time.
function pieces(bytes: Uint8Array, size: number): Reader {
  let at = 0
  return {
    read(buf: Uint8Array): number {
      const n = Math.min(size, buf.length, bytes.length - at)
      buf.set(bytes.subarray(at, at + n))
      at += n
      return n
    },
  }
}

// A value as text with its members in their order, so that two values
// compare as the walk of each would write them.
function render(value: any): string {
  if (Array.isArray(value)) return '[' + value.map(render).join(',') + ']'
  if (null !== value && 'object' === typeof value) {
    return (
      '{' +
      Object.keys(value)
        .map((k) => JSON.stringify(k) + ':' + render(value[k]))
        .join(',') +
      '}'
    )
  }
  return JSON.stringify(value)
}

// Every way this test reads `bytes` from a reader, each with its value: in
// every size of pieces under the window's own sizes, and under every small
// window.
function readings(tn: any, bytes: Uint8Array, sizes = PIECES): [string, string][] {
  const out: [string, string][] = []
  for (const size of sizes) {
    out.push([`pieces of ${size}`, render(tn.parseReader(pieces(bytes, size)))])
    for (const [margin, block] of WINDOWS) {
      out.push([
        `pieces of ${size}, margin ${margin}, block ${block}`,
        render(tn.parseReaderWithSizes(pieces(bytes, size), { margin, block, chunk: size })),
      ])
    }
  }
  return out
}

type SpecCase = { markdown: string; example: number }

function corpus(name: string): SpecCase[] {
  return JSON.parse(readFileSync(join(specDir, '..', name, 'spec.json'), 'utf8'))
}

describe('reader', () => {
  test('make reads a reader a window at a time', () => {
    assert.equal(make().config().lex.window, true)
    assert.equal(make({ gfm: false }).config().lex.window, true)
    // The plugin leaves the window to the parser that installs it.
    assert.equal(new Tabnas().use(Markdown).config().lex.window, false)
  })

  test('every row parses the same from a reader', () => {
    let compared = 0
    for (const spec of loadSpecDir(specDir)) {
      for (const row of spec.rows) {
        const opts = row.named('opts')
        const tn = make('' === opts.trim() ? {} : JSON.parse(opts))
        const input = row.unescNamed('input')
        const whole = render(tn.parse(input))
        for (const [how, read] of readings(tn, Buffer.from(input, 'utf8'))) {
          assert.equal(read, whole, `${spec.file} ${row.where()} in ${how}`)
          compared++
        }
      }
    }
    assert.ok(0 < compared)
  })

  test('every CommonMark example parses the same from a reader', () => {
    const tn = make({ gfm: false })
    const cases = corpus('commonmark')
    assert.equal(cases.length, 652)
    cases.forEach((c, index) => {
      const whole = render(tn.parse(c.markdown))
      for (const [how, read] of readings(tn, Buffer.from(c.markdown, 'utf8'))) {
        assert.equal(read, whole, `example ${c.example} in ${how}`)
      }
      if (0 === (index + 1) % 100 || index + 1 === cases.length) {
        console.log(
          `reader corpus: ${index + 1} of ${cases.length} ` +
            `(${Math.floor(((index + 1) * 100) / cases.length)}%)`,
        )
      }
    })
  })

  test('every GFM example parses the same from a reader', () => {
    const tn = make()
    const cases = corpus('gfm')
    assert.equal(cases.length, 24)
    for (const c of cases) {
      const whole = render(tn.parse(c.markdown))
      for (const [how, read] of readings(tn, Buffer.from(c.markdown, 'utf8'))) {
        assert.equal(read, whole, `gfm example ${c.example} in ${how}`)
      }
    }
  })

  test('line endings of every kind, at the window edge', () => {
    // A `\r\n` cut between two reads, a lone `\r`, a final line with and
    // without its ending, and characters past ASCII on either side.
    const tn = make()
    const docs = [
      'a\r\nb\r\n\r\nc',
      'a\rb\r\rc\r',
      '# é\r\n> ñ\r- €\n  𝄞\r\n',
      '\r\n\r\n',
      '\r',
      'x',
      '| a | b |\r\n| - | - |\r\n| 1 | 2 |\r',
    ]
    for (const d of docs) {
      const whole = render(tn.parse(d))
      for (const [how, read] of readings(tn, Buffer.from(d, 'utf8'))) {
        assert.equal(read, whole, JSON.stringify(d) + ' in ' + how)
      }
    }
  })

  test('long lines, code blocks and tables parse the same from a reader', () => {
    // Each construct longer than the window's 64 KiB margin, so the window
    // grows to hold a line, and a code block and a table of many lines. The
    // text is plain, a few nodes a line: the engine keeps every rule a parse
    // passes through unless its history is bounded (as a transducer's source
    // bounds it), so a document of many nodes costs heap the length alone
    // does not.
    const tn = make()
    const long = 'ab é€𝄞 cd '.repeat(9000) + '*x* [r]'
    const doc =
      '# ' + long + '\n\n' +
      long + '\n' + long + '\n\n' +
      '```\n' + (long + '\n').repeat(3) + '```\n\n' +
      '| a | b |\n| - | - |\n' + ('| ' + long + ' | c |\n').repeat(3) + '\n' +
      '> ' + long + '\n> - ' + long + '\n\n' +
      '[r]: /u\n'
    const whole = render(tn.parse(doc))
    const bytes = Buffer.from(doc, 'utf8')
    for (const size of [4096, 64 * 1024 + 7]) {
      assert.equal(render(tn.parseReader(pieces(bytes, size))), whole, `pieces of ${size}`)
    }
    assert.equal(
      render(tn.parseReaderWithSizes(pieces(bytes, 5), { margin: 16, block: 16, chunk: 7 })),
      whole,
      'a small window',
    )
  })

  test('a long document from a reader parses in time linear in its length', () => {
    const tn = make()
    const unit =
      '# Heading\n\nSome *text* and a [ref] with `code`.\n\n' +
      '- a\n- b\n  > quoted\n\n' +
      '| x | y |\n| - | - |\n| 1 | 2 |\n\n' +
      '```js\nlet x = 1\n```\n\n[ref]: /u "t"\n\n'
    const time = (n: number): number => {
      const bytes = Buffer.from(unit.repeat(n), 'utf8')
      const t0 = process.hrtime.bigint()
      tn.parseReader(pieces(bytes, 4096))
      return Number(process.hrtime.bigint() - t0)
    }
    // Sized for the engine's default, which keeps every rule a parse passes
    // through until it ends (a transducer's source bounds that history).
    time(50) // warm
    const small = time(100)
    const large = time(1000)
    // Ten times the document, about ten times the time; well under the
    // hundred a quadratic parse would take.
    assert.ok(large < 30 * small, `100 units ${small / 1e6} ms, 1000 units ${large / 1e6} ms`)
    console.log(`reader: 100 units ${(small / 1e6).toFixed(1)} ms, 1000 units ${(large / 1e6).toFixed(1)} ms`)
  })
})
