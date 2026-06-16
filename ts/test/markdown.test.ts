/* Copyright (c) 2021-2024 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '../dist/markdown'

const fixturesDir = join(__dirname, '..', '..', 'test', 'fixtures')
const manifest = JSON.parse(
  readFileSync(join(fixturesDir, 'manifest.json'), 'utf8'),
)

describe('markdown', () => {
  test('empty-records', async () => {
    // ignored by default

    const jo = new Tabnas().use(jsonic).use(Markdown)
    assert.deepEqual(jo.parse('\n'), [])
    assert.deepEqual(jo.parse('a\n1\n\n2\n3\n\n\n4\n'), [
      { a: '1' },
      { a: '2' },
      { a: '3' },
      { a: '4' },
    ])

    const ja = new Tabnas().use(jsonic).use(Markdown, { object: false })
    assert.deepEqual(ja.parse('\n'), [])
    assert.deepEqual(ja.parse('a\n1\n\n2\n3\n\n\n4\n'), [['1'], ['2'], ['3'], ['4']])

    // start and end also ignored

    assert.deepEqual(jo.parse('\r\na,b\r\nA,B\r\n'), [{ a: 'A', b: 'B' }])
    assert.deepEqual(jo.parse('\r\n\r\na,b\r\nA,B\r\n\r\n'), [{ a: 'A', b: 'B' }])
    assert.deepEqual(ja.parse('\r\na,b\r\nA,B\r\n'), [['A', 'B']])
    assert.deepEqual(ja.parse('\r\n\r\na,b\r\nA,B\r\n\r\n'), [['A', 'B']])

    // with option, empty creates record

    const jon = new Tabnas().use(jsonic).use(Markdown, { record: { empty: true } })
    assert.deepEqual(jon.parse('\n'), [])
    assert.deepEqual(jon.parse('a\n1\n\n2\n3\n\n\n4\n'), [
      { a: '1' },
      { a: '' },
      { a: '2' },
      { a: '3' },
      { a: '' },
      { a: '' },
      { a: '4' },
    ])

    // with comments

    const joc = new Tabnas().use(jsonic).use(Markdown, { comment: true })
    // console.log(joc.parse('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'))
    assert.deepEqual(joc.parse('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'), [
      { a: '1' },
      { a: '2' },
      { a: '3' },
      { a: '4' },
    ])

    const jocn = new Tabnas().use(jsonic).use(Markdown, {
      comment: true,
      record: { empty: true },
    })
    assert.deepEqual(jocn.parse('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'), [
      { a: '1' },
      { a: '' },
      { a: '2' },
      { a: '3' },
      { a: '' },
      { a: '' },
      { a: '4' },
    ])
  })

  test('header', async () => {
    const jo = new Tabnas().use(jsonic).use(Markdown)
    assert.deepEqual(jo.parse('\n'), [])
    assert.deepEqual(jo.parse('\na,b\nA,B'), [{ a: 'A', b: 'B' }])

    const ja = new Tabnas().use(jsonic).use(Markdown, { object: false })
    assert.deepEqual(ja.parse('\n'), [])
    assert.deepEqual(ja.parse('\na,b\nA,B'), [['A', 'B']])

    const jon = new Tabnas().use(jsonic).use(Markdown, { header: false })
    assert.deepEqual(jon.parse('\n'), [])
    assert.deepEqual(jon.parse('\na,b\nA,B'), [
      {
        'field~0': 'a',
        'field~1': 'b',
      },
      {
        'field~0': 'A',
        'field~1': 'B',
      },
    ])

    const jan = new Tabnas().use(jsonic).use(Markdown, { header: false, object: false })
    assert.deepEqual(jan.parse('\n'), [])
    assert.deepEqual(jan.parse('\na,b\nA,B'), [
      ['a', 'b'],
      ['A', 'B'],
    ])

    const jonf = new Tabnas().use(jsonic).use(Markdown, {
      header: false,
      field: { names: ['a', 'b'] },
    })
    assert.deepEqual(jonf.parse('\n'), [])
    assert.deepEqual(jonf.parse('\na,b\nA,B'), [
      {
        a: 'a',
        b: 'b',
      },
      {
        a: 'A',
        b: 'B',
      },
    ])
  })

  test('comma', async () => {
    const jo = new Tabnas().use(jsonic).use(Markdown)

    assert.deepEqual(jo.parse('\na'), [])
    assert.deepEqual(jo.parse('a\n1,'), [{ a: '1', 'field~1': '' }])
    assert.deepEqual(jo.parse('a\n,1'), [{ a: '', 'field~1': '1' }])
    assert.deepEqual(jo.parse('a,b\n1,2,'), [{ a: '1', b: '2', 'field~2': '' }])
    assert.deepEqual(jo.parse('a,b\n,1,2'), [{ a: '', b: '1', 'field~2': '2' }])

    assert.deepEqual(jo.parse('a\n1,\n'), [{ a: '1', 'field~1': '' }])
    assert.deepEqual(jo.parse('a\n,1\n'), [{ a: '', 'field~1': '1' }])
    assert.deepEqual(jo.parse('a,b\n1,2,\n'), [{ a: '1', b: '2', 'field~2': '' }])
    assert.deepEqual(jo.parse('a,b\n,1,2\n'), [{ a: '', b: '1', 'field~2': '2' }])
    assert.deepEqual(jo.parse('\na\n'), [])

    const ja = new Tabnas().use(jsonic).use(Markdown, { object: false })

    assert.deepEqual(ja.parse('a\n1,'), [['1', '']])
    assert.deepEqual(ja.parse('a\n,1'), [['', '1']])
    assert.deepEqual(ja.parse('a,b\n1,2,'), [['1', '2', '']])
    assert.deepEqual(ja.parse('a,b\n,1,2'), [['', '1', '2']])
    assert.deepEqual(ja.parse('\n1'), [])
  })

  test('separators', async () => {
    const jd = new Tabnas().use(jsonic).use(Markdown, {
      field: {
        separation: '|',
      },
    })

    assert.deepEqual(jd.parse('a|b|c\nA|B|C\nAA|BB|CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])

    const jD = new Tabnas().use(jsonic).use(Markdown, {
      field: {
        separation: '~~',
      },
    })

    assert.deepEqual(jD.parse('a~~b~~c\nA~~B~~C\nAA~~BB~~CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])

    const jn = new Tabnas().use(jsonic).use(Markdown, {
      record: {
        separators: '%',
      },
    })

    assert.deepEqual(jn.parse('a,b,c%A,B,C%AA,BB,CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])
  })

  test('double-quote', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown)

    assert.deepEqual(j.parse('a\n"b"'), [{ a: 'b' }])

    assert.deepEqual(j.parse('a\n"""b"'), [{ a: '"b' }])
    assert.deepEqual(j.parse('a\n"b"""'), [{ a: 'b"' }])
    assert.deepEqual(j.parse('a\n"""b"""'), [{ a: '"b"' }])
    assert.deepEqual(j.parse('a\n"b""c"'), [{ a: 'b"c' }])

    assert.deepEqual(j.parse('a\n"b""c""d"'), [{ a: 'b"c"d' }])
    assert.deepEqual(j.parse('a\n"b""c""d""e"'), [{ a: 'b"c"d"e' }])

    assert.deepEqual(j.parse('a\n"""b"'), [{ a: '"b' }])
    assert.deepEqual(j.parse('a\n"b"""'), [{ a: 'b"' }])
    assert.deepEqual(j.parse('a\n"""b"""'), [{ a: '"b"' }])

    assert.deepEqual(j.parse('a\n"""""b"'), [{ a: '""b' }])
    assert.deepEqual(j.parse('a\n"b"""""'), [{ a: 'b""' }])
    assert.deepEqual(j.parse('a\n"""""b"""""'), [{ a: '""b""' }])
  })

  test('trim', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown)

    assert.deepEqual(j.parse('a\n b'), [{ a: ' b' }])
    assert.deepEqual(j.parse('a\nb '), [{ a: 'b ' }])
    assert.deepEqual(j.parse('a\n b '), [{ a: ' b ' }])
    assert.deepEqual(j.parse('a\n  b   '), [{ a: '  b   ' }])
    assert.deepEqual(j.parse('a\n \tb \t '), [{ a: ' \tb \t ' }])

    assert.deepEqual(j.parse('a\n b c'), [{ a: ' b c' }])
    assert.deepEqual(j.parse('a\nb c '), [{ a: 'b c ' }])
    assert.deepEqual(j.parse('a\n b c '), [{ a: ' b c ' }])
    assert.deepEqual(j.parse('a\n  b c   '), [{ a: '  b c   ' }])
    assert.deepEqual(j.parse('a\n \tb c \t '), [{ a: ' \tb c \t ' }])

    const jt = new Tabnas().use(jsonic).use(Markdown, { trim: true })

    assert.deepEqual(jt.parse('a\n b'), [{ a: 'b' }])
    assert.deepEqual(jt.parse('a\nb '), [{ a: 'b' }])
    assert.deepEqual(jt.parse('a\n b '), [{ a: 'b' }])
    assert.deepEqual(jt.parse('a\n  b   '), [{ a: 'b' }])
    assert.deepEqual(jt.parse('a\n \tb \t '), [{ a: 'b' }])

    assert.deepEqual(jt.parse('a\n b c'), [{ a: 'b c' }])
    assert.deepEqual(jt.parse('a\nb c '), [{ a: 'b c' }])
    assert.deepEqual(jt.parse('a\n b c '), [{ a: 'b c' }])
    assert.deepEqual(jt.parse('a\n  b c   '), [{ a: 'b c' }])
    assert.deepEqual(jt.parse('a\n \tb c \t '), [{ a: 'b c' }])
  })

  test('comment', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown)
    assert.deepEqual(j.parse('a\n# b'), [{ a: '# b' }])
    assert.deepEqual(j.parse('a\n b #c'), [{ a: ' b #c' }])

    const jc = new Tabnas().use(jsonic).use(Markdown, { comment: true })
    assert.deepEqual(jc.parse('a\n# b'), [])
    assert.deepEqual(jc.parse('a\n b #c'), [{ a: ' b ' }])

    const jt = new Tabnas().use(jsonic).use(Markdown, { strict: false })
    assert.deepEqual(jt.parse('a\n# b'), [])
    assert.deepEqual(jt.parse('a\n b '), [{ a: 'b' }])
  })

  test('number', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown)
    assert.deepEqual(j.parse('a\n1'), [{ a: '1' }])
    assert.deepEqual(j.parse('a\n1e2'), [{ a: '1e2' }])

    const jn = new Tabnas().use(jsonic).use(Markdown, { number: true })
    assert.deepEqual(jn.parse('a\n1'), [{ a: 1 }])
    assert.deepEqual(jn.parse('a\n1e2'), [{ a: 100 }])

    const jt = new Tabnas().use(jsonic).use(Markdown, { strict: false })
    assert.deepEqual(jt.parse('a\n1'), [{ a: 1 }])
    assert.deepEqual(jt.parse('a\n1e2'), [{ a: 100 }])
  })

  test('value', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown)
    assert.deepEqual(j.parse('a\ntrue'), [{ a: 'true' }])
    assert.deepEqual(j.parse('a\nfalse'), [{ a: 'false' }])
    assert.deepEqual(j.parse('a\nnull'), [{ a: 'null' }])

    const jv = new Tabnas().use(jsonic).use(Markdown, { value: true })
    assert.deepEqual(jv.parse('a\ntrue'), [{ a: true }])
    assert.deepEqual(jv.parse('a\nfalse'), [{ a: false }])
    assert.deepEqual(jv.parse('a\nnull'), [{ a: null }])
  })

  test('stream', () => {
    return new Promise<void>((resolve) => {
      let tmp: any = {}
      let data: any[]
      const j = new Tabnas().use(jsonic).use(Markdown, {
        stream: (what: string, record?: any[]) => {
          if ('start' === what) {
            data = []
            tmp.start = Date.now()
          } else if ('record' === what) {
            data.push(record)
          } else if ('end' === what) {
            tmp.end = Date.now()

            assert.deepEqual(data, [
              { a: '1', b: '2' },
              { a: '3', b: '4' },
              { a: '5', b: '6' },
            ])

            assert.ok(tmp.start <= tmp.end)

            resolve()
          }
        },
      })

      j.parse('a,b\n1,2\n3,4\n5,6')
    })
  })

  test('unstrict', async () => {
    const j = new Tabnas().use(jsonic).use(Markdown, { strict: false })
    let d0 = j.parse(`a,b,c
true,[1,2],{x:{y:"q\\"w"}}
 x , 'y\\'y', "z\\"z"
`)
    assert.deepEqual(d0, [
      {
        a: true,
        b: [1, 2],
        c: {
          x: {
            y: 'q"w',
          },
        },
      },
      {
        a: 'x',
        b: "y'y",
        c: 'z"z',
      },
    ])

    assert.throws(() => j.parse('a\n{x:1}y'), /unexpected/)
  })

  test('fixtures', async () => {
    const markdown = new Tabnas().use(jsonic).use(Markdown)
    for (const [key, entry] of Object.entries(manifest) as [string, any][]) {
      const name: string = entry.name

      let parser = markdown
      if (entry.opt) {
        // Apply jsonicOpt after loading the jsonic grammar: the grammar plugin
        // re-applies the base JSON options, so engine options (e.g. a custom
        // comment char) must be set afterwards to take effect.
        let j = new Tabnas().use(jsonic)
        if (entry.jsonicOpt) {
          j.options(entry.jsonicOpt)
        }
        parser = j.use(Markdown, entry.opt)
      }
      const csvFile = entry.csvFile || key
      const raw = readFileSync(join(fixturesDir, csvFile + '.csv'), 'utf8')

      if (entry.err) {
        try {
          parser.parse(raw)
          assert.fail('Expected error ' + entry.err + ' for fixture: ' + name)
        } catch (e: any) {
          assert.deepEqual(entry.err, e.code)
        }
      } else {
        try {
          const expected = JSON.parse(
            readFileSync(join(fixturesDir, key + '.json'), 'utf8'),
          )
          const out = parser.parse(raw)
          assert.deepEqual(out, expected)
        } catch (e: any) {
          console.error('FIXTURE: ' + name)
          throw e
        }
      }
    }
  })
})
