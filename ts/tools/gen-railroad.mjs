#!/usr/bin/env node

// Regenerate ts/doc/grammar.svg and ts/doc/grammar.txt from the LIVE plugin
// instance.
//
//   cd ts && npm run build && node tools/gen-railroad.mjs
//
// The diagrams are extracted from a real `new Tabnas().use(Markdown)` — the
// same rules the engine parses with — so they are honest by derivation:
// there is no grammar file to fall out of step with the code. (The historical
// diagram was famously empty, a bare track with no boxes, because the grammar
// text it was generated from was inert; see dx-report §8 and §47.)
//
// Requires the engine and @tabnas/railroad installed (dev environment); the
// generated files are committed, so ordinary builds and the conformance
// tools stay engine-free.

import { writeFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const HERE = dirname(fileURLToPath(import.meta.url))
const require = createRequire(join(HERE, '..', 'package.json'))

const { Tabnas } = require('@tabnas/parser')
const { extractGrammar, modelToAscii, modelToSvg } = require('@tabnas/railroad')
const { Markdown } = require(join(HERE, '..', 'dist', 'markdown.js'))
const { makeInlineTn } = require(join(HERE, '..', 'dist', 'engine-inline.js'))
const { resolveOptions } = require(join(HERE, '..', 'dist', 'options.js'))

function render(base, tn) {
  const model = extractGrammar(tn)
  const txt = modelToAscii(model)
  writeFileSync(join(HERE, '..', 'doc', base + '.svg'), modelToSvg(model))
  writeFileSync(join(HERE, '..', 'doc', base + '.txt'), txt.endsWith('\n') ? txt : txt + '\n')
}

// The block instance: the plugin as a caller installs it.
render('grammar', new Tabnas().use(Markdown))
// The inline instance: the nested engine the plugin drives per leaf block.
render('grammar-inline', makeInlineTn(resolveOptions({})))

console.log('wrote ts/doc/grammar{,-inline}.{svg,txt} from live instances')
