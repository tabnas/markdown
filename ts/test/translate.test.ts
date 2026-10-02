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
