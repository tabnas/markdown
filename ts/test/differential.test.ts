/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Differential gate: the plugin path must agree with the engine-free path.
//
// `parseDocument(src, opts)` (engine-free) and `new Tabnas().use(Markdown,
// opts).parse(src)` (plugin path) are required to produce the same AST for
// every input, byte for byte after JSON flattening. Today the two paths
// share every line of code, so this suite is trivially green — that is the
// point. It exists so the engine-substrate stages (dx-report §42) cannot
// land a divergence silently: the conformance corpora are blind to several
// legal-input behaviors (see test/spec/edge.tsv), so "all suites green" is
// necessary but not sufficient once the plugin path stops sharing the
// drivers.
//
// Inputs: the full CommonMark corpus, the GFM corpus, every shared fixture,
// and seeded pseudo-random documents composed from the constructs where the
// paths could plausibly diverge (link tails holding backticks, tables in
// containers, trailing spaces, reference definitions...). The generator is
// deterministic — same seeds, same documents, every run — and
// `go/differential_test.go` builds the identical documents with the
// identical generator, so a reproduction case can be named by its seed in
// either runtime.

import { test } from 'node:test'
import assert from 'node:assert'
import * as fs from 'node:fs'
import * as path from 'node:path'

import { Tabnas } from '@tabnas/parser'
import { findSpecDir, loadSpecDir } from '@tabnas/support'

import { Markdown, parseDocument } from '../dist/markdown'

const specDir = findSpecDir(__dirname)

// One engine instance per option set, reused across every parse — the
// supported pattern (see perf.test.ts); a fresh instance per input would
// time out the suite for no extra coverage.
const OPTION_SETS: Array<Record<string, unknown>> = [{}, { gfm: false }]
const instances = OPTION_SETS.map((opts) => new Tabnas().use(Markdown, opts))

function flatten(v: unknown): unknown {
  return JSON.parse(JSON.stringify(v))
}

function checkInput(src: string, where: string): void {
  for (let i = 0; i < OPTION_SETS.length; i++) {
    const direct = parseDocument(src, OPTION_SETS[i])
    const plugin = instances[i].parse(src)
    assert.deepStrictEqual(
      flatten(plugin),
      flatten(direct),
      where + ' opts=' + JSON.stringify(OPTION_SETS[i]),
    )
  }
}

test('differential: CommonMark corpus', () => {
  const cases = JSON.parse(
    fs.readFileSync(path.join(specDir, '..', 'commonmark', 'spec.json'), 'utf8'),
  )
  for (const c of cases) checkInput(c.markdown, 'commonmark example ' + c.example)
})

test('differential: GFM corpus', () => {
  const cases = JSON.parse(
    fs.readFileSync(path.join(specDir, '..', 'gfm', 'spec.json'), 'utf8'),
  )
  for (const c of cases) checkInput(c.markdown, 'gfm example ' + c.example)
})

test('differential: shared fixtures', () => {
  for (const spec of loadSpecDir(specDir)) {
    for (const row of spec.rows) {
      checkInput(row.unescNamed('input'), spec.file + ' ' + row.where())
    }
  }
})

// --- seeded pseudo-random documents -----------------------------------------

/**
 * The fragment pool. ASCII only, every entry newline-terminated, and biased
 * hard toward the corpus blind spots. go/differential_test.go carries this
 * exact list — a change here must land there in the same commit.
 */
const FRAGMENTS: string[] = [
  '# H1\n',
  'para text with *emph* and _under_\n',
  '- item one\n',
  '- [x] task\n',
  '1. ordered\n',
  '> quoted line\n',
  '> a | b\n> --- | ---\n',
  '```\ncode\n```\n',
  'a | b\n--- | ---\nc | d\n',
  '[l](u "t`t")\n',
  '[a](b`c) `d`\n',
  '[not a `link](/foo`)\n',
  '`code` span\n',
  'hard  \nbreak\n',
  'soft \nbreak\n',
  '[ref]: /url "title"\n',
  'use [ref] and [missing] here\n',
  '~~del~~ and ~one~ tilde\n',
  '<div>\nhtml block\n</div>\n',
  'inline <em>html</em> tag\n',
  'escaped \\* star \\| pipe\n',
  '&amp; entity &#65; and &nosuch;\n',
  'bare www.example.com literal\n',
  'scheme https://x.example/z?q=1 literal\n',
  'mail a@b.co literal\n',
  '***\n',
  'Setext\n===\n',
  'Setext two\n---\n',
  '    indented code\n',
  '> [a](b`c) `d`\n',
  '![img](/i.png "alt`tick")\n',
  'tab\there\n',
  '\n',
]

/** xorshift32 — identical in go/differential_test.go. */
function nextRand(state: number): number {
  state ^= (state << 13) >>> 0
  state = state >>> 0
  state ^= state >>> 17
  state ^= (state << 5) >>> 0
  return state >>> 0
}

/** Deterministic document for a seed; Go builds the same one. */
export function fuzzDoc(seed: number): string {
  let state = (seed * 2654435761) >>> 0 || 1
  state = nextRand(state)
  const count = 3 + (state % 12)
  let doc = ''
  for (let i = 0; i < count; i++) {
    state = nextRand(state)
    doc += FRAGMENTS[state % FRAGMENTS.length]
  }
  return doc
}

const FUZZ_DOCS = 200

test('differential: seeded random documents', () => {
  for (let seed = 1; seed <= FUZZ_DOCS; seed++) {
    checkInput(fuzzDoc(seed), 'fuzz seed ' + seed)
  }
})
