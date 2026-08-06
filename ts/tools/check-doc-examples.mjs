#!/usr/bin/env node

// Verify the `// =>` assertions in the documentation, without the engine.
//
//   node ts/tools/check-doc-examples.mjs [--verbose]
//
// `ts/test/doc-examples.test.ts` is the real harness and runs in CI against a
// built package and a real `@tabnas/parser`. This is the same check for
// environments where the engine is not installable — it stages `src/*.ts`,
// substitutes a small stand-in for the engine, and evaluates each documented
// example so a wrong `// =>` value cannot be committed.
//
// The stand-in implements only what the plugin actually touches: `options`,
// `rule` with the `clear/bo/open/close` chain, and a `parse` that invokes the
// registered `bo` with a context exposing `src()`. That is the whole of the
// plugin's contact surface (see the wiring at the bottom of markdown.ts), so
// an example that passes here exercises the same parser the engine would call.
// It does NOT check engine-level behaviour — token consumption, error
// handling, `lex.emptyResult` — which remains the CI harness's job.

import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync, existsSync, statSync } from 'node:fs'
import { join, dirname, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'

const HERE = dirname(fileURLToPath(import.meta.url))
const TS_DIR = join(HERE, '..')
const REPO = join(TS_DIR, '..')
const verbose = process.argv.includes('--verbose')

// Same locations the CI harness walks.
const DOC_TARGETS = ['README.md', 'ts/README.md', 'go/README.md', 'ts/doc', 'go/doc']

function collectMarkdown() {
  const out = []
  const walk = (p) => {
    if (!existsSync(p)) return
    const st = statSync(p)
    if (st.isFile()) {
      if (p.endsWith('.md')) out.push(p)
      return
    }
    for (const e of readdirSync(p)) walk(join(p, e))
  }
  for (const t of DOC_TARGETS) walk(join(REPO, t))
  return out
}

// Extract fenced blocks. `ignore` in the info string opts a block out, matching
// the CI harness.
function extractBlocks(src) {
  const lines = src.split('\n')
  const blocks = []
  let cur = null
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^```(\w*)(.*)$/)
    if (m) {
      if (cur) {
        blocks.push(cur)
        cur = null
      } else {
        const lang = m[1].toLowerCase()
        if ('js' === lang || 'javascript' === lang) {
          cur = { code: [], startLine: i + 1, ignore: /\bignore\b/.test(m[2]) }
        }
      }
      continue
    }
    if (cur) cur.code.push(lines[i])
  }
  return blocks
}

const stage = join(tmpdir(), 'cm-docs-' + process.pid)
mkdirSync(stage, { recursive: true })

try {
  writeFileSync(join(stage, 'package.json'), '{"type":"module"}')
  for (const f of readdirSync(join(TS_DIR, 'src'))) {
    if (!f.endsWith('.ts')) continue
    let body = readFileSync(join(TS_DIR, 'src', f), 'utf8')
    // Point the plugin at the stand-in instead of the real engine.
    body = body.replace(/from '@tabnas\/parser'/g, "from './__engine.ts'")
    writeFileSync(join(stage, f), body)
  }

  writeFileSync(
    join(stage, '__engine.ts'),
    `// Stand-in for @tabnas/parser: only what the Markdown plugin touches.
export type Plugin = any
export type Rule = any
export type Context = any
export class Tabnas {
  _opts: any = {}
  _bo: any = null
  options(o: any) { Object.assign(this._opts, o); return this }
  rule(_name: string, def: any) {
    const self = this
    const rs = {
      clear() { return rs },
      bo(fn: any) { self._bo = fn; return rs },
      open() { return rs },
      close() { return rs },
    }
    def(rs)
    return this
  }
  use(plugin: any, opts?: any) { plugin(this, opts); return this }
  parse(src: string) {
    const r: any = { node: undefined }
    this._bo(r, { src: () => src })
    return r.node
  }
}
`,
  )

  const files = collectMarkdown()
  let tested = 0
  let failed = 0
  const failures = []

  for (const file of files) {
    const rel = relative(REPO, file)
    const blocks = extractBlocks(readFileSync(file, 'utf8'))

    blocks.forEach((b, bi) => {
      if (b.ignore) return
      const joined = b.code.join('\n')
      if (!/\/\/\s*=>/.test(joined)) return

      // `<expr> // => <expected>` becomes a deep-equality assertion. The
      // expected side is evaluated as a JS expression, so it may be an object
      // literal, a string, a number, etc.
      let count = 0
      const rewritten = joined
        .replace(/from ['"]@tabnas\/markdown['"]/g, `from '${stage}/markdown.ts'`)
        .replace(/from ['"]@tabnas\/parser['"]/g, `from '${stage}/__engine.ts'`)
        .split('\n')
        .map((line) => {
          const m = line.match(/^(\s*)(.+?)\s*\/\/\s*=>\s*(.+?)\s*$/)
          if (!m) return line
          count++
          const [, indent, expr, expected] = m
          const cleaned = expr.replace(/;\s*$/, '')
          return `${indent}__eq(${cleaned}, ${expected}, ${JSON.stringify(cleaned)});`
        })
        .join('\n')

      if (0 === count) return
      tested++

      const runner = join(stage, `__doc_${tested}.ts`)
      writeFileSync(
        runner,
        `function __eq(actual: any, expected: any, label: string) {
  const a = JSON.stringify(actual)
  const b = JSON.stringify(expected)
  if (a !== b) {
    console.error('MISMATCH: ' + label)
    console.error('  actual:   ' + a)
    console.error('  expected: ' + b)
    process.exitCode = 1
  }
}
${rewritten}
`,
      )

      try {
        const out = execFileSync(
          process.execPath,
          ['--experimental-strip-types', '--no-warnings', runner],
          { encoding: 'utf8', cwd: stage, stdio: ['ignore', 'pipe', 'pipe'] },
        )
        if (verbose) console.log(`  ok   ${rel} block #${bi + 1} (line ${b.startLine}) — ${count} assertion(s)`)
        if (out.trim() && verbose) console.log(out.trim())
      } catch (e) {
        failed++
        failures.push({
          where: `${rel} block #${bi + 1} (line ${b.startLine})`,
          detail: ((e.stdout || '') + (e.stderr || '')).trim(),
        })
      }
    })
  }

  for (const f of failures) {
    console.error(`\nFAIL ${f.where}`)
    console.error(f.detail.split('\n').slice(0, 12).map((l) => '  ' + l).join('\n'))
  }

  console.log(`\ndoc examples: ${tested - failed}/${tested} blocks passed across ${files.length} files\n`)
  process.exit(failed ? 1 : 0)
} finally {
  rmSync(stage, { recursive: true, force: true })
}
