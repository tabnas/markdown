#!/usr/bin/env node

// CommonMark 0.31.2 conformance runner.
//
//   node ts/tools/conformance.mjs               # summary + per-section table
//   node ts/tools/conformance.mjs --failures    # also dump every failing case
//   node ts/tools/conformance.mjs --section=Tabs
//   node ts/tools/conformance.mjs --example=42
//   node ts/tools/conformance.mjs --json        # machine-readable, for CI
//
// Why the copy-to-tmp dance: `ts/package.json` declares `"type": "commonjs"`
// (the published package is CJS), so Node's type stripping loads `src/*.ts`
// as CommonJS and rejects their `import` statements. Staging the sources in a
// directory whose package.json says `"type": "module"` lets the suite run
// against the real files with no build step and no `@tabnas/parser` present.
// The shipped build is still plain `tsc`; this is a dev harness only.

import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync, copyFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'

const HERE = dirname(fileURLToPath(import.meta.url))
const SRC = join(HERE, '..', 'src')
const SPEC = join(HERE, '..', '..', 'test', 'commonmark', 'spec.json')

const args = process.argv.slice(2)
const showFailures = args.includes('--failures')
const asJson = args.includes('--json')
const onlySection = args.find((a) => a.startsWith('--section='))?.slice(10)
const onlyExample = args.find((a) => a.startsWith('--example='))?.slice(10)

const stage = join(tmpdir(), 'cm-conformance-' + process.pid)
mkdirSync(stage, { recursive: true })
try {
  writeFileSync(join(stage, 'package.json'), '{"type":"module"}')
  for (const f of readdirSync(SRC)) {
    if (f.endsWith('.ts')) copyFileSync(join(SRC, f), join(stage, f))
  }

  // The driver runs inside the staged directory so its relative `.ts` imports
  // resolve to the copies. It emits one JSON line so we can parse results back.
  writeFileSync(
    join(stage, '__run.ts'),
    `
import { readFileSync } from 'node:fs'
import { parse } from './commonmark.ts'
import { renderHTML } from './html.ts'

const cases = JSON.parse(readFileSync(process.argv[2], 'utf8'))
const out = []
for (const c of cases) {
  let actual = null
  let error = null
  try {
    // The CommonMark suite is pure CommonMark: GFM extensions must be off or
    // they change results for tables, strikethrough and autolink literals.
    actual = renderHTML(parse(c.markdown, { gfm: false, breaks: false }))
  } catch (e) {
    error = (e && e.stack) ? String(e.stack).split('\\n').slice(0, 3).join(' | ') : String(e)
  }
  out.push({
    example: c.example,
    section: c.section,
    markdown: c.markdown,
    expected: c.html,
    actual,
    error,
    pass: null === error && actual === c.html,
  })
}
process.stdout.write(JSON.stringify(out))
`,
  )

  let cases = JSON.parse(readFileSync(SPEC, 'utf8'))
  if (onlySection) cases = cases.filter((c) => c.section === onlySection)
  if (onlyExample) cases = cases.filter((c) => String(c.example) === onlyExample)

  const casesFile = join(stage, 'cases.json')
  writeFileSync(casesFile, JSON.stringify(cases))

  let raw
  try {
    raw = execFileSync(
      process.execPath,
      ['--experimental-strip-types', '--no-warnings', join(stage, '__run.ts'), casesFile],
      { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, cwd: stage },
    )
  } catch (e) {
    // A hard crash (parser threw at module load, syntax error, OOM) means the
    // whole run is void — report it as such rather than as 652 failures.
    process.stderr.write('conformance runner failed to execute:\n')
    process.stderr.write((e.stderr || e.message || String(e)) + '\n')
    process.exit(2)
  }

  const results = JSON.parse(raw)
  const passed = results.filter((r) => r.pass)
  const failed = results.filter((r) => !r.pass)

  const bySection = new Map()
  for (const r of results) {
    const s = bySection.get(r.section) ?? { total: 0, pass: 0 }
    s.total++
    if (r.pass) s.pass++
    bySection.set(r.section, s)
  }

  if (asJson) {
    console.log(
      JSON.stringify(
        {
          total: results.length,
          passed: passed.length,
          failed: failed.length,
          pct: results.length ? +((100 * passed.length) / results.length).toFixed(2) : 0,
          sections: Object.fromEntries(bySection),
          failures: failed.map((f) => ({
            example: f.example,
            section: f.section,
            markdown: f.markdown,
            expected: f.expected,
            actual: f.actual,
            error: f.error,
          })),
        },
        null,
        1,
      ),
    )
  } else {
    const width = Math.max(...[...bySection.keys()].map((k) => k.length), 10)
    console.log('\nCommonMark 0.31.2 conformance\n')
    for (const [name, s] of [...bySection.entries()].sort(
      (a, b) => a[1].pass / a[1].total - b[1].pass / b[1].total,
    )) {
      const pct = ((100 * s.pass) / s.total).toFixed(0).padStart(3)
      const bar = s.pass === s.total ? 'OK ' : '   '
      console.log(`  ${bar} ${name.padEnd(width)}  ${String(s.pass).padStart(3)}/${String(s.total).padEnd(3)}  ${pct}%`)
    }
    const pct = results.length ? ((100 * passed.length) / results.length).toFixed(2) : '0.00'
    console.log(`\n  TOTAL  ${passed.length}/${results.length}  ${pct}%\n`)

    if (showFailures) {
      for (const f of failed) {
        console.log('─'.repeat(72))
        console.log(`Example ${f.example}  [${f.section}]`)
        console.log('--- markdown ---'); console.log(JSON.stringify(f.markdown))
        console.log('--- expected ---'); console.log(JSON.stringify(f.expected))
        console.log('--- actual   ---'); console.log(JSON.stringify(f.actual))
        if (f.error) { console.log('--- error    ---'); console.log(f.error) }
      }
    }
  }

  process.exit(failed.length ? 1 : 0)
} finally {
  rmSync(stage, { recursive: true, force: true })
}
