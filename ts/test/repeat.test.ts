/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Every repetition is a replace loop, never a push chain (alchemy's
// AGENTS.md, "The reader repeats by replacement").
//
// A container's children are the `children` rule, which goes on by
// replacement (`r`) from one child to the next, so a document of ten
// thousand blocks, or a list of ten thousand items, or a paragraph of ten
// thousand inline nodes, runs at the rule depth one needs. Real nesting
// still nests, as it should: a block quote inside a block quote is a level
// deeper. The rule depth is the engine's `d`, read by a rule subscriber on
// every step of the parse. rs/tests/repeat_test.rs and go/repeat_test.go
// hold the other runtimes to the same.

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { make } from '../dist/markdown'

/** The deepest rule a parse of `src` reaches. */
function maxDepth(src: string, opts?: Record<string, unknown>): number {
  let max = 0
  const tn: any = make(opts)
  tn.sub({
    rule: (rule: any) => {
      if (rule.d > max) max = rule.d
    },
  })
  tn.parse(src)
  return max
}

const N = 10000

/** Each construct, one of it, and many of it: the same depth. */
const REPEATED: [string, string, (n: number) => string][] = [
  ['paragraphs', 'a\n', (n) => 'a\n\n'.repeat(n)],
  ['ATX headings', '# h\n', (n) => '# h\n'.repeat(n)],
  ['setext headings', 'h\n=\n', (n) => 'h\n=\n\n'.repeat(n)],
  ['thematic breaks', '***\n', (n) => '***\n'.repeat(n)],
  ['fenced code blocks', '```\nx\n```\n', (n) => '```\nx\n```\n'.repeat(n)],
  ['indented code blocks', '    x\n\na\n', (n) => '    x\n\na\n\n'.repeat(n)],
  ['HTML blocks', '<div>\n</div>\n', (n) => '<div>\n</div>\n\n'.repeat(n)],
  ['definitions', '[a]: /u\n', (n) => '[a]: /u\n'.repeat(n)],
  ['list items', '- a\n', (n) => '- a\n'.repeat(n)],
  ['loose list items', '- a\n', (n) => '- a\n\n'.repeat(n)],
  ['ordered list items', '1. a\n', (n) => '1. a\n'.repeat(n)],
  ['paragraphs in a block quote', '> a\n', (n) => '> a\n>\n'.repeat(n)],
  ['lines of a paragraph', 'a\n', (n) => 'a\n'.repeat(n)],
  ['lines of a code block', '```\nx\n```\n', (n) => '```\n' + 'x\n'.repeat(n) + '```\n'],
  ['table rows', '| a |\n| - |\n| b |\n', (n) => '| a |\n| - |\n' + '| b |\n'.repeat(n)],
  ['table cells', '| a |\n| - |\n', (n) => '| a '.repeat(n) + '|\n' + '| - '.repeat(n) + '|\n'],
  ['emphasis in a paragraph', '*a*\n', (n) => '*a* '.repeat(n) + '\n'],
  ['links in a paragraph', '[a](/u)\n', (n) => '[a](/u) '.repeat(n) + '\n'],
  ['references in a paragraph', '[a]\n', (n) => '[a] '.repeat(n) + '\n'],
  ['code spans in a paragraph', '`a`\n', (n) => '`a` '.repeat(n) + '\n'],
  ['hard breaks in a paragraph', 'a  \nb\n', (n) => 'a  \n'.repeat(n) + 'b\n'],
]

describe('repeat', () => {
  for (const [what, one, many] of REPEATED) {
    test(`ten thousand ${what} reach the depth one does`, () => {
      const d1 = maxDepth(one)
      const dn = maxDepth(many(N))
      assert.ok(0 < d1)
      assert.equal(dn, d1, `${what}: one reaches ${d1}, ${N} reach ${dn}`)
    })
  }

  test('nesting nests: each block quote is a level deeper', () => {
    // A block quote is a rule and its children's loop, two levels a quote.
    const d = (n: number) => maxDepth('> '.repeat(n) + 'a\n')
    assert.equal(d(2) - d(1), 2)
    assert.equal(d(100) - d(1), 2 * 99)
  })

  test('nesting nests: each emphasis is a level deeper', () => {
    const d = (n: number) => maxDepth('*'.repeat(n) + 'a' + '*'.repeat(n) + '\n')
    // `***a***` is emphasis in strong: three delimiters, two nodes.
    assert.ok(d(3) > d(1))
  })
})
