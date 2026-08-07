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
const GFM = { gfm: true, breaks: false }

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

// ---------------------------------------------------------------------------
// GFM extensions
//
// Five are implemented — tables, strikethrough, task list items, disallowed
// raw HTML and autolink literals — and all five are gated on `gfm`.
//
// `ts/tools/gfm-conformance.mjs` reports these as a per-section table; this is
// the version that belongs to `npm test`.

const gfmSpecFile = join(__dirname, '..', '..', 'test', 'gfm', 'spec.json')
const gfmCases: SpecCase[] = JSON.parse(readFileSync(gfmSpecFile, 'utf8'))

// Every vendored extension section must be complete.
const IMPLEMENTED_SECTIONS = [
  'Tables (extension)',
  'Task list items (extension)',
  'Strikethrough (extension)',
  'Autolinks (extension)',
  'Disallowed Raw HTML (extension)',
]

describe('gfm-extensions corpus', () => {
  test('corpus is intact', () => {
    assert.equal(gfmCases.length, 24)
  })

  for (const section of IMPLEMENTED_SECTIONS) {
    describe(section, () => {
      const inSection = gfmCases.filter((c) => c.section === section)
      test('section is non-empty', () => assert.ok(0 < inSection.length))
      for (const c of inSection) {
        test(`example ${c.example}`, () => {
          assert.equal(
            renderHTML(parse(c.markdown, GFM), GFM),
            c.html,
            'markdown: ' + JSON.stringify(c.markdown),
          )
        })
      }
    })
  }
})

describe('gfm tables', () => {
  const html = (src: string) => toHtml(src, GFM)

  // A table's HTML is verbose enough that a literal expectation buries the
  // thing being tested, so rows are assembled from their cells here. Newline
  // placement is still byte-exact — that is what the corpus judges.
  const tr = (cells: string[], tag: 'th' | 'td') =>
    '<tr>\n' + cells.map((c) => c + '\n').join('') + '</tr>\n'
  const thead = (cells: string[]) => '<thead>\n' + tr(cells, 'th') + '</thead>\n'
  const tbody = (rows: string[][]) =>
    '<tbody>\n' + rows.map((r) => tr(r, 'td')).join('') + '</tbody>\n'
  const table = (head: string, body?: string) =>
    '<table>\n' + head + (undefined === body ? '' : body) + '</table>\n'

  const th = (s: string, align?: string) =>
    '<th' + (align ? ` align="${align}"` : '') + '>' + s + '</th>'
  const td = (s: string, align?: string) =>
    '<td' + (align ? ` align="${align}"` : '') + '>' + s + '</td>'

  // --- the eight spec behaviours, each asserted on its own ---

  test('1. header row, delimiter row, data row', () => {
    assert.equal(
      html('| foo | bar |\n| --- | --- |\n| baz | bim |\n'),
      table(thead([th('foo'), th('bar')]), tbody([[td('baz'), td('bim')]])),
    )
  })

  test('2. colons set alignment, and pipes may be inconsistent', () => {
    // Cell widths need not match, and a leading/trailing pipe is optional on
    // every row independently.
    assert.equal(
      html('| abc | defghi |\n:-: | -----------:\nbar | baz\n'),
      table(
        thead([th('abc', 'center'), th('defghi', 'right')]),
        tbody([[td('bar', 'center'), td('baz', 'right')]]),
      ),
    )
    // All four delimiter shapes, and `null` means no attribute at all.
    assert.equal(
      html('| a | b | c | d |\n| :- | -: | :-: | --- |\n| 1 | 2 | 3 | 4 |\n'),
      table(
        thead([th('a', 'left'), th('b', 'right'), th('c', 'center'), th('d')]),
        tbody([[td('1', 'left'), td('2', 'right'), td('3', 'center'), td('4')]]),
      ),
    )
  })

  test('3. an escaped pipe is content, inside other inline spans too', () => {
    assert.equal(
      html('| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| b **\\|** im |\n'),
      table(
        thead([th('f|oo')]),
        tbody([[td('b <code>|</code> az')], [td('b <strong>|</strong> im')]]),
      ),
    )
  })

  test('4. another block-level structure breaks the table', () => {
    assert.equal(
      html('| abc | def |\n| --- | --- |\n| bar | baz |\n> bar\n'),
      table(thead([th('abc'), th('def')]), tbody([[td('bar'), td('baz')]])) +
        '<blockquote>\n<p>bar</p>\n</blockquote>\n',
    )
  })

  test('5. a blank line breaks the table', () => {
    assert.equal(
      html('| abc | def |\n| --- | --- |\n| bar | baz |\nbar\n\nbar\n'),
      table(
        thead([th('abc'), th('def')]),
        tbody([
          [td('bar'), td('baz')],
          [td('bar'), td('')],
        ]),
      ) + '<p>bar</p>\n',
    )
  })

  test('6. header and delimiter must agree on the cell count', () => {
    // Not a table at all — the whole thing stays one paragraph.
    assert.equal(
      html('| abc | def |\n| --- |\n| bar |\n'),
      '<p>| abc | def |\n| --- |\n| bar |</p>\n',
    )
    // The mismatch is on the count, not the widths: 1-vs-1 and 3-vs-3 match.
    assert.equal(html('| abc |\n| --- |\n'), table(thead([th('abc')])))
    assert.equal(html('| abc | def |\n| --- | --- | --- |\n').startsWith('<p>'), true)
  })

  test('7. short rows are padded, long rows are truncated', () => {
    assert.equal(
      html('| abc | def |\n| --- | --- |\n| bar |\n| bar | baz | boo |\n'),
      table(
        thead([th('abc'), th('def')]),
        tbody([
          [td('bar'), td('')],
          [td('bar'), td('baz')],
        ]),
      ),
    )
  })

  test('8. no body rows means no <tbody> at all', () => {
    const out = html('| abc | def |\n| --- | --- |\n')
    assert.equal(out, table(thead([th('abc'), th('def')])))
    assert.ok(!out.includes('tbody'))
  })

  // --- the rules those examples rest on ---

  test('a delimiter cell is hyphens and at most two colons', () => {
    // Leading and trailing pipes are optional here as everywhere. A row of
    // nothing but hyphens (`-`, `---`) is left out: it is a setext underline
    // first, which the test below covers.
    for (const delim of [
      '| --- |', '| :-- |', '| --: |', '| :-: |', '| - |', '|-|', '|--', '--|',
      ':-', '-:', ':-:', ':---:',
    ]) {
      assert.ok(html('| a |\n' + delim + '\n').startsWith('<table>'), delim)
    }
    for (const delim of [
      '| === |', '| -+- |', '| - - |', '| :: |', '| :  |', '| a |', '|  |', '| ::- |',
      '| -:- |', '| *** |', '| — |',
    ]) {
      assert.ok(html('| a |\n' + delim + '\n').startsWith('<p>'), delim)
    }
  })

  test('spaces and tabs between pipes and content are trimmed', () => {
    assert.equal(
      html('|   a   |\t b\t|\n| - | - |\n|\tx\t|   y   |\n'),
      table(thead([th('a'), th('b')]), tbody([[td('x'), td('y')]])),
    )
  })

  test('the header row is the paragraph’s last line, so the paragraph splits', () => {
    assert.equal(
      html('aaa\nbbb\n| a | b |\n| - | - |\n| c | d |\n'),
      '<p>aaa\nbbb</p>\n' +
        table(thead([th('a'), th('b')]), tbody([[td('c'), td('d')]])),
    )
    // The lines left behind are a real paragraph: their reference definitions
    // still register, because splitting finalizes them.
    assert.equal(
      html('[r]: /url\n| a |\n| - |\n\n[r]\n'),
      table(thead([th('a')])) + '<p><a href="/url">r</a></p>\n',
    )
  })

  test('a setext underline still wins over a one-column delimiter row', () => {
    // `---` is both. The block starts are ordered so the heading wins, exactly
    // as cmark-gfm orders them; `| foo |` as heading text is the giveaway.
    assert.equal(html('foo\n---\n'), '<h2>foo</h2>\n')
    assert.equal(html('| foo |\n---\n'), '<h2>| foo |</h2>\n')
    // A list marker wins too: `- | -` is a bullet, not a two-column delimiter.
    assert.equal(
      html('| a | b |\n- | -\n'),
      '<p>| a | b |</p>\n<ul>\n<li>| -</li>\n</ul>\n',
    )
  })

  test('a delimiter row with no paragraph above it is just a paragraph', () => {
    assert.equal(html('| - |\n'), '<p>| - |</p>\n')
    assert.equal(html('\n| - |\n'), '<p>| - |</p>\n')
    // …and it must be the line *directly* above.
    assert.equal(html('| a |\n\n| - |\n'), '<p>| a |</p>\n<p>| - |</p>\n')
  })

  test('a table nests in a block quote and in a list item', () => {
    assert.equal(
      html('> | a | b |\n> | - | - |\n> | c | d |\n'),
      '<blockquote>\n' +
        table(thead([th('a'), th('b')]), tbody([[td('c'), td('d')]])) +
        '</blockquote>\n',
    )
    assert.equal(
      html('- | a | b |\n  | - | - |\n  | c | d |\n'),
      '<ul>\n<li>\n' +
        table(thead([th('a'), th('b')]), tbody([[td('c'), td('d')]])) +
        '</li>\n</ul>\n',
    )
    // A lazy continuation is not a delimiter row: the quote's marker is
    // missing, so the paragraph simply carries on.
    assert.equal(
      html('> | a | b |\n| - | - |\n'),
      '<blockquote>\n<p>| a | b |\n| - | - |</p>\n</blockquote>\n',
    )
  })

  test('a table immediately followed by another table', () => {
    // Separated by a blank line: two tables.
    assert.equal(
      html('| a |\n| - |\n| 1 |\n\n| b |\n| - |\n| 2 |\n'),
      table(thead([th('a')]), tbody([[td('1')]])) +
        table(thead([th('b')]), tbody([[td('2')]])),
    )
    // Without one, the second "table" is body rows of the first — a delimiter
    // row is only special directly under a *paragraph*.
    assert.equal(
      html('| a |\n| - |\n| b |\n| - |\n'),
      table(thead([th('a')]), tbody([[td('b')], [td('-')]])),
    )
  })

  test('inlines inside cells are parsed, and only inlines', () => {
    assert.equal(
      html('| a |\n| - |\n| *e* ~~s~~ `c` [l](/u) www.x.com |\n'),
      table(
        thead([th('a')]),
        tbody([
          [
            td(
              '<em>e</em> <del>s</del> <code>c</code> ' +
                '<a href="/u">l</a> <a href="http://www.x.com">www.x.com</a>',
            ),
          ],
        ]),
      ),
    )
    // The tagfilter is a render-time step and still applies inside a cell.
    assert.equal(
      html('| a |\n| - |\n| <title>x |\n'),
      table(thead([th('a')]), tbody([[td('&lt;title>x')]])),
    )
  })

  // --- escaped pipes, in detail ---

  test('splitting is on unescaped pipes only', () => {
    // One cell, not two: `\|` never separates.
    assert.equal(html('| a\\|b |\n| - |\n'), table(thead([th('a|b')])))
    // `\\` is an escaped backslash, so the pipe after it *does* separate.
    assert.equal(
      html('| x | y |\n| - | - |\n| a\\\\ | b |\n'),
      table(thead([th('x'), th('y')]), tbody([[td('a\\'), td('b')]])),
    )
    // A trailing pipe that is itself escaped is content, not the optional
    // trailing delimiter.
    assert.equal(html('| a\\| |\n| - |\n'), table(thead([th('a|')])))
  })

  test('an escaped pipe becomes a literal pipe before inline parsing', () => {
    // This is the whole point of resolving `\|` at split time: a code span is
    // a literal, so nothing downstream could turn `\|` into `|` inside one.
    assert.equal(
      html('| a |\n| - |\n| `\\|` |\n'),
      table(thead([th('a')]), tbody([[td('<code>|</code>')]])),
    )
    assert.equal(
      html('| a |\n| - |\n| **\\|** |\n'),
      table(thead([th('a')]), tbody([[td('<strong>|</strong>')]])),
    )
    // …and the cell text is not unescaped twice: every other escape is left
    // for the inline phase, which handles it exactly once.
    assert.equal(
      html('| a |\n| - |\n| \\*not em\\* |\n'),
      table(thead([th('a')]), tbody([[td('*not em*')]])),
    )
    assert.equal(
      html('| a |\n| - |\n| \\\\ |\n'),
      table(thead([th('a')]), tbody([[td('\\')]])),
    )
  })

  // --- the AST projection ---

  test('AST: table, tableRow, tableCell, and align', () => {
    const doc: any = parseDocument('| h1 | h2 |\n| :- | -: |\n| b1 | b2 |\n', GFM)
    assert.equal(doc.children.length, 1)

    const t = doc.children[0]
    assert.equal(t.type, 'table')
    assert.deepEqual(t.align, ['left', 'right'])
    assert.equal(t.children.length, 2)

    // mdast has no header flag: the FIRST row is the header, by convention.
    assert.deepEqual(t.children[0], {
      type: 'tableRow',
      children: [
        { type: 'tableCell', children: [{ type: 'text', value: 'h1' }] },
        { type: 'tableCell', children: [{ type: 'text', value: 'h2' }] },
      ],
    })
    assert.deepEqual(t.children[1], {
      type: 'tableRow',
      children: [
        { type: 'tableCell', children: [{ type: 'text', value: 'b1' }] },
        { type: 'tableCell', children: [{ type: 'text', value: 'b2' }] },
      ],
    })
  })

  test('AST: align carries null for a column with no colon', () => {
    const doc: any = parseDocument('| a | b | c |\n| --- | :-: | --: |\n', GFM)
    assert.deepEqual(doc.children[0].align, [null, 'center', 'right'])
    // One entry per column, always.
    assert.equal(doc.children[0].align.length, 3)
    assert.equal(doc.children[0].children[0].children.length, 3)
  })

  test('AST: padded cells are real, empty cells', () => {
    const doc: any = parseDocument('| a | b |\n| - | - |\n| x |\n', GFM)
    const body = doc.children[0].children[1]
    assert.equal(body.children.length, 2)
    assert.deepEqual(body.children[1], { type: 'tableCell', children: [] })
  })

  test('AST: cells hold inline nodes', () => {
    const doc: any = parseDocument('| a |\n| - |\n| *x* |\n', GFM)
    assert.deepEqual(doc.children[0].children[1].children[0], {
      type: 'tableCell',
      children: [{ type: 'emphasis', children: [{ type: 'text', value: 'x' }] }],
    })
  })

  test('AST: a table nested in a list item projects in place', () => {
    const doc: any = parseDocument('- | a |\n  | - |\n', GFM)
    const item = doc.children[0].children[0]
    assert.equal(item.children.length, 1)
    assert.equal(item.children[0].type, 'table')
  })

  // --- sourcepos ---

  // A table is opened by its delimiter row but begins on the header row above
  // it, and the two rows are indented independently. Taking the start column
  // from the delimiter row therefore reports a column belonging to a different
  // line — visible through `parseTree`, which is public.
  const tableStart = (src: string) => {
    const w = parse(src, GFM).walker()
    let e
    while ((e = w.next())) {
      if (e.entering && 'table' === e.node.type) return e.node.sourcepos[0]
    }
    return null
  }

  test('a table starts at the header row, in the header row’s own column', () => {
    assert.deepEqual(tableStart('a | b\n| - | -\n'), [1, 1], 'control: neither row indented')
    assert.deepEqual(tableStart('  a | b\n| - | -\n'), [1, 3], 'the header row is indented')
    assert.deepEqual(tableStart('a | b\n  | - | -\n'), [1, 1], 'only the delimiter row is')
    assert.deepEqual(tableStart('  a | b\n  | - | -\n'), [1, 3], 'both rows are')
  })

  test('the header row supplies the column even when the paragraph splits', () => {
    // Here the header row is the *last* line of a longer paragraph, so the
    // column cannot come from the paragraph node either — that one belongs to
    // the first line, which stays a paragraph.
    assert.deepEqual(tableStart('x\n  a | b\n| - | -\n'), [2, 3])
    assert.deepEqual(tableStart('  x\na | b\n| - | -\n'), [2, 1])
    // A table inside a block quote counts columns from the start of the line,
    // marker included, exactly as every other block does.
    assert.deepEqual(tableStart('> a | b\n> | - | -\n'), [1, 3])
    assert.deepEqual(tableStart('>   a | b\n> | - | -\n'), [1, 5])
  })

  test('the paragraph a table splits ends on its own last line', () => {
    // The split closes a block *two* lines back — the header row in between
    // belongs to the table — so the end column cannot come from the previous
    // line the way every other finalize takes it. Taking it from there reports
    // the header row's length on a line that is not the header row.
    const paraEnd = (src: string) => {
      const w = parse(src, GFM).walker()
      let e
      while ((e = w.next())) {
        if (e.entering && 'paragraph' === e.node.type) return e.node.sourcepos[1]
      }
      return null
    }

    // `aaaaaaaaaa` is 10 characters and the header row below it is 9, so the
    // wrong column here is a *shorter* one, which no clamp could explain.
    assert.deepEqual(paraEnd('aaaaaaaaaa\n| a | b |\n| - | - |\n'), [1, 10])
    // …and the other way round: `aa` is 2 characters under a 20-character
    // header row, so there the wrong column is a longer one.
    assert.deepEqual(paraEnd('aa\n| aaaaaaaaaaaa | b |\n| - | - |\n'), [1, 2])
    // Every other interrupter already ends this paragraph at column 10; the
    // table must not be the odd one out.
    for (const after of ['# h\n', '```\nx\n```\n', '> q\n', '- i\n', '***\n', '<div>\n', '\nx\n']) {
      assert.deepEqual(paraEnd('aaaaaaaaaa\n' + after), [1, 10], JSON.stringify(after))
    }
  })

  test('every row of one table is measured on the same basis', () => {
    // Rows span their own text, so two rows of the same table on equally
    // indented lines must report the same end column. The header row is built
    // in `tryOpenTable` and the body rows in `finalizeTable`; measuring the
    // header against the whole source line would put it two columns past the
    // body rows inside a block quote.
    const rowSpans = (src: string) => {
      const out: unknown[] = []
      const w = parse(src, GFM).walker()
      let e
      while ((e = w.next())) {
        if (e.entering && 'table_row' === e.node.type) out.push(e.node.sourcepos)
      }
      return out
    }

    assert.deepEqual(rowSpans('| a | b |\n| - | - |\n| c | d |\n'), [
      [[1, 1], [1, 9]],
      [[3, 1], [3, 9]],
    ])
    assert.deepEqual(rowSpans('> | a | b |\n> | - | - |\n> | c | d |\n'), [
      [[1, 1], [1, 9]],
      [[3, 1], [3, 9]],
    ])
    assert.deepEqual(rowSpans('  | a | b |\n  | - | - |\n  | c | d |\n'), [
      [[1, 1], [1, 9]],
      [[3, 1], [3, 9]],
    ])
  })

  // --- gfm:false ---

  test('gfm:false produces no table nodes and byte-identical paragraph HTML', () => {
    const sources = [
      '| foo | bar |\n| --- | --- |\n| baz | bim |\n',
      '| abc | defghi |\n:-: | -----------:\nbar | baz\n',
      '| f\\|oo  |\n| ------ |\n| b `\\|` az |\n',
      '| abc | def |\n| --- | --- |\n',
      'aaa\n| a | b |\n| - | - |\n',
    ]
    for (const src of sources) {
      const out = toHtml(src, OPTS)
      assert.ok(!out.includes('<table'), JSON.stringify(src))
      // Exactly what a paragraph of these lines renders as — the escaping and
      // the soft breaks included.
      assert.equal(out, renderHTML(parse(src, OPTS)), JSON.stringify(src))
      assert.ok(out.startsWith('<p>'), JSON.stringify(src))

      const doc: any = parseDocument(src, OPTS)
      for (const block of doc.children) {
        assert.notEqual(block.type, 'table', JSON.stringify(src))
      }
    }
    // The delimiter row is not even a paragraph break: it all stays one.
    assert.equal(
      toHtml('| a | b |\n| - | - |\n| c | d |\n', OPTS),
      '<p>| a | b |\n| - | - |\n| c | d |</p>\n',
    )
  })

  test('renderHTML with no options follows the parse for tables too', () => {
    assert.equal(renderHTML(parse('| a |\n| - |\n', OPTS)), '<p>| a |\n| - |</p>\n')
    assert.ok(renderHTML(parse('| a |\n| - |\n', GFM)).startsWith('<table>'))
  })
})

describe('gfm table robustness', () => {
  // Nothing here may throw, and nothing may become super-linear: the inputs
  // are adversarial, and untrusted input picks them.

  test('a 10000-column delimiter row', () => {
    const cols = 10000
    const src =
      '|' + ' h |'.repeat(cols) + '\n' +
      '|' + ' --- |'.repeat(cols) + '\n' +
      '|' + ' b |'.repeat(cols) + '\n'
    let out = ''
    assert.doesNotThrow(() => {
      out = toHtml(src, GFM)
    })
    assert.equal(out.split('<th>').length - 1, cols)
    assert.equal(out.split('<td>').length - 1, cols)

    const doc: any = parseDocument(src, GFM)
    assert.equal(doc.children[0].align.length, cols)
    assert.equal(doc.children[0].children[0].children.length, cols)
  })

  test('10000 body rows', () => {
    const rows = 10000
    const src = '| a | b |\n| - | - |\n' + '| x | y |\n'.repeat(rows)
    let out = ''
    assert.doesNotThrow(() => {
      out = toHtml(src, GFM)
    })
    // One header row plus every body row.
    assert.equal(out.split('<tr>').length - 1, rows + 1)
    const doc: any = parseDocument(src, GFM)
    assert.equal(doc.children[0].children.length, rows + 1)
  })

  test('a pipe repeated 50000 times', () => {
    const n = 50000
    // On its own it is one paragraph and nothing more.
    assert.doesNotThrow(() => toHtml('|'.repeat(n) + '\n', GFM))
    assert.doesNotThrow(() => toHtml('|'.repeat(n) + '\n', OPTS))
    // As a header row over a matching delimiter row it is a very wide table:
    // n pipes with the leading and trailing one stripped is n-1 empty cells.
    let out = ''
    assert.doesNotThrow(() => {
      out = toHtml('|'.repeat(n) + '\n|' + ' - |'.repeat(n - 1) + '\n', GFM)
    })
    assert.equal(out.split('<th></th>').length - 1, n - 1)
  })

  test('padding a very wide table over very many rows stays bounded', () => {
    // The one shape whose node count is not bounded by the input: 10000
    // columns over 10000 one-cell rows asks for 10^8 cells. The autocomplete
    // budget caps it, so quadrupling the rows must not grow the work.
    const header = '|' + ' h |'.repeat(10000) + '\n' + '|' + ' --- |'.repeat(10000) + '\n'
    const measure = (rows: number) => {
      const src = header + '| x |\n'.repeat(rows)
      const t0 = performance.now()
      const out = toHtml(src, GFM)
      return { ms: performance.now() - t0, len: out.length }
    }

    const base = measure(5000)
    const large = measure(20000)

    // The output is capped, not proportional to the row count.
    assert.ok(3 > large.len / base.len, `output grew ${(large.len / base.len).toFixed(1)}x`)
    if (5 > base.ms) return
    assert.ok(
      6 > large.ms / base.ms,
      `4x rows took ${(large.ms / base.ms).toFixed(1)}x time ` +
        `(${base.ms.toFixed(1)}ms -> ${large.ms.toFixed(1)}ms)`,
    )
  })

  test('a delimiter row that never matches stays linear', () => {
    // Every second line is a syntactically valid delimiter row that fails the
    // cell-count test, so the paragraph grows without bound and each line asks
    // for its last line again. Reading that back out of the accumulated
    // content re-flattens a rope every time — O(n^2), and this is the input
    // that shows it.
    const measure = (n: number) => {
      const src = '| a | b |\n| --- |\n'.repeat(n)
      let bestMs = Infinity
      for (let i = 0; i < 3; i++) {
        const t0 = performance.now()
        toHtml(src, GFM)
        bestMs = Math.min(bestMs, performance.now() - t0)
      }
      return bestMs
    }

    measure(2000) // warm up the JIT
    const base = measure(20000)
    const large = measure(80000)

    // Below this the measurement is noise, not signal (see the sourcepos test).
    if (5 > base) return

    const ratio = large / base
    assert.ok(
      12 > ratio,
      `4x input took ${ratio.toFixed(1)}x time (${base.toFixed(1)}ms -> ${large.toFixed(1)}ms); ` +
        'linear is ~4x, quadratic ~16x',
    )
  })

  test('degenerate table fragments parse and render', () => {
    for (const src of [
      '|', '||', '|||', '\\|', '|\\', '| - ', ' - |', '|-|', ':', '::', ':-:', '-:',
      'a\n|', 'a\n:', 'a\n-:', 'a\n|-', 'a\n||', 'a\n:-:|', 'a|b\n-|-|-', 'a\n\\|',
      '|a|\n|\\|', 'a\n|\t-\t|', '|\n|', '| |\n| |', 'a\n' + '-'.repeat(10000),
      '|'.repeat(200) + '\n' + '-|'.repeat(200), '> a\n> |-|', '- a\n  |-|',
      'a\n---\n', 'a\n- | -', '|a\n|-\n    x', '\\\\|\n-',
    ]) {
      assert.doesNotThrow(() => toHtml(src + '\n', GFM), JSON.stringify(src))
      assert.doesNotThrow(() => toHtml(src + '\n', OPTS), JSON.stringify(src))
      assert.doesNotThrow(() => parseDocument(src + '\n', GFM), JSON.stringify(src))
    }
  })
})

describe('gfm task list items', () => {
  const html = (src: string) => toHtml(src, GFM)

  test('a marker becomes a checkbox and is consumed', () => {
    assert.equal(
      html('- [ ] foo\n'),
      '<ul>\n<li><input disabled="" type="checkbox"> foo</li>\n</ul>\n',
    )
    assert.equal(
      html('- [x] bar\n'),
      '<ul>\n<li><input checked="" disabled="" type="checkbox"> bar</li>\n</ul>\n',
    )
    // "either a whitespace character or the letter x in either lowercase or
    // uppercase" — and a tab between the brackets is whitespace, so unchecked.
    assert.equal(html('- [X] up\n'), '<ul>\n<li><input checked="" disabled="" type="checkbox"> up</li>\n</ul>\n')
    assert.equal(html('- [\t] tab\n'), '<ul>\n<li><input disabled="" type="checkbox"> tab</li>\n</ul>\n')
  })

  test('works for every list flavour, and nests', () => {
    assert.equal(html('* [ ] a\n'), '<ul>\n<li><input disabled="" type="checkbox"> a</li>\n</ul>\n')
    assert.equal(html('1. [x] a\n'), '<ol>\n<li><input checked="" disabled="" type="checkbox"> a</li>\n</ol>\n')
    assert.equal(
      html('> - [x] quoted\n'),
      '<blockquote>\n<ul>\n<li><input checked="" disabled="" type="checkbox"> quoted</li>\n</ul>\n</blockquote>\n',
    )
    assert.equal(
      html('- - [x] deep\n'),
      '<ul>\n<li>\n<ul>\n<li><input checked="" disabled="" type="checkbox"> deep</li>\n</ul>\n</li>\n</ul>\n',
    )
  })

  test('a loose item puts the checkbox inside the paragraph', () => {
    assert.equal(
      html('- [x] a\n\n- [ ] b\n'),
      '<ul>\n<li>\n<p><input checked="" disabled="" type="checkbox"> a</p>\n</li>\n' +
        '<li>\n<p><input disabled="" type="checkbox"> b</p>\n</li>\n</ul>\n',
    )
  })

  test('what is not a marker', () => {
    // The extension requires "at least one whitespace character before any
    // other content", and only a space, an `x` or an `X` between the brackets.
    assert.equal(html('- [x]no space\n'), '<ul>\n<li>[x]no space</li>\n</ul>\n')
    assert.equal(html('- [x]\n'), '<ul>\n<li>[x]</li>\n</ul>\n')
    assert.equal(html('- [y] no\n'), '<ul>\n<li>[y] no</li>\n</ul>\n')
    assert.equal(html('- [xx] no\n'), '<ul>\n<li>[xx] no</li>\n</ul>\n')
    // Only the *first* block of the item, and only at its start.
    assert.equal(
      html('- a\n\n  [x] b\n'),
      '<ul>\n<li>\n<p>a</p>\n<p>[x] b</p>\n</li>\n</ul>\n',
    )
    // Not a list item at all.
    assert.equal(html('[x] loose text\n'), '<p>[x] loose text</p>\n')
  })

  test('the marker beats a link reference definition of the same label', () => {
    // Decided over the paragraph's raw text, before the inline phase runs, so
    // a `[x]` definition cannot turn the marker into a link.
    assert.equal(
      html('[x]: /url\n\n- [x] still a task\n'),
      '<ul>\n<li><input checked="" disabled="" type="checkbox"> still a task</li>\n</ul>\n',
    )
  })

  test('AST: checked is true, false or null', () => {
    const doc: any = parseDocument('- [x] a\n- [ ] b\n- c\n', GFM)
    const items = doc.children[0].children
    assert.equal(items.length, 3)
    assert.equal(items[0].checked, true)
    assert.equal(items[1].checked, false)
    assert.equal(items[2].checked, null)
    // mdast keeps the field on every item, and the marker is gone from the text.
    for (const item of items) assert.ok('checked' in item)
    assert.deepEqual(items[0].children, [
      { type: 'paragraph', children: [{ type: 'text', value: 'a' }] },
    ])

    // Nested items carry their own state.
    const nested: any = parseDocument('- [x] a\n  - [ ] b\n', GFM)
    const outer = nested.children[0].children[0]
    assert.equal(outer.checked, true)
    assert.equal(outer.children[1].children[0].checked, false)
  })
})

describe('gfm disallowed raw html', () => {
  const html = (src: string) => toHtml(src, GFM)

  test('the nine tags lose their leading angle bracket', () => {
    for (const tag of [
      'title', 'textarea', 'style', 'xmp', 'iframe',
      'noembed', 'noframes', 'script', 'plaintext',
    ]) {
      assert.equal(html('a <' + tag + '> b\n'), '<p>a &lt;' + tag + '> b</p>\n', tag)
      assert.equal(html('a </' + tag + '> b\n'), '<p>a &lt;/' + tag + '> b</p>\n', '/' + tag)
    }
  })

  test('case-insensitive, opening and closing, block and inline', () => {
    assert.equal(html('a <XMP> b\n'), '<p>a &lt;XMP> b</p>\n')
    assert.equal(html('a </ScRiPt> b\n'), '<p>a &lt;/ScRiPt> b</p>\n')
    assert.equal(html('<script>\nx\n</script>\n'), '&lt;script>\nx\n&lt;/script>\n')
    assert.equal(
      html('<blockquote>\n  <xmp> no\n</blockquote>\n'),
      '<blockquote>\n  &lt;xmp> no\n</blockquote>\n',
    )
  })

  test('everything else passes through verbatim', () => {
    assert.equal(html('a <em> b\n'), '<p>a <em> b</p>\n')
    assert.equal(html('a <scriptlet> b\n'), '<p>a <scriptlet> b</p>\n')
    assert.equal(html('a <titles> b\n'), '<p>a <titles> b</p>\n')
    assert.equal(html('a <script-ish> b\n'), '<p>a <script-ish> b</p>\n')
    // The filter is a rendering step; a code span is text, not raw HTML.
    assert.equal(html('`<script>`\n'), '<p><code>&lt;script&gt;</code></p>\n')
  })

  test('the tree and the AST keep the original text', () => {
    const doc: any = parseDocument('<script>x</script>\n', GFM)
    assert.deepEqual(doc.children[0], { type: 'html', value: '<script>x</script>' })
  })
})

describe('gfm autolink literals', () => {
  const html = (src: string) => toHtml(src, GFM)
  const a = (href: string, text: string) => '<a href="' + href + '">' + text + '</a>'

  test('www., the three schemes, and email', () => {
    assert.equal(html('www.commonmark.org\n'), '<p>' + a('http://www.commonmark.org', 'www.commonmark.org') + '</p>\n')
    assert.equal(html('http://commonmark.org\n'), '<p>' + a('http://commonmark.org', 'http://commonmark.org') + '</p>\n')
    assert.equal(html('https://commonmark.org\n'), '<p>' + a('https://commonmark.org', 'https://commonmark.org') + '</p>\n')
    assert.equal(html('ftp://foo.bar.baz\n'), '<p>' + a('ftp://foo.bar.baz', 'ftp://foo.bar.baz') + '</p>\n')
    assert.equal(html('foo@bar.baz\n'), '<p>' + a('mailto:foo@bar.baz', 'foo@bar.baz') + '</p>\n')
  })

  test('only at a line start, after whitespace, or after * _ ~ (', () => {
    for (const before of ['', ' ', '*', '_', '~', '(']) {
      const src = before + 'www.a.com\n'
      assert.ok(html(src).includes('href="http://www.a.com"'), JSON.stringify(src))
    }
    // Anything else, and the text is left alone.
    for (const before of ['x', '-', '/', '=', '"', '&', ':']) {
      const src = before + 'www.a.com\n'
      assert.ok(!html(src).includes('<a href'), JSON.stringify(src))
    }
  })

  // The rule is about the SOURCE character immediately before the match, and
  // the post-pass runs over a tree that no longer carries source offsets. At
  // offset 0 of a text node that character lives in the previous sibling, and
  // the sibling's *type* names it exactly — so each row below states the type,
  // the character it ends on, and whether an autolink may start after it.
  //
  // Rejecting offset 0 whenever there is any previous sibling is the obvious
  // wrong fix: it breaks the four `true` rows in the middle of this table.
  const boundaryRows: Array<[string, string, string, boolean]> = [
    // previous sibling, prefix that produces it, its last source character, links?
    ['(none)', '', 'start of line', true],
    ['softbreak', 'a\n', 'start of line', true],
    ['linebreak', 'a\\\n', 'start of line', true],
    ['emph', '*a*', '*', true],
    ['strong', '**a**', '*', true],
    ['del', '~~a~~', '~', true],
    ['code', '`a`', '`', false],
    ['link', '[a](/u)', ')', false],
    ['image', '![a](/u)', ')', false],
    ['html_inline', '<b>', '>', false],
  ]

  for (const [prevType, prefix, lastChar, links] of boundaryRows) {
    test(`offset 0 after ${prevType} (source ends ${lastChar}): ${links ? 'links' : 'no link'}`, () => {
      // The row's first column, asserted rather than assumed: with an inert
      // word in place of the URL, the paragraph ends in a text node whose
      // previous sibling is exactly the node this row names.
      const para: any = parse(prefix + 'X\n', GFM).firstChild
      const tail = para.lastChild
      assert.equal(tail.type, 'text')
      assert.equal(tail.literal, 'X')
      assert.equal(null === tail.prev ? '(none)' : tail.prev.type, prevType)

      const out = html(prefix + 'www.a.com\n')
      assert.equal(out.includes('href="http://www.a.com"'), links, out)
    })
  }

  test('emphasis is still a delimiter an autolink may follow', () => {
    // Called out on its own because it is what the obvious wrong fix breaks.
    assert.equal(
      html('*a*www.b.com\n'),
      '<p><em>a</em>' + a('http://www.b.com', 'www.b.com') + '</p>\n',
    )
    // And its counterpart, which must not link.
    assert.equal(html('`x`www.a.com\n'), '<p><code>x</code>www.a.com</p>\n')
  })

  test('the boundary applies to email addresses too', () => {
    assert.equal(html('*a*b@c.de\n'), '<p><em>a</em>' + a('mailto:b@c.de', 'b@c.de') + '</p>\n')
    assert.equal(html('`x`b@c.de\n'), '<p><code>x</code>b@c.de</p>\n')
    assert.equal(html('[l](/u)b@c.de\n'), '<p>' + a('/u', 'l') + 'b@c.de</p>\n')
  })

  test('a valid domain needs a period and no underscore in its last two segments', () => {
    assert.ok(html('x www.a.b.com y\n').includes('<a href'))
    assert.ok(!html('x www.foo_bar.com y\n').includes('<a href'), 'second-to-last segment')
    assert.ok(!html('x www.a.foo_bar y\n').includes('<a href'), 'last segment')
    assert.ok(html('x www.foo_bar.a.com y\n').includes('<a href'), 'earlier segments may hold _')
    assert.ok(!html('x http://foo_bar.com y\n').includes('<a href'))
    assert.ok(!html('x http://localhost/y z\n').includes('<a href'), 'no period, no domain')
  })

  test('trailing punctuation is not part of the link', () => {
    assert.equal(
      html('Visit www.commonmark.org.\n'),
      '<p>Visit ' + a('http://www.commonmark.org', 'www.commonmark.org') + '.</p>\n',
    )
    assert.equal(
      html('Visit www.commonmark.org/a.b.\n'),
      '<p>Visit ' + a('http://www.commonmark.org/a.b', 'www.commonmark.org/a.b') + '.</p>\n',
    )
    for (const p of ['?', '!', '.', ',', ':', '*', '~']) {
      assert.ok(html('x www.a.com' + p + '\n').includes('>www.a.com</a>'), 'trailing ' + p)
    }
    // `_` is the one that is also a domain character, so it reaches the domain
    // check before the trim can drop it and invalidates the last segment —
    // there is no link here at all, which is cmark-gfm's behaviour too.
    assert.equal(html('x www.a.com_\n'), '<p>x www.a.com_</p>\n')
    // Past the domain it is ordinary trailing punctuation again.
    assert.equal(
      html('x www.a.com/y_\n'),
      '<p>x ' + a('http://www.a.com/y', 'www.a.com/y') + '_</p>\n',
    )
  })

  test('parentheses balance across the whole match, and only when it ends in )', () => {
    const link = a('http://www.google.com/search?q=Markup+(business)', 'www.google.com/search?q=Markup+(business)')
    assert.equal(html('www.google.com/search?q=Markup+(business)\n'), '<p>' + link + '</p>\n')
    assert.equal(html('www.google.com/search?q=Markup+(business)))\n'), '<p>' + link + '))</p>\n')
    assert.equal(html('(www.google.com/search?q=Markup+(business))\n'), '<p>(' + link + ')</p>\n')
    assert.equal(html('(www.google.com/search?q=Markup+(business)\n'), '<p>(' + link + '</p>\n')
    // Interior parentheses alone trigger nothing.
    assert.equal(
      html('www.google.com/search?q=(business))+ok\n'),
      '<p>' + a('http://www.google.com/search?q=(business))+ok', 'www.google.com/search?q=(business))+ok') + '</p>\n',
    )
  })

  test('a trailing entity-like ;, and < ends a link', () => {
    assert.equal(
      html('www.google.com/search?q=commonmark&hl=en\n'),
      '<p>' + a('http://www.google.com/search?q=commonmark&amp;hl=en', 'www.google.com/search?q=commonmark&amp;hl=en') + '</p>\n',
    )
    assert.equal(
      html('www.google.com/search?q=commonmark&hl;\n'),
      '<p>' + a('http://www.google.com/search?q=commonmark', 'www.google.com/search?q=commonmark') + '&amp;hl;</p>\n',
    )
    assert.equal(
      html('www.commonmark.org/he<lp\n'),
      '<p>' + a('http://www.commonmark.org/he', 'www.commonmark.org/he') + '&lt;lp</p>\n',
    )
  })

  test('email: + before the @ only, and a trailing - or _ invalidates it', () => {
    // `+` may occur before the `@`, but not after: the first address here has
    // no valid domain, the second does.
    assert.equal(
      html("hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.\n"),
      "<p>hello@mail+xyz.example isn't valid, but " +
        a('mailto:hello+xyz@mail.example', 'hello+xyz@mail.example') +
        ' is.</p>\n',
    )
    assert.equal(html('a.b-c_d@a.b\n'), '<p>' + a('mailto:a.b-c_d@a.b', 'a.b-c_d@a.b') + '</p>\n')
    assert.equal(html('a.b-c_d@a.b.\n'), '<p>' + a('mailto:a.b-c_d@a.b', 'a.b-c_d@a.b') + '.</p>\n')
    assert.equal(html('a.b-c_d@a.b-\n'), '<p>a.b-c_d@a.b-</p>\n')
    assert.equal(html('a.b-c_d@a.b_\n'), '<p>a.b-c_d@a.b_</p>\n')
    assert.equal(html('foo@bar\n'), '<p>foo@bar</p>\n', 'the domain needs a period')
  })

  test('a local part of 65 characters or more is not recognised at all', () => {
    // The rewind that finds the start of a local part stops after 64
    // characters, which is what keeps this pass linear and is RFC 5321's own
    // limit. Stopping there is a cap, not a boundary: when the character just
    // outside it still belongs to the local part, the address is over-long and
    // there is no address — linking the 64-character tail would invent one.
    for (const [local, linked] of [
      ['a'.repeat(63), true],
      ['a'.repeat(64), true],
      ['a'.repeat(65), false],
      ['a'.repeat(100), false],
      // A leading `_` is itself a local-part character, so these are the same
      // four lengths shifted by one — 64 links, 65 does not.
      ['_' + 'a'.repeat(63), true],
      ['_' + 'a'.repeat(64), false],
      ['_' + 'a'.repeat(65), false],
      ['_' + 'a'.repeat(100), false],
    ] as Array<[string, boolean]>) {
      const addr = local + '@b.co'
      const out = html(addr + '\n')
      const label = local.length + '-character local part'
      if (linked) {
        assert.equal(out, '<p>' + a('mailto:' + addr, addr) + '</p>\n', label)
      } else {
        assert.equal(out, '<p>' + addr + '</p>\n', label)
      }
    }
  })

  test('a match spanning inline nodes is still one link', () => {
    // `_` ends a text run in the inline scanner, so `a.b-c_d@a.b` reaches the
    // post-pass as three text siblings. Consolidating them is what makes the
    // whole address one link rather than `d@a.b`.
    const doc: any = parseDocument('a.b-c_d@a.b\n', GFM)
    assert.deepEqual(doc.children[0].children, [
      {
        type: 'link',
        url: 'mailto:a.b-c_d@a.b',
        title: null,
        children: [{ type: 'text', value: 'a.b-c_d@a.b' }],
      },
    ])
  })

  test('never inside a link, a code span, or raw HTML', () => {
    // No links in links (§6.3).
    assert.equal(html('[label www.a.com](/dest)\n'), '<p>' + a('/dest', 'label www.a.com') + '</p>\n')
    assert.equal(
      html('[label www.a.com][ref]\n\n[ref]: /dest\n'),
      '<p>' + a('/dest', 'label www.a.com') + '</p>\n',
    )
    assert.equal(html('<https://a.com/x>\n'), '<p>' + a('https://a.com/x', 'https://a.com/x') + '</p>\n')
    // Code spans and raw HTML are literals the post-pass never looks inside.
    assert.equal(html('`www.a.com`\n'), '<p><code>www.a.com</code></p>\n')
    assert.equal(html('<b title="www.a.com">x</b>\n'), '<p><b title="www.a.com">x</b></p>\n')
    // An image description is alt text, so it is left alone too.
    assert.equal(html('![alt www.a.com](/i)\n'), '<p><img src="/i" alt="alt www.a.com" /></p>\n')
  })

  test('inside emphasis, strikethrough, headings and list items', () => {
    assert.equal(html('*www.a.com*\n'), '<p><em>' + a('http://www.a.com', 'www.a.com') + '</em></p>\n')
    assert.equal(html('~~www.a.com~~\n'), '<p><del>' + a('http://www.a.com', 'www.a.com') + '</del></p>\n')
    assert.equal(html('# www.a.com\n'), '<h1>' + a('http://www.a.com', 'www.a.com') + '</h1>\n')
    assert.equal(html('- www.a.com\n'), '<ul>\n<li>' + a('http://www.a.com', 'www.a.com') + '</li>\n</ul>\n')
  })

  test('the AST carries the decoded destination', () => {
    const doc: any = parseDocument('see www.a.com/ä now\n', GFM)
    assert.deepEqual(doc.children[0].children[1], {
      type: 'link',
      url: 'http://www.a.com/ä',
      title: null,
      children: [{ type: 'text', value: 'www.a.com/ä' }],
    })
    assert.ok(toHtml('see www.a.com/ä now\n', GFM).includes('href="http://www.a.com/%C3%A4"'))
  })
})

describe('gfm:false disables every extension', () => {
  // This is the regression that matters most: the whole 652-example CommonMark
  // suite runs with `gfm:false`, and the conformance runner renders with
  // `renderHTML(tree)` — no options at all — so the renderer must take the
  // flavour from the tree rather than from its own defaults.
  const sources: Array<[string, string]> = [
    ['- [ ] foo\n', '<ul>\n<li>[ ] foo</li>\n</ul>\n'],
    ['- [x] foo\n', '<ul>\n<li>[x] foo</li>\n</ul>\n'],
    ['<script>alert(1)</script>\n', '<script>alert(1)</script>\n'],
    ['a <title> b\n', '<p>a <title> b</p>\n'],
    ['www.commonmark.org\n', '<p>www.commonmark.org</p>\n'],
    ['http://commonmark.org\n', '<p>http://commonmark.org</p>\n'],
    ['foo@bar.baz\n', '<p>foo@bar.baz</p>\n'],
    ['~~x~~\n', '<p>~~x~~</p>\n'],
    ['| a | b |\n| - | - |\n| c | d |\n', '<p>| a | b |\n| - | - |\n| c | d |</p>\n'],
    ['a\n:-:\n', '<p>a\n:-:</p>\n'],
  ]

  test('toHtml with gfm:false', () => {
    for (const [src, expected] of sources) {
      assert.equal(toHtml(src, OPTS), expected, JSON.stringify(src))
    }
  })

  test('renderHTML with no options at all follows the parse', () => {
    for (const [src, expected] of sources) {
      assert.equal(renderHTML(parse(src, OPTS)), expected, JSON.stringify(src))
    }
    // …and an explicit option still wins over what the tree records.
    assert.equal(
      renderHTML(parse('<script>x</script>\n', OPTS), { gfm: true }),
      '&lt;script>x&lt;/script>\n',
    )
  })

  test('the AST carries no GFM shapes either', () => {
    const doc: any = parseDocument('- [x] www.a.com and a@b.co\n', OPTS)
    const item = doc.children[0].children[0]
    assert.equal(item.checked, null)
    assert.deepEqual(item.children, [
      { type: 'paragraph', children: [{ type: 'text', value: '[x] www.a.com and a@b.co' }] },
    ])
  })
})

describe('gfm autolink robustness', () => {
  // Nothing here may throw, and nothing may become super-linear: the inputs
  // are adversarial, and untrusted input picks them.

  test('a very long run of www. prefixes', () => {
    const src = 'www.'.repeat(50000) + '\n'
    let out = ''
    assert.doesNotThrow(() => {
      out = toHtml(src, GFM)
    })
    // One link over the whole run; the final period is trailing punctuation.
    assert.ok(out.startsWith('<p><a href="http://www.www.'))
    assert.ok(out.endsWith('www</a>.</p>\n'))
    assert.equal(out.split('<a href').length - 1, 1)
  })

  test('deeply parenthesised URLs', () => {
    const balanced = 'www.a.com/' + '('.repeat(20000) + ')'.repeat(20000) + '\n'
    const unbalanced = 'www.a.com/x' + ')'.repeat(20000) + '\n'
    let bal = ''
    let unbal = ''
    assert.doesNotThrow(() => {
      bal = toHtml(balanced, GFM)
      unbal = toHtml(unbalanced, GFM)
    })
    // Balanced parentheses are all part of the link.
    assert.ok(bal.includes('href="http://www.a.com/' + '('.repeat(20000) + ')'.repeat(20000) + '"'))
    // Unbalanced ones are all dropped, and stay in the text.
    assert.ok(unbal.startsWith('<p><a href="http://www.a.com/x">www.a.com/x</a>)'))
    assert.ok(unbal.endsWith(')'.repeat(20000) + '</p>\n'))
  })

  test('candidates that fail late stay linear', () => {
    // `_` both continues a domain and opens an autolink, so every underscore
    // here starts a candidate whose domain runs to the end of the paragraph —
    // the shape that goes quadratic without a bound on the domain scan.
    const measure = (n: number) => {
      const src = '_www.a_b.c'.repeat(n) + '\n'
      let best = Infinity
      for (let i = 0; i < 3; i++) {
        const t0 = performance.now()
        toHtml(src, GFM)
        best = Math.min(best, performance.now() - t0)
      }
      return best
    }

    measure(2000) // warm up the JIT
    const base = measure(20000)
    const large = measure(80000)

    // Below this the measurement is noise, not signal (see the sourcepos test).
    if (5 > base) return

    const ratio = large / base
    assert.ok(
      12 > ratio,
      `4x input took ${ratio.toFixed(1)}x time (${base.toFixed(1)}ms -> ${large.toFixed(1)}ms); ` +
        'linear is ~4x, quadratic ~16x',
    )
  })

  test('degenerate fragments parse and render', () => {
    for (const src of [
      'www.', 'www..', 'www...', 'w', '@', '@@@', 'www.@', 'a@', '@b.c',
      'http://', '://', 'www. x', 'a@' + '.'.repeat(500),
      'www.a.com/' + ')'.repeat(500), '&'.repeat(500) + ';',
      '- [', '- []', '- [ ', '<script', '</',
    ]) {
      assert.doesNotThrow(() => toHtml(src + '\n', GFM), JSON.stringify(src))
      assert.doesNotThrow(() => toHtml(src + '\n', OPTS), JSON.stringify(src))
    }
  })
})
