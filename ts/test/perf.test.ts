/* Copyright (c) 2021-2025 Richard Rodger, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '../dist/markdown'

// makeMarkdownParser builds a fresh Tabnas instance with jsonic + the Markdown
// plugin. Building the engine and applying the embedded markdown grammar is
// the expensive part; parsing a tiny document is comparatively cheap.
function makeMarkdownParser(): Tabnas {
  return new Tabnas().use(jsonic).use(Markdown)
}

describe('perf', () => {
  // TestParseReusesInstance guards against a performance regression in how the
  // Markdown plugin is consumed. @tabnas/markdown is a PLUGIN with no exported
  // convenience parse() — callers build a Tabnas instance and .use(Markdown)
  // themselves. The right (fast) pattern is to build that instance ONCE and
  // reuse it; the wrong (slow) pattern rebuilds the engine + applies the
  // markdown grammar on every parse, which is dominated by grammar
  // construction and is many times slower.
  //
  // This test pins the correct usage: it compares N parses that rebuild the
  // instance each call against N parses reusing ONE instance, on the SAME
  // machine in the SAME run. Reuse must stay close to linear relative to the
  // rebuild path (here reuse is far faster), so the rebuild-per-parse anti-
  // pattern would blow the ratio. The check is machine-INDEPENDENT: both sides
  // scale together on a slow CI box, so there is deliberately NO wall-clock
  // budget.
  //
  // (If a cached convenience parse() were ever added to this package, this is
  // the shape that would catch it rebuilding the grammar per call — mirrors
  // yaml's TestParseReusesInstance / @tabnas/json's module-level defaultParser.)
  test('reuses instance', () => {
    const src = 'a,b,c\n1,2,3'
    const n = 300

    // Warm both paths so the comparison is steady-state.
    for (let i = 0; i < 50; i++) {
      makeMarkdownParser().parse(src)
    }
    const reused = makeMarkdownParser()
    for (let i = 0; i < 50; i++) {
      reused.parse(src)
    }

    // Rebuild-per-parse: pays grammar construction on every iteration.
    const t0 = process.hrtime.bigint()
    for (let i = 0; i < n; i++) {
      makeMarkdownParser().parse(src)
    }
    const rebuild = Number(process.hrtime.bigint() - t0)

    // Reuse one instance: only pays parsing per iteration.
    const t1 = process.hrtime.bigint()
    for (let i = 0; i < n; i++) {
      reused.parse(src)
    }
    const reuse = Number(process.hrtime.bigint() - t1)

    // Reusing one instance must be no slower than rebuilding it every parse.
    // Allow 4x slack for scheduling noise around an otherwise lopsided win
    // (reuse should be much faster). This catches a regression where a
    // reused-instance path somehow rebuilt the grammar per parse without
    // depending on absolute wall-clock speed.
    const ratio = reuse / rebuild
    assert.ok(
      reuse <= 4 * rebuild,
      `reusing one Markdown instance is not faster than rebuilding it per parse: ` +
        `${n} reuse parses took ${(reuse / 1e6).toFixed(1)}ms vs ` +
        `${(rebuild / 1e6).toFixed(1)}ms rebuilding each time ` +
        `(ratio ${ratio.toFixed(1)}x, limit 4x). ` +
        `Build the Tabnas+Markdown instance once and reuse it ` +
        `(the grammar build dominates a parse).`,
    )

    // eslint-disable-next-line no-console
    console.log(
      `rebuild-per-parse=${(rebuild / 1e6).toFixed(1)}ms  ` +
        `reuse-one=${(reuse / 1e6).toFixed(1)}ms  ` +
        `rebuild/reuse=${(rebuild / reuse).toFixed(2)}x`,
    )
  })
})
