// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// recognizer_test.go — pin tests for the pure recognizers in block.go and
// inline.go. The recognizers are the single home of each construct's
// recognition logic: the hand drivers delegate to them, and any other driver
// over the same syntax is expected to share them rather than reimplement
// them. ts/test/recognizer.test.ts pins the same cases against the canonical
// TypeScript; the shapes differ where Go idiom differs (multiple returns for
// TS object results), the behavior may not.

import (
	"reflect"
	"testing"
)

func eqAny(t *testing.T, got, want any, what string) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Errorf("%s: got %#v, want %#v", what, got, want)
	}
}

func TestSegmentNextLine(t *testing.T) {
	type seg struct {
		text string
		next int
		ok   bool
	}
	cut := func(src string, pos int) seg {
		text, next, ok := segmentNextLine(src, pos)
		return seg{text, next, ok}
	}
	eqAny(t, cut("a\nb", 0), seg{"a", 2, true}, "LF line")
	eqAny(t, cut("a\nb", 2), seg{"b", 3, true}, "last line")
	eqAny(t, cut("a\nb", 3), seg{"", 3, false}, "past end")
	// A final line ending does not introduce a trailing blank line.
	eqAny(t, cut("a\n", 0), seg{"a", 2, true}, "final newline")
	eqAny(t, cut("a\n", 2), seg{"", 2, false}, "after final newline")
	// \r\n is one terminator; a lone \r is one too.
	eqAny(t, cut("a\r\nb", 0), seg{"a", 3, true}, "CRLF")
	eqAny(t, cut("a\rb", 0), seg{"a", 2, true}, "CR")
	// Blank line between terminators.
	eqAny(t, cut("a\n\nb", 2), seg{"", 3, true}, "blank line")
	// §2.3: NUL is replaced with U+FFFD.
	eqAny(t, cut("a\x00b", 0), seg{"a�b", 3, true}, "NUL replacement")
	// Empty source has no lines at all (the driver special-cases it).
	eqAny(t, cut("", 0), seg{"", 0, false}, "empty source")
}

func TestBlockLineRecognizers(t *testing.T) {
	if !isBlank("") || !isBlank(" \t ") || isBlank(" x") {
		t.Error("isBlank")
	}

	if !isThematicBreak("***") || !isThematicBreak("- - -") || !isThematicBreak("_ _ _ _") {
		t.Error("isThematicBreak accepts")
	}
	if isThematicBreak("**") || isThematicBreak("*-*") {
		t.Error("isThematicBreak rejects")
	}

	if level, n := matchATXHeadingMarker("# Hello"); level != 1 || n != 2 {
		t.Errorf("atx '# Hello': %d %d", level, n)
	}
	if level, n := matchATXHeadingMarker("###### x"); level != 6 || n != 7 {
		t.Errorf("atx six: %d %d", level, n)
	}
	if level, n := matchATXHeadingMarker("##"); level != 2 || n != 2 {
		t.Errorf("atx bare: %d %d", level, n)
	}
	if level, _ := matchATXHeadingMarker("####### x"); level != 0 {
		t.Error("atx seven")
	}
	if level, _ := matchATXHeadingMarker("#x"); level != 0 {
		t.Error("atx no space")
	}

	if matchCodeFence("```js") != 3 || matchCodeFence("~~~~") != 4 {
		t.Error("matchCodeFence accepts")
	}
	// A backtick info string may not contain a backtick.
	if matchCodeFence("``` a`b") != 0 || matchCodeFence("``") != 0 {
		t.Error("matchCodeFence rejects")
	}

	if matchClosingCodeFence("```") != 3 || matchClosingCodeFence("````  ") != 4 {
		t.Error("matchClosingCodeFence accepts")
	}
	if matchClosingCodeFence("``` x") != 0 {
		t.Error("matchClosingCodeFence rejects")
	}

	if matchSetextHeadingLine("===") != '=' || matchSetextHeadingLine("-  ") != '-' {
		t.Error("setext accepts")
	}
	if matchSetextHeadingLine("=-") != 0 || matchSetextHeadingLine("x") != 0 {
		t.Error("setext rejects")
	}

	kinds := map[string]int{
		"<script>":        1,
		"<!-- c -->":      2,
		"<?php":           3,
		"<!DOCTYPE html>": 4,
		"<![CDATA[x":      5,
		"<div>":           6,
		"<x-tag>":         7,
		"plain":           0,
	}
	for s, want := range kinds {
		if got := htmlBlockOpenKind(s); got != want {
			t.Errorf("htmlBlockOpenKind(%q) = %d, want %d", s, got, want)
		}
	}

	if !htmlBlockCloses("x</script>y", 1) || !htmlBlockCloses("x -->", 2) ||
		!htmlBlockCloses("?>", 3) || !htmlBlockCloses(">", 4) || !htmlBlockCloses("]]>", 5) {
		t.Error("htmlBlockCloses accepts")
	}
	if htmlBlockCloses("x", 2) {
		t.Error("htmlBlockCloses rejects")
	}

	if !isBulletListMarker("- x") || !isBulletListMarker("+ x") || isBulletListMarker("x") {
		t.Error("isBulletListMarker")
	}
	if d, delim := matchOrderedListMarker("1. x"); d != 1 || delim != '.' {
		t.Error("ordered 1.")
	}
	if d, delim := matchOrderedListMarker("123456789) x"); d != 9 || delim != ')' {
		t.Error("ordered nine digits")
	}
	// §5.2: at most nine digits.
	if d, _ := matchOrderedListMarker("1234567890. x"); d != 0 {
		t.Error("ordered ten digits")
	}

	if state, length, ok := taskListMarker([]byte("[x] done")); !ok || state != 'x' || length != 4 {
		t.Error("task marker checked")
	}
	if state, length, ok := taskListMarker([]byte("[ ] todo")); !ok || state != ' ' || length != 4 {
		t.Error("task marker unchecked")
	}
	// The trailing whitespace is required: `[x]` alone is ordinary text.
	if _, _, ok := taskListMarker([]byte("[x]")); ok {
		t.Error("task marker bare")
	}
	if _, _, ok := taskListMarker([]byte("[y] no")); ok {
		t.Error("task marker bad state")
	}

	if got := parseDelimiterRow("| :-- | --: | :-: | --- |"); !reflect.DeepEqual(
		got, []TableAlign{AlignLeft, AlignRight, AlignCenter, AlignNone}) {
		t.Errorf("parseDelimiterRow: %#v", got)
	}
	if parseDelimiterRow("| a |") != nil {
		t.Error("parseDelimiterRow rejects")
	}
	eqAny(t, splitTableRow("| a | b |"), []string{"a", "b"}, "splitTableRow")
	// `\|` is a literal pipe, resolved at split time.
	eqAny(t, splitTableRow(`a \| b`), []string{"a | b"}, "splitTableRow escape")
}

func TestInlineRecognizers(t *testing.T) {
	eqAny(t, scanCodeSpan("`a`", 0), &codeSpanScan{closed: true, end: 3, literal: "a"}, "code span")
	// One leading and one trailing space strip together.
	eqAny(t, scanCodeSpan("` `` `", 0), &codeSpanScan{closed: true, end: 6, literal: "``"}, "code span strip")
	// Unequal runs leave the opener literal.
	eqAny(t, scanCodeSpan("``a`", 0), &codeSpanScan{closed: false, end: 2, ticks: "``"}, "code span open")
	if scanCodeSpan("a`", 0) != nil {
		t.Error("code span none")
	}

	eqAny(t, scanEscape(`\*`, 0), escapeScan{kind: "char", literal: "*", end: 2}, "escape char")
	eqAny(t, scanEscape(`\a`, 0), escapeScan{kind: "literal", literal: `\`, end: 1}, "escape literal")
	eqAny(t, scanEscape("\\\n  x", 0), escapeScan{kind: "linebreak", literal: "", end: 4}, "escape linebreak")
	eqAny(t, scanEscape(`\`, 0), escapeScan{kind: "literal", literal: `\`, end: 1}, "escape trailing")

	eqAny(t, scanAngleAutolink("<http://x.y>", 0),
		&angleAutolinkScan{dest: "http://x.y", label: "http://x.y", end: 12}, "uri autolink")
	eqAny(t, scanAngleAutolink("<a@b.co>", 0),
		&angleAutolinkScan{dest: "mailto:a@b.co", label: "a@b.co", end: 8}, "email autolink")
	if scanAngleAutolink("<not a link>", 0) != nil {
		t.Error("autolink none")
	}

	if raw, end, ok := scanHtmlTag(`<em class="x">`, 0); !ok || raw != `<em class="x">` || end != 14 {
		t.Error("html tag")
	}
	if _, _, ok := scanHtmlTag("<1bad>", 0); ok {
		t.Error("html tag none")
	}
	if literal, end, ok := scanEntity("&amp;", 0); !ok || literal != "&" || end != 5 {
		t.Error("entity named")
	}
	if literal, end, ok := scanEntity("&#65;", 0); !ok || literal != "A" || end != 5 {
		t.Error("entity numeric")
	}
	// An unknown named reference matches the entity shape but decodes to
	// itself — CommonMark leaves it as literal text.
	if literal, end, ok := scanEntity("&nosuch;", 0); !ok || literal != "&nosuch;" || end != 8 {
		t.Error("entity unknown")
	}
	if _, _, ok := scanEntity("&;", 0); ok {
		t.Error("entity none")
	}

	eqAny(t, scanDelimiterRun("*a*", 0, '*'),
		&delimScan{numdelims: 1, canOpen: true, canClose: false}, "delim open")
	eqAny(t, scanDelimiterRun("*a*", 2, '*'),
		&delimScan{numdelims: 1, canOpen: false, canClose: true}, "delim close")
	// `_` may not open inside a word: snake_case stays intact.
	eqAny(t, scanDelimiterRun("snake_case", 5, '_'),
		&delimScan{numdelims: 1, canOpen: false, canClose: false}, "delim snake")
	if scanDelimiterRun("ab", 0, '*') != nil {
		t.Error("delim none")
	}

	type brk struct{ hard, trim bool }
	classify := func(s string, has bool) brk {
		hard, trim := classifyBreak(s, has)
		return brk{hard, trim}
	}
	eqAny(t, classify("foo  ", true), brk{true, true}, "hard break")
	eqAny(t, classify("foo ", true), brk{false, true}, "soft trim")
	eqAny(t, classify("foo", true), brk{false, false}, "soft plain")
	eqAny(t, classify("", false), brk{false, false}, "no prev")

	if title, end, ok := scanLinkTitle(`"t\"x"`, 0); !ok || title != `t"x` || end != 6 {
		t.Error("link title")
	}
	if _, _, ok := scanLinkTitle("x", 0); ok {
		t.Error("link title none")
	}
	if dest, end, ok := scanLinkDestination("<u v>", 0); !ok || dest != "u v" || end != 5 {
		t.Error("braced destination")
	}
	if dest, end, ok := scanLinkDestination("/a(b)c d", 0); !ok || dest != "/a(b)c" || end != 6 {
		t.Error("bare destination")
	}
	// Unbalanced parens are not a destination.
	if _, _, ok := scanLinkDestination("a(b", 0); ok {
		t.Error("destination unbalanced")
	}

	if raw, length, ok := scanLinkLabel("[ref]:", 0); !ok || raw != 5 || length != 5 {
		t.Error("link label")
	}
	if _, _, ok := scanLinkLabel("[a[b]", 0); ok {
		t.Error("link label nested")
	}
	// Over the §4.7 limit: consumed but invalid.
	long := "["
	for i := 0; i < 1000; i++ {
		long += "x"
	}
	long += "]"
	if raw, length, ok := scanLinkLabel(long, 0); !ok || raw != 1002 || length != 0 {
		t.Error("link label too long")
	}

	eqAny(t, scanInlineLinkTail(`(/u "t")`, 0),
		&linkTailScan{dest: "/u", title: "t", hasTitle: true, end: 8}, "tail titled")
	eqAny(t, scanInlineLinkTail("(/u)", 0),
		&linkTailScan{dest: "/u", title: "", hasTitle: false, end: 4}, "tail bare")
	// A title must be separated from the destination by whitespace.
	eqAny(t, scanInlineLinkTail(`(/u"t")`, 0),
		&linkTailScan{dest: `/u"t"`, title: "", hasTitle: false, end: 7}, "tail glued")
	if scanInlineLinkTail("(/u", 0) != nil || scanInlineLinkTail("x", 0) != nil {
		t.Error("tail none")
	}

	if skipInitialSpaces("  x", 0) != 2 {
		t.Error("skipInitialSpaces")
	}
	if matchSpnl("  \n  x", 0) != 5 {
		t.Error("matchSpnl")
	}
	// At most one line ending.
	if matchSpnl("\n\nx", 0) != 1 {
		t.Error("matchSpnl two endings")
	}
	if codePointBefore("a\U0001D11E", 5) != '\U0001D11E' || codePointBefore("x", 0) != '\n' {
		t.Error("codePointBefore")
	}
	if codePointAt("\U0001D11Ea", 0) != '\U0001D11E' || codePointAt("x", 1) != '\n' {
		t.Error("codePointAt")
	}
}
