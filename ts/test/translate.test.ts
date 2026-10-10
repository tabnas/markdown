/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

import * as assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as path from 'node:path'
import { test } from 'node:test'

const { translate } = require('../dist/markdown')
const root = path.resolve(__dirname, '..', '..')

test('translation parts expose the manifest, sources and explicit entries', () => {
  const parts = translate()
  assert.ok(parts)
  assert.equal(parts.manifest, readFileSync(path.join(root, 'tabnas.plugin.json'), 'utf8'))
  assert.equal(parts.lift?.entry, 'markdown-lift')
  assert.equal(parts.lift?.source, readFileSync(path.join(root, 'alchemy', 'lift.alc'), 'utf8'))
  assert.equal(parts.render?.entry, 'markdown-render')
  assert.equal(parts.render?.source, readFileSync(path.join(root, 'alchemy', 'render.alc'), 'utf8'))
})

// An embed takes a plain tree into a format's own schema. Markdown's
// events carry its own mdast-adjacent tree, but its render writes from
// records, which any tree's rows give, so its manifest names no embed and
// the package carries none; a manifest that named one would be held to its
// file here, as the lift and the render are above.
test('translation parts carry the embed the manifest names, and none where it names none', () => {
  const parts = translate()
  const spec = JSON.parse(readFileSync(path.join(root, 'tabnas.plugin.json'), 'utf8')).translate
  if (null == spec.embed) {
    assert.equal(parts.embed, undefined)
  } else {
    assert.equal(parts.embed?.entry, 'markdown-embed')
    assert.equal(parts.embed?.source, readFileSync(path.join(root, spec.embed), 'utf8'))
  }
})
