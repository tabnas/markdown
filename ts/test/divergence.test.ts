/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The canonical side of the recorded divergences (../../DIVERGENCE.md).
//
// The Rust port caps container and inline nesting; TypeScript and Go do
// not. `rs/tests/robust_test.rs::nesting_is_capped_at_the_constants`
// pins the Rust column of that table at both bounds, so a cap that moved
// or vanished would fail there. The TypeScript and Go columns carried no
// such assertion: they said "100 block quotes / 101 block quotes" in
// prose, and nothing in any suite would have noticed TypeScript growing
// a cap of its own and quietly turning a recorded divergence into
// agreement.
//
// This file is the TypeScript half of that register, and
// `go/robust_test.go::TestNestingIsUncapped` is the Go half. Each asserts
// the SAME rows of the same table, at the same boundaries, so the
// divergence is executable in all three runtimes rather than in one.
//
// These are deliberately small depths. The point is the boundary the
// table names, not endurance: `rs/tests/robust_test.rs` covers the deep
// end, and a document deep enough to threaten this runtime's own stack
// would test Node rather than the parser.

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { parseTree } from '../dist/markdown'
import type { MdNode } from '../dist/node'

// The deepest run of matching nodes anywhere in the tree, counted over
// every descendant. The Rust test uses the same shape, because past a
// cap the wrappers that do form sit beside the literal text rather than
// above it, and a count of the root's own chain would miss that.
function deepest(node: MdNode, want: (t: string) => boolean): number {
  const here = want(node.type) ? 1 : 0
  let below = 0
  for (let child = node.firstChild; child; child = child.next) {
    const d = deepest(child, want)
    if (d > below) below = d
  }
  return here + below
}

// One predicate per node type, never a union of two. A predicate that
// accepts either of a pair lets one stand in for the other, so a
// regression yielding 100 lists and no items, or 50 `emph` where the
// table records 50 `strong`, would keep these assertions green while the
// cell they claim to pin had changed. Each cell is asserted on its own.
const isQuote = (t: string) => 'block_quote' === t
const isList = (t: string) => 'list' === t
const isItem = (t: string) => 'item' === t
const isEmph = (t: string) => 'emph' === t
const isStrong = (t: string) => 'strong' === t

function depth(src: string, want: (t: string) => boolean): number {
  return deepest(parseTree(src, {}), want)
}

describe('divergence — nesting is uncapped here', () => {
  // DIVERGENCE.md, rows one and two: 100 markers nest 100 block quotes in
  // every runtime, and the 101st is where Rust stops and this one does
  // not.
  test('block quotes nest as deep as the markers go', () => {
    assert.equal(depth('> '.repeat(100) + 'x', isQuote), 100)
    assert.equal(depth('> '.repeat(101) + 'x', isQuote), 101)
    assert.equal(depth('> '.repeat(150) + 'x', isQuote), 150)
  })

  // Rows three and four, which the table states as "N lists and items
  // EACH" -- so each half is asserted on its own. A list marker opens two
  // containers, a list and its item, which is why 50 markers are the 100
  // the Rust cap allows and 51 are the first past it.
  test('list markers nest as deep as the markers go', () => {
    for (const markers of [50, 51]) {
      const src = '- '.repeat(markers) + 'x'
      assert.equal(depth(src, isList), markers)
      assert.equal(depth(src, isItem), markers)
    }
  })

  // Rows five and six. A run of stars pairs up: 2n stars a side nest n
  // `strong` wrappers, so 100 stars are the 50 Rust allows and 102 are
  // the first past it. The table names `strong`, so `strong` is what is
  // counted -- and `emph` is pinned at zero, because a run of paired
  // stars that came back as emphasis would be a different cell.
  test('emphasis nests as deep as the delimiters go', () => {
    const stars = (n: number) => '*'.repeat(n) + 'x' + '*'.repeat(n)
    for (const [side, want] of [[100, 50], [102, 51]] as const) {
      assert.equal(depth(stars(side), isStrong), want)
      assert.equal(depth(stars(side), isEmph), 0)
    }
  })

  // The text a marker past a cap would have carried is never dropped in
  // any runtime; in this one there is no cap for it to fall past, so the
  // document reads back whole.
  test('no marker is silently discarded', () => {
    const tree = parseTree('> '.repeat(101) + 'x', {})
    let node: MdNode | null = tree
    let seen = 0
    while (node) {
      if (isQuote(node.type)) seen++
      node = node.firstChild
    }
    assert.equal(seen, 101)
  })
})
