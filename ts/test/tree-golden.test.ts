/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Golden native-tree snapshots, shared with the Go runtime.
//
// The `test/spec/*.tsv` fixtures compare public ASTs, and the projection
// drops `sourcepos` — so a positional divergence between the runtimes is
// invisible to them. This suite pins the NATIVE tree instead: every fixture
// input is parsed with `parseTree` and serialized to a canonical JSON shape
// (see `serializeTree`), and the result is compared byte-for-values against
// `test/spec/tree/<name>.json`. `go/tree_golden_test.go` asserts the same
// files with the same serializer, so the two runtimes cannot drift on
// anything the native tree carries — sourcepos included — without one of
// them going red.
//
// Regenerate after an intentional tree change (TypeScript is canonical):
//
//   cd ts && npm run build && MD_TREE_GOLDEN=write npm test
//
// and re-run the Go suite to confirm the port agrees.

import { test } from 'node:test'
import assert from 'node:assert'
import * as fs from 'node:fs'
import * as path from 'node:path'

import { findSpecDir, loadSpecDir } from '@tabnas/support'

import { parseTree } from '../dist/markdown'
import type { MdNode } from '../dist/markdown'

const WRITE = 'write' === process.env.MD_TREE_GOLDEN

// Node types whose `literal` is meaningful (always a string there). Kept as
// an explicit list because Go cannot distinguish an absent literal from an
// empty one — the serializers include the field for exactly these types.
const LITERAL_TYPES: Record<string, true> = {
  text: true,
  code: true,
  html_inline: true,
  code_block: true,
  html_block: true,
}

/**
 * The canonical serialization both runtimes produce. Field-for-field it
 * carries: `type` and `sourcepos` always; `literal` for the types above;
 * `level` for headings; `destination` (and `title` when present) for links
 * and images; fence data for fenced code; `listData`, `tableAlign`,
 * `isHeaderRow`, `checked` where set; `gfm` on the document node. Internal
 * bookkeeping (`stringContent`, `open`, `lastLineBlank`) is deliberately
 * absent.
 */
function serializeTree(node: MdNode): Record<string, unknown> {
  const out: Record<string, unknown> = { type: node.type, sourcepos: node.sourcepos }

  if (true === LITERAL_TYPES[node.type]) out.literal = node.literal
  if ('heading' === node.type) out.level = node.level
  if ('link' === node.type || 'image' === node.type) {
    out.destination = node.destination
    if (null !== node.title) out.title = node.title
  }
  if ('code_block' === node.type) {
    out.isFenced = node.isFenced
    if (node.isFenced) {
      out.info = node.info
      out.fenceChar = node.fenceChar
      out.fenceLength = node.fenceLength
      out.fenceOffset = node.fenceOffset
    }
  }
  if (null !== node.listData) out.listData = { ...node.listData }
  if (null !== node.tableAlign) out.tableAlign = node.tableAlign
  if (node.isHeaderRow) out.isHeaderRow = true
  if (null !== node.checked) out.checked = node.checked
  if ('document' === node.type) out.gfm = node.gfm

  const children: unknown[] = []
  for (let c = node.firstChild; null !== c; c = c.next) children.push(serializeTree(c))
  if (0 < children.length) out.children = children

  return out
}

const specDir = findSpecDir(__dirname)
const treeDir = path.join(specDir, 'tree')

for (const spec of loadSpecDir(specDir)) {
  test('tree goldens: ' + spec.file, () => {
    const cases = spec.rows.map((row) => {
      const optsRaw = row.named('opts')
      const opts = '' === optsRaw.trim() ? {} : JSON.parse(optsRaw)
      return {
        // The input is stored in its escaped fixture form so the golden
        // files stay single-line; both runtimes unescape with the shared
        // fixture codec before parsing.
        input: row.named('input'),
        opts: optsRaw,
        tree: serializeTree(parseTree(row.unescNamed('input'), opts)),
      }
    })

    const goldenPath = path.join(treeDir, spec.file.replace(/\.tsv$/, '.json'))

    if (WRITE) {
      fs.mkdirSync(treeDir, { recursive: true })
      fs.writeFileSync(goldenPath, JSON.stringify(cases, null, 1) + '\n')
      return
    }

    const golden = JSON.parse(fs.readFileSync(goldenPath, 'utf8'))
    assert.deepStrictEqual(JSON.parse(JSON.stringify(cases)), golden)
  })
}
