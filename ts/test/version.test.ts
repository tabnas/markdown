/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// The exported VERSION must equal package.json "version".
//
// This is the CI check for version drift. It exists because the constant HAS
// drifted: @tabnas/json exported Version = '1.0.0' for several releases while
// the package shipped 0.4.x, because nothing rewrote it and AGENTS.md wrongly
// claimed `make publish-go` kept it in sync. A release that bumps
// package.json and forgets the constant now fails here.
//
// The package root is required, not `../dist/markdown`, so this also proves
// VERSION is reachable as public API through package.json "main".

import { describe, test } from 'node:test'
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

// Read rather than require: a missing or malformed package.json must fail
// loudly here, not be swallowed by a module-resolution error.
const pkgPath = join(__dirname, '..', 'package.json')

function readPkg(): { name: string; version: string } {
  let raw: string
  try {
    raw = readFileSync(pkgPath, 'utf8')
  } catch (err: any) {
    // Deliberately fatal, never skipped: a version check that silently does
    // not run is the failure mode this test exists to prevent.
    assert.fail(`cannot read ts/package.json, so VERSION cannot be checked: ${err.message}`)
  }
  let pkg: any
  try {
    pkg = JSON.parse(raw)
  } catch (err: any) {
    assert.fail(`ts/package.json is not readable JSON: ${err.message}`)
  }
  assert.ok(pkg.version, 'ts/package.json has no version field')
  return pkg
}

const api = require('..')

describe('version', () => {
  test('VERSION matches package.json', () => {
    const pkg = readPkg()
    assert.equal(
      api.VERSION,
      pkg.version,
      `VERSION drift: ${pkg.name} exports ${api.VERSION} but package.json is ` +
        `${pkg.version}. Both are rewritten by admin/publish.sh at release; ` +
        `if you bumped one by hand, bump the other.`,
    )
  })

  test('VERSION is exported and looks like a semver', () => {
    assert.equal(typeof api.VERSION, 'string', 'VERSION must be exported as a string')
    assert.match(api.VERSION, /^\d+\.\d+\.\d+/, 'VERSION must be a semver')
  })
})
