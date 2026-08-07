#!/usr/bin/env node

// GFM extension conformance, over the vendored corpus in test/gfm/spec.json.
//
//   node ts/tools/gfm-conformance.mjs [--failures] [--section=Tables]
//
// Only the *extension* sections of the GFM spec are vendored — 24 examples.
// The core CommonMark examples in that document are deliberately excluded:
// cmark-gfm tracks CommonMark 0.29, and nine of its emphasis cases expect
// pre-0.31.2 nested-strong output that this parser (correctly) no longer
// produces. Core conformance is `conformance.mjs`, against 0.31.2.
//
// Runs with `gfm:true`, since that is what these extensions are gated on.
// Same staging trick as conformance.mjs — see the comment there.

import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'

const HERE = dirname(fileURLToPath(import.meta.url))
const SRC = join(HERE, '..', 'src')
const SPEC = join(HERE, '..', '..', 'test', 'gfm', 'spec.json')

const args = process.argv.slice(2)
const showFailures = args.includes('--failures')
const onlySection = args.find((a) => a.startsWith('--section='))?.slice(10)

const stage = join(tmpdir(), 'gfm-conformance-' + process.pid)
mkdirSync(stage, { recursive: true })
try {
  writeFileSync(join(stage, 'package.json'), '{"type":"module"}')
  for (const f of readdirSync(SRC)) {
    if (f.endsWith('.ts')) writeFileSync(join(stage, f), readFileSync(join(SRC, f), 'utf8'))
  }

  writeFileSync(
    join(stage, '__run.ts'),
    `
import { readFileSync } from 'node:fs'
import { parse } from './commonmark.ts'
import { renderHTML } from './html.ts'

const cases = JSON.parse(readFileSync(process.argv[2], 'utf8'))
const out = []
for (const c of cases) {
  let actual = null, error = null
  try {
    actual = renderHTML(parse(c.markdown, { gfm: true, breaks: false }), { gfm: true, breaks: false })
  } catch (e) {
    error = String((e && e.stack) || e).split('\\n').slice(0, 3).join(' | ')
  }
  out.push({ ...c, actual, error, pass: null === error && actual === c.html })
}
process.stdout.write(JSON.stringify(out))
`,
  )

  let cases = JSON.parse(readFileSync(SPEC, 'utf8'))
  if (onlySection) cases = cases.filter((c) => c.section.startsWith(onlySection))
  const casesFile = join(stage, 'cases.json')
  writeFileSync(casesFile, JSON.stringify(cases))

  let raw
  try {
    raw = execFileSync(
      process.execPath,
      ['--experimental-strip-types', '--no-warnings', join(stage, '__run.ts'), casesFile],
      { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, cwd: stage },
    )
  } catch (e) {
    process.stderr.write('gfm conformance runner failed to execute:\n')
    process.stderr.write((e.stderr || e.message || String(e)) + '\n')
    process.exit(2)
  }

  const results = JSON.parse(raw)
  const bySection = new Map()
  for (const r of results) {
    const s = bySection.get(r.section) ?? { total: 0, pass: 0 }
    s.total++
    if (r.pass) s.pass++
    bySection.set(r.section, s)
  }

  const width = Math.max(...[...bySection.keys()].map((k) => k.length), 10)
  console.log('\nGFM extensions (gfm:true)\n')
  for (const [name, s] of [...bySection.entries()].sort(
    (a, b) => a[1].pass / a[1].total - b[1].pass / b[1].total,
  )) {
    const mark = s.pass === s.total ? 'OK ' : '   '
    console.log(`  ${mark} ${name.padEnd(width)}  ${String(s.pass).padStart(2)}/${String(s.total).padEnd(2)}`)
  }
  const passed = results.filter((r) => r.pass).length
  console.log(`\n  TOTAL  ${passed}/${results.length}\n`)

  if (showFailures) {
    for (const f of results.filter((r) => !r.pass)) {
      console.log('─'.repeat(72))
      console.log(`Example ${f.example}  [${f.section}]`)
      console.log('--- markdown ---'); console.log(JSON.stringify(f.markdown))
      console.log('--- expected ---'); console.log(JSON.stringify(f.html))
      console.log('--- actual   ---'); console.log(JSON.stringify(f.actual))
      if (f.error) { console.log('--- error ---'); console.log(f.error) }
    }
  }

  process.exit(passed === results.length ? 0 : 1)
} finally {
  rmSync(stage, { recursive: true, force: true })
}
