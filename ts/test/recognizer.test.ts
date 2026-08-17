/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Pin tests for the pure recognizers exported by block.ts and inline.ts.
//
// The recognizers are the single home of each construct's recognition logic:
// the hand drivers delegate to them, and any other driver over the same
// syntax (an engine lexer adapter, a differential harness) is expected to
// share them rather than reimplement them. These cases pin the exact result
// shapes so a refactor that changes one is a red test, not a silent drift.
// `go/recognizer_test.go` pins the same cases against the Go port.

import { test } from 'node:test'
import { deepStrictEqual as eq, strictEqual as is } from 'node:assert'

import {
  segmentNextLine,
  isBlank,
  isThematicBreakLine,
  matchAtxHeading,
  stripAtxClosing,
  matchCodeFence,
  matchClosingCodeFence,
  setextHeadingLevel,
  htmlBlockOpenKind,
  htmlBlockCloses,
  matchBulletListMarker,
  matchOrderedListMarker,
  matchTaskListMarker,
  parseDelimiterRow,
  splitTableRow,
} from '../dist/block'

import {
  scanCodeSpan,
  scanEscape,
  scanAngleAutolink,
  scanHtmlTag,
  scanEntity,
  scanDelimiterRun,
  classifyBreak,
  scanLinkTitle,
  scanLinkDestination,
  scanLinkLabel,
  scanInlineLinkTail,
  matchSpnl,
  skipInitialSpaces,
  codePointBefore,
  codePointAt,
} from '../dist/inline'

// --- block.ts ---------------------------------------------------------------

test('segmentNextLine cuts physical lines', () => {
  eq(segmentNextLine('a\nb', 0), { text: 'a', next: 2 })
  eq(segmentNextLine('a\nb', 2), { text: 'b', next: 3 })
  is(segmentNextLine('a\nb', 3), null)
  // A final line ending does not introduce a trailing blank line.
  eq(segmentNextLine('a\n', 0), { text: 'a', next: 2 })
  is(segmentNextLine('a\n', 2), null)
  // \r\n is one terminator; a lone \r is one too.
  eq(segmentNextLine('a\r\nb', 0), { text: 'a', next: 3 })
  eq(segmentNextLine('a\rb', 0), { text: 'a', next: 2 })
  // Blank line between terminators.
  eq(segmentNextLine('a\n\nb', 2), { text: '', next: 3 })
  // §2.3: NUL is replaced with U+FFFD.
  eq(segmentNextLine('a\u0000b', 0), { text: 'a\uFFFDb', next: 3 })
  // Empty source has no lines at all (the driver special-cases it).
  is(segmentNextLine('', 0), null)
})

test('isBlank', () => {
  is(isBlank(''), true)
  is(isBlank(' \t '), true)
  is(isBlank(' x'), false)
})

test('isThematicBreakLine', () => {
  is(isThematicBreakLine('***'), true)
  is(isThematicBreakLine('- - -'), true)
  is(isThematicBreakLine('_ _ _ _'), true)
  is(isThematicBreakLine('**'), false)
  is(isThematicBreakLine('*-*'), false)
})

test('matchAtxHeading', () => {
  eq(matchAtxHeading('# Hello'), { marker: '# ', level: 1 })
  eq(matchAtxHeading('###### x'), { marker: '###### ', level: 6 })
  eq(matchAtxHeading('##'), { marker: '##', level: 2 })
  is(matchAtxHeading('####### x'), null)
  is(matchAtxHeading('#x'), null)
})

test('stripAtxClosing', () => {
  is(stripAtxClosing('Hello ###'), 'Hello')
  is(stripAtxClosing('###'), '')
  is(stripAtxClosing('Hello #x'), 'Hello #x')
})

test('matchCodeFence', () => {
  is(matchCodeFence('```js'), '```')
  is(matchCodeFence('~~~~'), '~~~~')
  // A backtick info string may not contain a backtick.
  is(matchCodeFence('``` a`b'), null)
  is(matchCodeFence('``'), null)
})

test('matchClosingCodeFence', () => {
  is(matchClosingCodeFence('```'), '```')
  is(matchClosingCodeFence('````  '), '````')
  is(matchClosingCodeFence('``` x'), null)
})

test('setextHeadingLevel', () => {
  is(setextHeadingLevel('==='), 1)
  is(setextHeadingLevel('-  '), 2)
  is(setextHeadingLevel('=-'), 0)
  is(setextHeadingLevel('x'), 0)
})

test('htmlBlockOpenKind', () => {
  is(htmlBlockOpenKind('<script>'), 1)
  is(htmlBlockOpenKind('<!-- c -->'), 2)
  is(htmlBlockOpenKind('<?php'), 3)
  is(htmlBlockOpenKind('<!DOCTYPE html>'), 4)
  is(htmlBlockOpenKind('<![CDATA[x'), 5)
  is(htmlBlockOpenKind('<div>'), 6)
  is(htmlBlockOpenKind('<x-tag>'), 7)
  is(htmlBlockOpenKind('plain'), 0)
})

test('htmlBlockCloses', () => {
  is(htmlBlockCloses('x</script>y', 1), true)
  is(htmlBlockCloses('x -->', 2), true)
  is(htmlBlockCloses('x', 2), false)
  is(htmlBlockCloses('?>', 3), true)
  is(htmlBlockCloses('>', 4), true)
  is(htmlBlockCloses(']]>', 5), true)
})

test('list markers', () => {
  eq(matchBulletListMarker('- x'), { marker: '-', char: '-' })
  eq(matchBulletListMarker('+ x'), { marker: '+', char: '+' })
  is(matchBulletListMarker('x'), null)
  eq(matchOrderedListMarker('1. x'), { marker: '1.', start: 1, delimiter: '.' })
  eq(matchOrderedListMarker('123456789) x'), { marker: '123456789)', start: 123456789, delimiter: ')' })
  // §5.2: at most nine digits.
  is(matchOrderedListMarker('1234567890. x'), null)
})

test('matchTaskListMarker', () => {
  eq(matchTaskListMarker('[x] done'), { checked: true, length: 4 })
  eq(matchTaskListMarker('[ ] todo'), { checked: false, length: 4 })
  // The trailing whitespace is required: `[x]` alone is ordinary text.
  is(matchTaskListMarker('[x]'), null)
  is(matchTaskListMarker('[y] no'), null)
})

test('parseDelimiterRow and splitTableRow', () => {
  eq(parseDelimiterRow('| :-- | --: | :-: | --- |'), ['left', 'right', 'center', null])
  is(parseDelimiterRow('| a |'), null)
  eq(splitTableRow('| a | b |'), ['a', 'b'])
  // `\|` is a literal pipe, resolved at split time.
  eq(splitTableRow('a \\| b'), ['a | b'])
})

// --- inline.ts --------------------------------------------------------------

test('scanCodeSpan', () => {
  eq(scanCodeSpan('`a`', 0), { closed: true, end: 3, literal: 'a' })
  // One leading and one trailing space strip together.
  eq(scanCodeSpan('` `` `', 0), { closed: true, end: 6, literal: '``' })
  // Unequal runs leave the opener literal.
  eq(scanCodeSpan('``a`', 0), { closed: false, end: 2, ticks: '``' })
  is(scanCodeSpan('a`', 0), null)
})

test('scanEscape', () => {
  eq(scanEscape('\\*', 0), { kind: 'char', literal: '*', end: 2 })
  eq(scanEscape('\\a', 0), { kind: 'literal', literal: '\\', end: 1 })
  eq(scanEscape('\\\n  x', 0), { kind: 'linebreak', literal: '', end: 4 })
  eq(scanEscape('\\', 0), { kind: 'literal', literal: '\\', end: 1 })
})

test('scanAngleAutolink', () => {
  eq(scanAngleAutolink('<http://x.y>', 0), { dest: 'http://x.y', label: 'http://x.y', end: 12 })
  eq(scanAngleAutolink('<a@b.co>', 0), { dest: 'mailto:a@b.co', label: 'a@b.co', end: 8 })
  is(scanAngleAutolink('<not a link>', 0), null)
})

test('scanHtmlTag and scanEntity', () => {
  eq(scanHtmlTag('<em class="x">', 0), { raw: '<em class="x">', end: 14 })
  is(scanHtmlTag('<1bad>', 0), null)
  eq(scanEntity('&amp;', 0), { literal: '&', end: 5 })
  eq(scanEntity('&#65;', 0), { literal: 'A', end: 5 })
  // An unknown named reference matches the entity shape but decodes to
  // itself — CommonMark leaves it as literal text.
  eq(scanEntity('&nosuch;', 0), { literal: '&nosuch;', end: 8 })
  is(scanEntity('&;', 0), null)
})

test('scanDelimiterRun', () => {
  eq(scanDelimiterRun('*a*', 0, 42), { numdelims: 1, canOpen: true, canClose: false })
  eq(scanDelimiterRun('*a*', 2, 42), { numdelims: 1, canOpen: false, canClose: true })
  // `_` may not open inside a word: snake_case stays intact.
  eq(scanDelimiterRun('snake_case', 5, 95), { numdelims: 1, canOpen: false, canClose: false })
  is(scanDelimiterRun('ab', 0, 42), null)
})

test('classifyBreak', () => {
  eq(classifyBreak('foo  '), { hard: true, trim: true })
  eq(classifyBreak('foo '), { hard: false, trim: true })
  eq(classifyBreak('foo'), { hard: false, trim: false })
  eq(classifyBreak(null), { hard: false, trim: false })
})

test('scanLinkTitle and scanLinkDestination', () => {
  eq(scanLinkTitle('"t\\"x"', 0), { title: 't"x', end: 6 })
  is(scanLinkTitle('x', 0), null)
  eq(scanLinkDestination('<u v>', 0), { dest: 'u v', end: 5 })
  eq(scanLinkDestination('/a(b)c d', 0), { dest: '/a(b)c', end: 6 })
  // Unbalanced parens are not a destination.
  is(scanLinkDestination('a(b', 0), null)
})

test('scanLinkLabel', () => {
  eq(scanLinkLabel('[ref]:', 0), { rawLength: 5, length: 5 })
  is(scanLinkLabel('[a[b]', 0), null)
  // Over the §4.7 limit: consumed but invalid.
  const long = '[' + 'x'.repeat(1000) + ']'
  eq(scanLinkLabel(long, 0), { rawLength: 1002, length: 0 })
})

test('scanInlineLinkTail', () => {
  eq(scanInlineLinkTail('(/u "t")', 0), { dest: '/u', title: 't', end: 8 })
  eq(scanInlineLinkTail('(/u)', 0), { dest: '/u', title: null, end: 4 })
  // A title must be separated from the destination by whitespace.
  eq(scanInlineLinkTail('(/u"t")', 0), { dest: '/u"t"', title: null, end: 7 })
  is(scanInlineLinkTail('(/u', 0), null)
  is(scanInlineLinkTail('x', 0), null)
})

test('position helpers', () => {
  is(skipInitialSpaces('  x', 0), 2)
  is(matchSpnl('  \n  x', 0), 5)
  // At most one line ending.
  is(matchSpnl('\n\nx', 0), 1)
  is(codePointBefore('a𝄞', 3), '𝄞')
  is(codePointBefore('x', 0), '\n')
  is(codePointAt('𝄞a', 0), '𝄞')
  is(codePointAt('x', 1), '\n')
})
