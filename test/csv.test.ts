/* Copyright (c) 2021-2024 Richard Rodger and other contributors, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import Util from 'util'

import { Jsonic } from 'jsonic'
import { Csv } from '../dist/csv'

const Spectrum = require('csv-spectrum')

const fixturesDir = join(__dirname, '..', 'test', 'fixtures')
const manifest = JSON.parse(
  readFileSync(join(fixturesDir, 'manifest.json'), 'utf8'),
)

describe('csv', () => {
  test('empty-records', async () => {
    // ignored by default

    const jo = Jsonic.make().use(Csv)
    assert.deepEqual(jo('\n'), [])
    assert.deepEqual(jo('a\n1\n\n2\n3\n\n\n4\n'), [
      { a: '1' },
      { a: '2' },
      { a: '3' },
      { a: '4' },
    ])

    const ja = Jsonic.make().use(Csv, { object: false })
    assert.deepEqual(ja('\n'), [])
    assert.deepEqual(ja('a\n1\n\n2\n3\n\n\n4\n'), [['1'], ['2'], ['3'], ['4']])

    // start and end also ignored

    assert.deepEqual(jo('\r\na,b\r\nA,B\r\n'), [{ a: 'A', b: 'B' }])
    assert.deepEqual(jo('\r\n\r\na,b\r\nA,B\r\n\r\n'), [{ a: 'A', b: 'B' }])
    assert.deepEqual(ja('\r\na,b\r\nA,B\r\n'), [['A', 'B']])
    assert.deepEqual(ja('\r\n\r\na,b\r\nA,B\r\n\r\n'), [['A', 'B']])

    // with option, empty creates record

    const jon = Jsonic.make().use(Csv, { record: { empty: true } })
    assert.deepEqual(jon('\n'), [])
    assert.deepEqual(jon('a\n1\n\n2\n3\n\n\n4\n'), [
      { a: '1' },
      { a: '' },
      { a: '2' },
      { a: '3' },
      { a: '' },
      { a: '' },
      { a: '4' },
    ])

    // with comments

    const joc = Jsonic.make().use(Csv, { comment: true })
    // console.log(joc('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'))
    assert.deepEqual(joc('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'), [
      { a: '1' },
      { a: '2' },
      { a: '3' },
      { a: '4' },
    ])

    const jocn = Jsonic.make().use(Csv, {
      comment: true,
      record: { empty: true },
    })
    assert.deepEqual(jocn('a#X\n1\n#Y\n2\n3\n\n#Z\n4\n#Q'), [
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
    const jo = Jsonic.make().use(Csv)
    assert.deepEqual(jo('\n'), [])
    assert.deepEqual(jo('\na,b\nA,B'), [{ a: 'A', b: 'B' }])

    const ja = Jsonic.make().use(Csv, { object: false })
    assert.deepEqual(ja('\n'), [])
    assert.deepEqual(ja('\na,b\nA,B'), [['A', 'B']])

    const jon = Jsonic.make().use(Csv, { header: false })
    assert.deepEqual(jon('\n'), [])
    assert.deepEqual(jon('\na,b\nA,B'), [
      {
        'field~0': 'a',
        'field~1': 'b',
      },
      {
        'field~0': 'A',
        'field~1': 'B',
      },
    ])

    const jan = Jsonic.make().use(Csv, { header: false, object: false })
    assert.deepEqual(jan('\n'), [])
    assert.deepEqual(jan('\na,b\nA,B'), [
      ['a', 'b'],
      ['A', 'B'],
    ])

    const jonf = Jsonic.make().use(Csv, {
      header: false,
      field: { names: ['a', 'b'] },
    })
    assert.deepEqual(jonf('\n'), [])
    assert.deepEqual(jonf('\na,b\nA,B'), [
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
    const jo = Jsonic.make().use(Csv)

    assert.deepEqual(jo('\na'), [])
    assert.deepEqual(jo('a\n1,'), [{ a: '1', 'field~1': '' }])
    assert.deepEqual(jo('a\n,1'), [{ a: '', 'field~1': '1' }])
    assert.deepEqual(jo('a,b\n1,2,'), [{ a: '1', b: '2', 'field~2': '' }])
    assert.deepEqual(jo('a,b\n,1,2'), [{ a: '', b: '1', 'field~2': '2' }])

    assert.deepEqual(jo('a\n1,\n'), [{ a: '1', 'field~1': '' }])
    assert.deepEqual(jo('a\n,1\n'), [{ a: '', 'field~1': '1' }])
    assert.deepEqual(jo('a,b\n1,2,\n'), [{ a: '1', b: '2', 'field~2': '' }])
    assert.deepEqual(jo('a,b\n,1,2\n'), [{ a: '', b: '1', 'field~2': '2' }])
    assert.deepEqual(jo('\na\n'), [])

    const ja = Jsonic.make().use(Csv, { object: false })

    assert.deepEqual(ja('a\n1,'), [['1', '']])
    assert.deepEqual(ja('a\n,1'), [['', '1']])
    assert.deepEqual(ja('a,b\n1,2,'), [['1', '2', '']])
    assert.deepEqual(ja('a,b\n,1,2'), [['', '1', '2']])
    assert.deepEqual(ja('\n1'), [])
  })

  test('separators', async () => {
    const jd = Jsonic.make().use(Csv, {
      field: {
        separation: '|',
      },
    })

    assert.deepEqual(jd('a|b|c\nA|B|C\nAA|BB|CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])

    const jD = Jsonic.make().use(Csv, {
      field: {
        separation: '~~',
      },
    })

    assert.deepEqual(jD('a~~b~~c\nA~~B~~C\nAA~~BB~~CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])

    const jn = Jsonic.make().use(Csv, {
      record: {
        separators: '%',
      },
    })

    assert.deepEqual(jn('a,b,c%A,B,C%AA,BB,CC'), [
      { a: 'A', b: 'B', c: 'C' },
      { a: 'AA', b: 'BB', c: 'CC' },
    ])
  })

  test('double-quote', async () => {
    const j = Jsonic.make().use(Csv)

    assert.deepEqual(j('a\n"b"'), [{ a: 'b' }])

    assert.deepEqual(j('a\n"""b"'), [{ a: '"b' }])
    assert.deepEqual(j('a\n"b"""'), [{ a: 'b"' }])
    assert.deepEqual(j('a\n"""b"""'), [{ a: '"b"' }])
    assert.deepEqual(j('a\n"b""c"'), [{ a: 'b"c' }])

    assert.deepEqual(j('a\n"b""c""d"'), [{ a: 'b"c"d' }])
    assert.deepEqual(j('a\n"b""c""d""e"'), [{ a: 'b"c"d"e' }])

    assert.deepEqual(j('a\n"""b"'), [{ a: '"b' }])
    assert.deepEqual(j('a\n"b"""'), [{ a: 'b"' }])
    assert.deepEqual(j('a\n"""b"""'), [{ a: '"b"' }])

    assert.deepEqual(j('a\n"""""b"'), [{ a: '""b' }])
    assert.deepEqual(j('a\n"b"""""'), [{ a: 'b""' }])
    assert.deepEqual(j('a\n"""""b"""""'), [{ a: '""b""' }])
  })

  test('trim', async () => {
    const j = Jsonic.make().use(Csv)

    assert.deepEqual(j('a\n b'), [{ a: ' b' }])
    assert.deepEqual(j('a\nb '), [{ a: 'b ' }])
    assert.deepEqual(j('a\n b '), [{ a: ' b ' }])
    assert.deepEqual(j('a\n  b   '), [{ a: '  b   ' }])
    assert.deepEqual(j('a\n \tb \t '), [{ a: ' \tb \t ' }])

    assert.deepEqual(j('a\n b c'), [{ a: ' b c' }])
    assert.deepEqual(j('a\nb c '), [{ a: 'b c ' }])
    assert.deepEqual(j('a\n b c '), [{ a: ' b c ' }])
    assert.deepEqual(j('a\n  b c   '), [{ a: '  b c   ' }])
    assert.deepEqual(j('a\n \tb c \t '), [{ a: ' \tb c \t ' }])

    const jt = Jsonic.make().use(Csv, { trim: true })

    assert.deepEqual(jt('a\n b'), [{ a: 'b' }])
    assert.deepEqual(jt('a\nb '), [{ a: 'b' }])
    assert.deepEqual(jt('a\n b '), [{ a: 'b' }])
    assert.deepEqual(jt('a\n  b   '), [{ a: 'b' }])
    assert.deepEqual(jt('a\n \tb \t '), [{ a: 'b' }])

    assert.deepEqual(jt('a\n b c'), [{ a: 'b c' }])
    assert.deepEqual(jt('a\nb c '), [{ a: 'b c' }])
    assert.deepEqual(jt('a\n b c '), [{ a: 'b c' }])
    assert.deepEqual(jt('a\n  b c   '), [{ a: 'b c' }])
    assert.deepEqual(jt('a\n \tb c \t '), [{ a: 'b c' }])
  })

  test('comment', async () => {
    const j = Jsonic.make().use(Csv)
    assert.deepEqual(j('a\n# b'), [{ a: '# b' }])
    assert.deepEqual(j('a\n b #c'), [{ a: ' b #c' }])

    const jc = Jsonic.make().use(Csv, { comment: true })
    assert.deepEqual(jc('a\n# b'), [])
    assert.deepEqual(jc('a\n b #c'), [{ a: ' b ' }])

    const jt = Jsonic.make().use(Csv, { strict: false })
    assert.deepEqual(jt('a\n# b'), [])
    assert.deepEqual(jt('a\n b '), [{ a: 'b' }])
  })

  test('number', async () => {
    const j = Jsonic.make().use(Csv)
    assert.deepEqual(j('a\n1'), [{ a: '1' }])
    assert.deepEqual(j('a\n1e2'), [{ a: '1e2' }])

    const jn = Jsonic.make().use(Csv, { number: true })
    assert.deepEqual(jn('a\n1'), [{ a: 1 }])
    assert.deepEqual(jn('a\n1e2'), [{ a: 100 }])

    const jt = Jsonic.make().use(Csv, { strict: false })
    assert.deepEqual(jt('a\n1'), [{ a: 1 }])
    assert.deepEqual(jt('a\n1e2'), [{ a: 100 }])
  })

  test('value', async () => {
    const j = Jsonic.make().use(Csv)
    assert.deepEqual(j('a\ntrue'), [{ a: 'true' }])
    assert.deepEqual(j('a\nfalse'), [{ a: 'false' }])
    assert.deepEqual(j('a\nnull'), [{ a: 'null' }])

    const jv = Jsonic.make().use(Csv, { value: true })
    assert.deepEqual(jv('a\ntrue'), [{ a: true }])
    assert.deepEqual(jv('a\nfalse'), [{ a: false }])
    assert.deepEqual(jv('a\nnull'), [{ a: null }])
  })

  test('stream', () => {
    return new Promise<void>((resolve) => {
      let tmp: any = {}
      let data: any[]
      const j = Jsonic.make().use(Csv, {
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

      j('a,b\n1,2\n3,4\n5,6')
    })
  })

  test('unstrict', async () => {
    const j = Jsonic.make().use(Csv, { strict: false })
    let d0 = j(`a,b,c
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

    assert.throws(() => j('a\n{x:1}y'), /unexpected/)
  })

  test('spectrum', async () => {
    const j = Jsonic.make().use(Csv)
    const tests = await Util.promisify(Spectrum)()
    for (let i = 0; i < tests.length; i++) {
      let test = tests[i]
      let name = test.name
      let json = JSON.parse(test.json.toString())
      let csv = test.csv.toString()
      let res = j(csv)
      let testname = name + ' ' + (i + 1) + '/' + tests.length

      // Broken test, reenable when fixed
      if (5 === i) {
        continue
      }

      assert.deepEqual({ [testname]: res }, { [testname]: json })
    }
  })

  test('fixtures', async () => {
    const csv = Jsonic.make().use(Csv)
    for (const [key, entry] of Object.entries(manifest) as [string, any][]) {
      const name: string = entry.name

      let parser = csv
      if (entry.opt) {
        let j = entry.jsonicOpt ? Jsonic.make(entry.jsonicOpt) : Jsonic.make()
        parser = j.use(Csv, entry.opt)
      }
      const csvFile = entry.csvFile || key
      const raw = readFileSync(join(fixturesDir, csvFile + '.csv'), 'utf8')

      if (entry.err) {
        try {
          parser(raw)
          assert.fail('Expected error ' + entry.err + ' for fixture: ' + name)
        } catch (e: any) {
          assert.deepEqual(entry.err, e.code)
        }
      } else {
        try {
          const expected = JSON.parse(
            readFileSync(join(fixturesDir, key + '.json'), 'utf8'),
          )
          const out = parser(raw)
          assert.deepEqual(out, expected)
        } catch (e: any) {
          console.error('FIXTURE: ' + name)
          throw e
        }
      }
    }
  })
})
