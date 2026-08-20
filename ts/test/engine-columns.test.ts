/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Token COLUMNS after a non-ASCII character, in both engine phases.
//
// This port is the canonical one and its answers were already right: `pnt.cI
// += text.length` and `pnt.cI += consumed.length` count UTF-16 units, which
// for every character below U+10000 is the character count the column
// contract asks for. The Go half wrote the same two expressions over BYTE
// lengths, so a 2-byte `é` cost two columns and a 3-byte `€` three, in the
// block phase (engine-block.ts / engineblock.go) and the inline phase
// (engine-inline.ts / engineinline.go) alike.
//
// This file is the other leg of the runtime-alignment rule: the invariant is
// pinned independently in each runtime, so the repaired Go half cannot drift
// back while the parity suites stay green — and neither can this half drift
// away from it. go/enginecol_test.go asserts the same two tables.
//
// The astral rows are the only ones where the answers differ, and that is
// the recorded engine divergence — this port counts UTF-16 units (an astral
// character is 2), Go counts runes (1). See parser/DIVERGENCE.md, "Column
// positions for astral characters".

import { test } from 'node:test'
import assert from 'node:assert'

import { Tabnas, makeLex } from '@tabnas/parser'
import { Markdown } from '../dist/markdown'
import { makeInlineTn } from '../dist/engine-inline'
import { InlineParser } from '../dist/inline'
import { MdNode } from '../dist/node'
import { resolveOptions } from '../dist/options'

// lexColumns drives a token stream to its end and renders each token as
// `name@row:col`, which is the whole observable surface of the column
// arithmetic: neither phase puts these positions in the AST, so a pin that
// went through parse() would assert nothing about them.
function lexColumns(lex: any): string {
  const out: string[] = []
  for (let i = 0; i < 32; i++) {
    const t = lex.next()
    if (!t) break
    out.push(t.name + '@' + t.rI + ':' + t.cI)
    if ('#ZZ' === t.name || '#BD' === t.name) break
  }
  return out.join(' ')
}

test('block columns count characters, not bytes', () => {
  // Every case omits the trailing newline on purpose. A source that ends in
  // one takes the terminated branch, which resets cI to 1 and hides the
  // arithmetic — the reason this defect was first written off on the Go side
  // as having no diagnostic surface.
  const cases: [string, string, string][] = [
    // Controls. Pure ASCII, and a source that DOES end in a newline: without
    // them, "columns count characters" is also satisfied by never counting.
    ['ascii-nl', '# xx\n', '#LB@1:1 #ZZ@2:1'],
    ['ascii-nonl', '# xx', '#LB@1:1 #ZZ@1:5'],

    // 2 and 3 bytes, 1 UTF-16 unit, 1 rune: both ports agree.
    ['latin1', '# é', '#LB@1:1 #ZZ@1:4'],
    ['bmp', '# €', '#LB@1:1 #ZZ@1:4'],
    ['para-latin1', 'é text', '#LB@1:1 #ZZ@1:7'],
    ['para-bmp', '€€ text', '#LB@1:1 #ZZ@1:8'],
    ['emph-latin1', '*é* x', '#LB@1:1 #ZZ@1:6'],
    ['link-bmp', '[€](u) x', '#LB@1:1 #ZZ@1:9'],
    ['code-latin1', '`é` x', '#LB@1:1 #ZZ@1:6'],
    ['html-latin1', '<b>é</b> x', '#LB@1:1 #ZZ@1:11'],

    // A terminated line followed by an unterminated one: the row advances on
    // the first and the column counts on the second.
    ['multiline-latin1', 'é a\nb c', '#LB@1:1 #LB@2:1 #ZZ@2:4'],

    // 4 bytes, TWO UTF-16 units, 1 rune: the recorded divergence, and the
    // only block row where the two halves differ. Go asserts `#ZZ@1:4`.
    ['astral', '# \u{1F600}', '#LB@1:1 #ZZ@1:5'],
  ]
  for (const [label, src, want] of cases) {
    const j: any = new Tabnas().use(Markdown)
    const lex = makeLex({ src: () => src, cfg: j.internal().config, opts: j.options, sub: {} } as any)
    assert.equal(lexColumns(lex), want, label + ': ' + JSON.stringify(src))
  }
})

test('inline columns count characters, not bytes', () => {
  // The inline phase is a second engine instance over one block's content,
  // and its positions never reach the AST — parseInlinesEngine discards
  // them. Driving its lexer directly is the level at which the arithmetic is
  // observable at all, so it is the level the pin asserts.
  const opts = resolveOptions({})
  const cases: [string, string, string][] = [
    // Control: pure ASCII.
    ['ascii', 'a b c', '#ITX@1:1 #ZZ@1:6'],

    // 2 and 3 bytes, 1 UTF-16 unit, 1 rune: both ports agree.
    ['latin1', 'é text', '#ITX@1:1 #ZZ@1:7'],
    ['bmp', '€€ text', '#ITX@1:1 #ZZ@1:8'],
    ['emph-latin1', '*é* x', '#IDL@1:1 #ITX@1:2 #IDL@1:3 #ITX@1:4 #ZZ@1:6'],
    ['code-latin1', '`é` x', '#ICS@1:1 #ITX@1:4 #ZZ@1:6'],
    ['link-bmp', '[€](u) x', '#IOB@1:1 #ITX@1:2 #ICB@1:3 #ITX@1:7 #ZZ@1:9'],

    // The `0 < newlines` branch, which resets the column against the last
    // line ending rather than accumulating. A multiline code span is the
    // shortest input that reaches it; without these two rows that branch has
    // no reproduction and the pin proves nothing about half the change.
    ['codespan-multiline-latin1', '`é\né` tail é', '#ICS@1:1 #ITX@2:3 #ZZ@2:10'],
    ['codespan-multiline-bmp', '`€€\nx` tail €', '#ICS@1:1 #ITX@2:3 #ZZ@2:10'],

    // 4 bytes, TWO UTF-16 units, 1 rune: the recorded divergence, on both
    // branches. Go asserts `#ZZ@1:7` and `#IBK@1:4 ... #ZZ@2:4`.
    ['astral', '\u{1F600} text', '#ITX@1:1 #ZZ@1:8'],
    ['softbreak-astral', '\u{1F600} a\n\u{1F600} b', '#ITX@1:1 #IBK@1:5 #ITX@2:1 #ZZ@2:5'],
  ]
  for (const [label, subject, want] of cases) {
    const tn: any = makeInlineTn(opts)
    const p = new InlineParser({}, opts)
    p.subject = subject
    p.pos = 0
    const ctx = {
      src: () => subject,
      cfg: tn.internal().config,
      opts: tn.options,
      sub: {},
      u: { inl: { parser: p, block: new MdNode('paragraph') } },
    }
    assert.equal(lexColumns(makeLex(ctx as any)), want, label + ': ' + JSON.stringify(subject))
  }
})
