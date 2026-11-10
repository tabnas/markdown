/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// References stay unresolved in the AST, as mdast keeps them (admin ADR-33,
// ruling 6): a bracketed label is a `linkReference` or an `imageReference`
// naming it, whether or not the document defines it, and each definition is
// a `definition` node where it stands. `resolveReferences` gives the tree back
// with the references resolved, as CommonMark reads a document, and this file
// holds it to that reading over both conformance corpora: it gives
// CommonMark's tree for every example but the five registered below, where a
// label with no definition decided how the text around it nests. The
// definitions and references themselves are pinned in every runtime by
// test/spec/reference.tsv; go/references_test.go and
// rs/tests/references_test.rs hold the other runtimes to this file.

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { findSpecDir } from '@tabnas/support'

import { parseDocument, resolveReferences } from '../dist/markdown'
import { parse } from '../dist/commonmark'
import { toAst } from '../dist/ast'
import { resolveOptions } from '../dist/options'

const specDir = findSpecDir(__dirname)

/**
 * The CommonMark examples where resolving the tree's references does not
 * give the tree CommonMark reads, each with why: the declared loss. An
 * example that comes to resolve as CommonMark reads it fails here, and is
 * then deleted.
 */
const RESOLVED_OTHERWISE: Record<number, string> = {
  512: 'the inner [bar] reads as a reference, so the outer link text holds a link and is no link',
  523: 'the [bar* baz] reads as a reference, so the emphasis cannot cross into it',
  528: 'the inner [bar] reads as a reference, so the outer link text holds a link and is no link',
  569: '[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]',
  571: '[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]',
}

/** CommonMark's reading of a document as an AST, without its definitions. */
function commonmarkReading(src: string, opts: Record<string, unknown>): unknown {
  const options = resolveOptions(opts)
  const strip = (node: any): any => {
    if (!Array.isArray(node.children)) return node
    const out: any = {}
    for (const k of Object.keys(node)) {
      out[k] =
        'children' === k
          ? node.children.filter((c: any) => 'definition' !== c.type).map(strip)
          : node[k]
    }
    return out
  }
  return strip(toAst(parse(src, options), options))
}

describe('references', () => {
  test('a defined reference resolves to its link, the first definition winning', () => {
    const doc = parseDocument('[a] ![b][a] [c][]\n\n[a]: /1 "t"\n[A]: /2\n[c]: /3\n')
    assert.deepStrictEqual(resolveReferences(doc as any), {
      type: 'document',
      children: [
        {
          type: 'paragraph',
          children: [
            { type: 'link', url: '/1', title: 't', children: [{ type: 'text', value: 'a' }] },
            { type: 'text', value: ' ' },
            { type: 'image', url: '/1', title: 't', alt: 'b' },
            { type: 'text', value: ' ' },
            { type: 'link', url: '/3', title: null, children: [{ type: 'text', value: 'c' }] },
          ],
        },
      ],
    })
  })

  test('a reference with no definition reads as its source text', () => {
    const doc = parseDocument('x [a] [b][] [c *d*][e] ![f][g] y\n')
    assert.deepStrictEqual(resolveReferences(doc as any), {
      type: 'document',
      children: [
        {
          type: 'paragraph',
          children: [
            { type: 'text', value: 'x [a] [b][] [c ' },
            { type: 'emphasis', children: [{ type: 'text', value: 'd' }] },
            { type: 'text', value: '][e] ![f][g] y' },
          ],
        },
      ],
    })
  })

  test('resolving leaves the tree it was given as it was', () => {
    const doc = parseDocument('[a]\n\n[a]: /u\n')
    const before = JSON.stringify(doc)
    resolveReferences(doc as any)
    assert.equal(JSON.stringify(doc), before)
  })

  test('resolving a deep tree does not recurse', () => {
    const doc = parseDocument('> '.repeat(5000) + '[a]\n\n[a]: /u\n')
    let node: any = resolveReferences(doc as any)
    let depth = 0
    while ('blockquote' === node.children[0].type) {
      node = node.children[0]
      depth++
    }
    assert.equal(depth, 5000)
    assert.equal(node.children[0].children[0].type, 'link')
  })

  for (const [name, opts] of [
    ['commonmark', { gfm: false }],
    ['gfm', {}],
  ] as [string, Record<string, unknown>][]) {
    test(`every ${name} example resolves as CommonMark reads it, but the registered`, () => {
      const cases: { markdown: string; example: number }[] = JSON.parse(
        readFileSync(join(specDir, '..', name, 'spec.json'), 'utf8'),
      )
      const otherwise: number[] = []
      for (const c of cases) {
        const resolved = JSON.stringify(resolveReferences(parseDocument(c.markdown, opts) as any))
        const reading = JSON.stringify(commonmarkReading(c.markdown, opts))
        if (resolved !== reading) otherwise.push(c.example)
      }
      const registered = 'commonmark' === name ? Object.keys(RESOLVED_OTHERWISE).map(Number) : []
      assert.deepStrictEqual(otherwise, registered)
    })
  }
})
