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
// The stand-in implements only what the plugin actually touches: `options`
// merging (the `mdLine` matcher registration, `parse.prepare`,
// `lex.emptyResult`), the `rule` chain, and a `parse` that drives the
// plugin's own matcher and alt actions the way the engine's lexer and rule
// loop would (see the wiring at the bottom of markdown.ts). An example that
// passes here exercises the same shared parser core the engine path calls.
// It does NOT check engine-level behaviour — real token consumption, rule
// dispatch order, error handling — which remains the CI harness's job.

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
    `// Stand-in for @tabnas/parser: only what the Markdown plugin touches —
// options merging (matcher registration, parse.prepare, lex.emptyResult),
// the rule() chain, and a parse() that drives the plugin's own mdLine
// matcher and alt actions the way the engine's lexer and rule loop would.
export type Plugin = any
export type Rule = any
export type Context = any
export type MakeLexMatcher = any
export class Tabnas {
  _opts: any = { lex: { match: {} }, parse: { prepare: {} } }
  _rules: any = {}
  options(o: any) {
    for (const k of Object.keys(o)) {
      if ('lex' === k) {
        Object.assign(this._opts.lex.match, o.lex.match ?? {})
        if ('emptyResult' in o.lex) this._opts.lex.emptyResult = o.lex.emptyResult
      } else if ('parse' === k) {
        Object.assign(this._opts.parse.prepare, o.parse.prepare ?? {})
      } else {
        this._opts[k] = o[k]
      }
    }
    return this
  }
  rule(name: string, def: any) {
    const spec: any = { open: [], close: [] }
    const rs = {
      clear() { spec.open = []; spec.close = []; return rs },
      bo() { return rs },
      open(alts: any) { spec.open = alts; return rs },
      close(alts: any) { spec.close = alts; return rs },
    }
    def(rs)
    this._rules[name] = spec
    return this
  }
  use(plugin: any, opts?: any) { plugin(this, opts); return this }
  parse(src: string, meta?: any) {
    const ctx: any = { u: {}, meta }
    for (const name of Object.keys(this._opts.parse.prepare)) {
      this._opts.parse.prepare[name](this, ctx, meta)
    }
    if ('' === src) return this._opts.lex.emptyResult

    const matcher = this._opts.lex.match.mdLine.make(null, null)
    const pnt = { sI: 0, rI: 1, cI: 1 }
    const lex = {
      src,
      pnt,
      token: (name: string, val: any, tsrc: any, _p: any, use?: any) =>
        ({ name, val, src: tsrc, use }),
    }

    const lineAlt = this._rules.line.open[0]
    let tkn: any
    while (undefined !== (tkn = matcher(lex, null))) {
      lineAlt.a({ o0: tkn }, ctx)
    }

    const finishAlt = this._rules.markdown.close[0]
    const r: any = { node: undefined }
    finishAlt.a(r, ctx)
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
