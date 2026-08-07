/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Shared lexical helpers. Port of ts/src/common.ts — the character classes the
// spec names, backslash/entity unescaping (§6.1, §6.2), link-label
// normalisation (§4.7), and the URL + XML escaping the HTML renderer needs.
//
// Two places differ from the TypeScript deliberately, both because Go has a
// better tool for the job:
//
//   - Entity decoding uses the standard library's HTML5 table rather than a
//     vendored copy, gated so that only semicolon-terminated references decode
//     (stdlib also accepts legacy forms like `&auml`, which §6.2 does not).
//   - Unicode punctuation uses unicode.IsPunct/IsSymbol rather than a regexp.

import (
	"html"
	"regexp"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

// escapableASCII is the full ASCII punctuation set of §2.1 — everything that
// may follow a backslash.
const escapableASCII = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"

// entityPattern matches a §6.2 character reference. Always semicolon-terminated.
const entityPattern = `&(?:#[xX][a-fA-F0-9]{1,6}|#[0-9]{1,7}|[a-zA-Z][a-zA-Z0-9]{1,31});`

var (
	reEntityOrEscapedChar = regexp.MustCompile(`\\[` + regexp.QuoteMeta(escapableASCII) + `]|` + entityPattern)
	reEntityWhole         = regexp.MustCompile(`^(?:` + entityPattern + `)$`)
	reWhitespaceRun       = regexp.MustCompile(`[ \t\r\n]+`)
)

// missingEntities are the WHATWG named references that the standard library's
// table does not carry. §6.2 takes the whole WHATWG list, and ts/src/entities.ts
// (2125 semicolon-terminated entries, generated from entities.json) has them,
// so without this the two runtimes disagree on `&nGt;` and `&nLt;`.
//
// These two are the complete set of gaps: TestEntityTableMatchesTypeScript
// checks every one of the 2125 names.
var missingEntities = map[string]string{
	"nGt": "≫⃒",
	"nLt": "≪⃒",
}

// IsEscapable reports whether b is ASCII punctuation, i.e. may follow a
// backslash (§6.1).
func IsEscapable(b byte) bool {
	return strings.IndexByte(escapableASCII, b) >= 0
}

// isUnicodeWhitespace implements §2.3. Deliberately not unicode.IsSpace:
// that set differs from the spec's at the edges, and emphasis flanking (§6.4)
// is decided on exactly this set.
func isUnicodeWhitespace(r rune) bool {
	switch r {
	case '\t', '\n', '\f', '\r', ' ':
		return true
	}
	// Zs, plus the line/paragraph separators the spec lists explicitly.
	switch r {
	case 0x00A0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return r >= 0x2000 && r <= 0x200A
}

// isUnicodePunctuation implements §2.1. 0.31.2 widened the definition from the
// P* categories alone to P* ∪ S*, which is why `$ + < = > ^ ` | ~ £ €` count.
// A P-only test is exactly the near-miss that costs emphasis-flanking examples.
func isUnicodePunctuation(r rune) bool {
	return unicode.IsPunct(r) || unicode.IsSymbol(r)
}

// decodeEntity resolves one character reference (§6.2), returning the input
// unchanged if it is not a valid one.
//
// Numeric references fold U+0000, out-of-range values and surrogates to
// U+FFFD. Named references come from the stdlib HTML5 table, but only in the
// semicolon-terminated form: html.UnescapeString also accepts legacy
// semicolon-less aliases such as `&auml`, which CommonMark does not.
func decodeEntity(text string) string {
	if !reEntityWhole.MatchString(text) {
		return text
	}

	body := text[1 : len(text)-1]

	if body[0] == '#' {
		isHex := len(body) > 1 && (body[1] == 'x' || body[1] == 'X')
		digits := body[1:]
		base := 10
		if isHex {
			digits = body[2:]
			base = 16
		}
		cp, err := strconv.ParseInt(digits, base, 64)
		if err != nil || cp == 0 || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF) {
			return "�"
		}
		return string(rune(cp))
	}

	if r, ok := missingEntities[body]; ok {
		return r
	}

	decoded := html.UnescapeString(text)
	if decoded == text {
		return text
	}

	// html.UnescapeString honours the legacy semicolon-less aliases, and it
	// honours them as a *prefix* of a longer name: `&ampa;` comes back as
	// `&amp` followed by the leftover `a;`, and `&nbspa;` as U+00A0 followed
	// by `a;`. §6.2 admits only the exact semicolon-terminated names, so a
	// decode that left part of the reference behind did not resolve one at
	// all — `&ampa;` is literal text, which is what the TypeScript's explicit
	// 2125-entry table gives.
	//
	// The residue is always a non-empty suffix of the name plus the `;`.
	// `body` is capped at 32 characters by entityPattern, so this is a
	// bounded scan. No genuine replacement can look like residue: only
	// `&semi;` decodes to a string ending in `;`, and a one-character result
	// cannot end with two or more characters of its own name.
	for k := 1; k < len(body); k++ {
		if strings.HasSuffix(decoded, body[k:]+";") {
			return text
		}
	}

	return decoded
}

// unescapeString resolves backslash escapes and entity references together
// (§6.1 + §6.2).
func unescapeString(s string) string {
	if !strings.ContainsAny(s, "\\&") {
		return s
	}
	return reEntityOrEscapedChar.ReplaceAllStringFunc(s, func(m string) string {
		if m[0] == '\\' {
			return m[1:]
		}
		return decodeEntity(m)
	})
}

// isJSSpace reports whether r is JavaScript whitespace: the ECMAScript
// WhiteSpace and LineTerminator productions, which are both what `\s` matches
// and what String.prototype.trim() removes. That is the spec's §2.3 Unicode
// whitespace plus the vertical tab and U+FEFF.
//
// Deliberately distinct from isUnicodeWhitespace, which is §2.3 exactly and
// answers a different question (emphasis flanking). And deliberately used in
// place of strings.TrimSpace / regexp `\s` wherever the canonical runtime uses
// a JavaScript trim or `\s`: TrimSpace trims U+0085, which JavaScript keeps,
// and keeps U+FEFF, which JavaScript trims, while Go's regexp `\s` is only
// [\t\n\f\r ]. Four separate places turn on exactly this set — the fence info
// string (block.go), the trailing-space test that decides a hard line break
// (inline.go), the code fence's language/meta split (ast.go and html.go), and
// §4.7 label matching (normalizeReference below) — so it is defined once.
func isJSSpace(r rune) bool {
	return '\v' == r || '\uFEFF' == r || isUnicodeWhitespace(r)
}

// jsTrim removes exactly what String.prototype.trim() removes.
func jsTrim(s string) string { return strings.TrimFunc(s, isJSSpace) }

// jsSpaceIndex is the byte index of the first JavaScript-whitespace character
// in s, with its width, or (-1, 0) when there is none. It is the Go spelling of
// `s.search(/\s/)`, except that the index is a byte offset and the width comes
// back too — JavaScript can advance past the match with `+ 1` because every
// character in the class is BMP, where Go must step a whole rune.
func jsSpaceIndex(s string) (idx int, width int) {
	for i := 0; i < len(s); {
		// Scan by byte: no character in the class above U+007F can be confused
		// with a UTF-8 continuation byte, and slicing at i never splits a rune.
		if c := s[i]; c < utf8.RuneSelf {
			if isJSSpace(rune(c)) {
				return i, 1
			}
			i++
			continue
		}
		r, size := utf8.DecodeRuneInString(s[i:])
		if isJSSpace(r) {
			return i, size
		}
		i += size
	}
	return -1, 0
}

// jsFullCaseUpper maps the code points whose Unicode **full** case round trip
// (lowercase, then uppercase) differs from the **simple**, one-rune-in
// one-rune-out mapping that strings.ToLower/strings.ToUpper apply.
//
// This is the whole reason normalizeReference cannot just be
// strings.ToUpper(strings.ToLower(s)). JavaScript's toLowerCase/toUpperCase
// implement the full mappings — the unconditional entries of Unicode's
// SpecialCasing.txt — so "\u00df".toUpperCase() is "SS", where Go's ToUpper
// leaves ß alone. CommonMark example 540 ([ẞ] resolving against [SS]) turns on
// exactly that, and 103 further code points (mostly Greek Extended
// iota-subscript forms) diverge the same way.
//
// Keyed on the *original* rune and holding the result of the whole round trip,
// which is well defined because in the root locale both halves are pure
// per-code-point mappings. The one context-sensitive rule, Final_Sigma, washes
// out here: σ and ς both uppercase to Σ.
//
// Generated from the mappings of the canonical runtime, then verified
// exhaustively against it over every code point (see normalizeReference).
var jsFullCaseUpper = map[rune]string{
	// Latin sharp s
	0x00DF: "\u0053\u0053", // LATIN SMALL LETTER SHARP S

	// Latin
	0x0130: "\u0049\u0307", // LATIN CAPITAL LETTER I WITH DOT ABOVE
	0x0149: "\u02bc\u004e", // LATIN SMALL LETTER N PRECEDED BY APOSTROPHE
	0x01F0: "\u004a\u030c", // LATIN SMALL LETTER J WITH CARON

	// Greek
	0x0390: "\u0399\u0308\u0301", // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND TONOS
	0x03B0: "\u03a5\u0308\u0301", // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND TONOS

	// Armenian
	0x0587: "\u0535\u0552", // ARMENIAN SMALL LIGATURE ECH YIWN

	// Latin Extended Additional
	0x1E96: "\u0048\u0331", // LATIN SMALL LETTER H WITH LINE BELOW
	0x1E97: "\u0054\u0308", // LATIN SMALL LETTER T WITH DIAERESIS
	0x1E98: "\u0057\u030a", // LATIN SMALL LETTER W WITH RING ABOVE
	0x1E99: "\u0059\u030a", // LATIN SMALL LETTER Y WITH RING ABOVE
	0x1E9A: "\u0041\u02be", // LATIN SMALL LETTER A WITH RIGHT HALF RING

	// Latin sharp s
	0x1E9E: "\u0053\u0053", // LATIN CAPITAL LETTER SHARP S

	// Greek Extended
	0x1F50: "\u03a5\u0313",       // GREEK SMALL LETTER UPSILON WITH PSILI
	0x1F52: "\u03a5\u0313\u0300", // GREEK SMALL LETTER UPSILON WITH PSILI AND VARIA
	0x1F54: "\u03a5\u0313\u0301", // GREEK SMALL LETTER UPSILON WITH PSILI AND OXIA
	0x1F56: "\u03a5\u0313\u0342", // GREEK SMALL LETTER UPSILON WITH PSILI AND PERISPOMENI
	0x1F80: "\u1f08\u0399",       // GREEK SMALL LETTER ALPHA WITH PSILI AND YPOGEGRAMMENI
	0x1F81: "\u1f09\u0399",       // GREEK SMALL LETTER ALPHA WITH DASIA AND YPOGEGRAMMENI
	0x1F82: "\u1f0a\u0399",       // GREEK SMALL LETTER ALPHA WITH PSILI AND VARIA AND YPOGEGRAMMENI
	0x1F83: "\u1f0b\u0399",       // GREEK SMALL LETTER ALPHA WITH DASIA AND VARIA AND YPOGEGRAMMENI
	0x1F84: "\u1f0c\u0399",       // GREEK SMALL LETTER ALPHA WITH PSILI AND OXIA AND YPOGEGRAMMENI
	0x1F85: "\u1f0d\u0399",       // GREEK SMALL LETTER ALPHA WITH DASIA AND OXIA AND YPOGEGRAMMENI
	0x1F86: "\u1f0e\u0399",       // GREEK SMALL LETTER ALPHA WITH PSILI AND PERISPOMENI AND YPOGEGRAMMENI
	0x1F87: "\u1f0f\u0399",       // GREEK SMALL LETTER ALPHA WITH DASIA AND PERISPOMENI AND YPOGEGRAMMENI
	0x1F88: "\u1f08\u0399",       // GREEK CAPITAL LETTER ALPHA WITH PSILI AND PROSGEGRAMMENI
	0x1F89: "\u1f09\u0399",       // GREEK CAPITAL LETTER ALPHA WITH DASIA AND PROSGEGRAMMENI
	0x1F8A: "\u1f0a\u0399",       // GREEK CAPITAL LETTER ALPHA WITH PSILI AND VARIA AND PROSGEGRAMMENI
	0x1F8B: "\u1f0b\u0399",       // GREEK CAPITAL LETTER ALPHA WITH DASIA AND VARIA AND PROSGEGRAMMENI
	0x1F8C: "\u1f0c\u0399",       // GREEK CAPITAL LETTER ALPHA WITH PSILI AND OXIA AND PROSGEGRAMMENI
	0x1F8D: "\u1f0d\u0399",       // GREEK CAPITAL LETTER ALPHA WITH DASIA AND OXIA AND PROSGEGRAMMENI
	0x1F8E: "\u1f0e\u0399",       // GREEK CAPITAL LETTER ALPHA WITH PSILI AND PERISPOMENI AND PROSGEGRAMMENI
	0x1F8F: "\u1f0f\u0399",       // GREEK CAPITAL LETTER ALPHA WITH DASIA AND PERISPOMENI AND PROSGEGRAMMENI
	0x1F90: "\u1f28\u0399",       // GREEK SMALL LETTER ETA WITH PSILI AND YPOGEGRAMMENI
	0x1F91: "\u1f29\u0399",       // GREEK SMALL LETTER ETA WITH DASIA AND YPOGEGRAMMENI
	0x1F92: "\u1f2a\u0399",       // GREEK SMALL LETTER ETA WITH PSILI AND VARIA AND YPOGEGRAMMENI
	0x1F93: "\u1f2b\u0399",       // GREEK SMALL LETTER ETA WITH DASIA AND VARIA AND YPOGEGRAMMENI
	0x1F94: "\u1f2c\u0399",       // GREEK SMALL LETTER ETA WITH PSILI AND OXIA AND YPOGEGRAMMENI
	0x1F95: "\u1f2d\u0399",       // GREEK SMALL LETTER ETA WITH DASIA AND OXIA AND YPOGEGRAMMENI
	0x1F96: "\u1f2e\u0399",       // GREEK SMALL LETTER ETA WITH PSILI AND PERISPOMENI AND YPOGEGRAMMENI
	0x1F97: "\u1f2f\u0399",       // GREEK SMALL LETTER ETA WITH DASIA AND PERISPOMENI AND YPOGEGRAMMENI
	0x1F98: "\u1f28\u0399",       // GREEK CAPITAL LETTER ETA WITH PSILI AND PROSGEGRAMMENI
	0x1F99: "\u1f29\u0399",       // GREEK CAPITAL LETTER ETA WITH DASIA AND PROSGEGRAMMENI
	0x1F9A: "\u1f2a\u0399",       // GREEK CAPITAL LETTER ETA WITH PSILI AND VARIA AND PROSGEGRAMMENI
	0x1F9B: "\u1f2b\u0399",       // GREEK CAPITAL LETTER ETA WITH DASIA AND VARIA AND PROSGEGRAMMENI
	0x1F9C: "\u1f2c\u0399",       // GREEK CAPITAL LETTER ETA WITH PSILI AND OXIA AND PROSGEGRAMMENI
	0x1F9D: "\u1f2d\u0399",       // GREEK CAPITAL LETTER ETA WITH DASIA AND OXIA AND PROSGEGRAMMENI
	0x1F9E: "\u1f2e\u0399",       // GREEK CAPITAL LETTER ETA WITH PSILI AND PERISPOMENI AND PROSGEGRAMMENI
	0x1F9F: "\u1f2f\u0399",       // GREEK CAPITAL LETTER ETA WITH DASIA AND PERISPOMENI AND PROSGEGRAMMENI
	0x1FA0: "\u1f68\u0399",       // GREEK SMALL LETTER OMEGA WITH PSILI AND YPOGEGRAMMENI
	0x1FA1: "\u1f69\u0399",       // GREEK SMALL LETTER OMEGA WITH DASIA AND YPOGEGRAMMENI
	0x1FA2: "\u1f6a\u0399",       // GREEK SMALL LETTER OMEGA WITH PSILI AND VARIA AND YPOGEGRAMMENI
	0x1FA3: "\u1f6b\u0399",       // GREEK SMALL LETTER OMEGA WITH DASIA AND VARIA AND YPOGEGRAMMENI
	0x1FA4: "\u1f6c\u0399",       // GREEK SMALL LETTER OMEGA WITH PSILI AND OXIA AND YPOGEGRAMMENI
	0x1FA5: "\u1f6d\u0399",       // GREEK SMALL LETTER OMEGA WITH DASIA AND OXIA AND YPOGEGRAMMENI
	0x1FA6: "\u1f6e\u0399",       // GREEK SMALL LETTER OMEGA WITH PSILI AND PERISPOMENI AND YPOGEGRAMMENI
	0x1FA7: "\u1f6f\u0399",       // GREEK SMALL LETTER OMEGA WITH DASIA AND PERISPOMENI AND YPOGEGRAMMENI
	0x1FA8: "\u1f68\u0399",       // GREEK CAPITAL LETTER OMEGA WITH PSILI AND PROSGEGRAMMENI
	0x1FA9: "\u1f69\u0399",       // GREEK CAPITAL LETTER OMEGA WITH DASIA AND PROSGEGRAMMENI
	0x1FAA: "\u1f6a\u0399",       // GREEK CAPITAL LETTER OMEGA WITH PSILI AND VARIA AND PROSGEGRAMMENI
	0x1FAB: "\u1f6b\u0399",       // GREEK CAPITAL LETTER OMEGA WITH DASIA AND VARIA AND PROSGEGRAMMENI
	0x1FAC: "\u1f6c\u0399",       // GREEK CAPITAL LETTER OMEGA WITH PSILI AND OXIA AND PROSGEGRAMMENI
	0x1FAD: "\u1f6d\u0399",       // GREEK CAPITAL LETTER OMEGA WITH DASIA AND OXIA AND PROSGEGRAMMENI
	0x1FAE: "\u1f6e\u0399",       // GREEK CAPITAL LETTER OMEGA WITH PSILI AND PERISPOMENI AND PROSGEGRAMMENI
	0x1FAF: "\u1f6f\u0399",       // GREEK CAPITAL LETTER OMEGA WITH DASIA AND PERISPOMENI AND PROSGEGRAMMENI
	0x1FB2: "\u1fba\u0399",       // GREEK SMALL LETTER ALPHA WITH VARIA AND YPOGEGRAMMENI
	0x1FB3: "\u0391\u0399",       // GREEK SMALL LETTER ALPHA WITH YPOGEGRAMMENI
	0x1FB4: "\u0386\u0399",       // GREEK SMALL LETTER ALPHA WITH OXIA AND YPOGEGRAMMENI
	0x1FB6: "\u0391\u0342",       // GREEK SMALL LETTER ALPHA WITH PERISPOMENI
	0x1FB7: "\u0391\u0342\u0399", // GREEK SMALL LETTER ALPHA WITH PERISPOMENI AND YPOGEGRAMMENI
	0x1FBC: "\u0391\u0399",       // GREEK CAPITAL LETTER ALPHA WITH PROSGEGRAMMENI
	0x1FC2: "\u1fca\u0399",       // GREEK SMALL LETTER ETA WITH VARIA AND YPOGEGRAMMENI
	0x1FC3: "\u0397\u0399",       // GREEK SMALL LETTER ETA WITH YPOGEGRAMMENI
	0x1FC4: "\u0389\u0399",       // GREEK SMALL LETTER ETA WITH OXIA AND YPOGEGRAMMENI
	0x1FC6: "\u0397\u0342",       // GREEK SMALL LETTER ETA WITH PERISPOMENI
	0x1FC7: "\u0397\u0342\u0399", // GREEK SMALL LETTER ETA WITH PERISPOMENI AND YPOGEGRAMMENI
	0x1FCC: "\u0397\u0399",       // GREEK CAPITAL LETTER ETA WITH PROSGEGRAMMENI
	0x1FD2: "\u0399\u0308\u0300", // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND VARIA
	0x1FD3: "\u0399\u0308\u0301", // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND OXIA
	0x1FD6: "\u0399\u0342",       // GREEK SMALL LETTER IOTA WITH PERISPOMENI
	0x1FD7: "\u0399\u0308\u0342", // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND PERISPOMENI
	0x1FE2: "\u03a5\u0308\u0300", // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND VARIA
	0x1FE3: "\u03a5\u0308\u0301", // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND OXIA
	0x1FE4: "\u03a1\u0313",       // GREEK SMALL LETTER RHO WITH PSILI
	0x1FE6: "\u03a5\u0342",       // GREEK SMALL LETTER UPSILON WITH PERISPOMENI
	0x1FE7: "\u03a5\u0308\u0342", // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND PERISPOMENI
	0x1FF2: "\u1ffa\u0399",       // GREEK SMALL LETTER OMEGA WITH VARIA AND YPOGEGRAMMENI
	0x1FF3: "\u03a9\u0399",       // GREEK SMALL LETTER OMEGA WITH YPOGEGRAMMENI
	0x1FF4: "\u038f\u0399",       // GREEK SMALL LETTER OMEGA WITH OXIA AND YPOGEGRAMMENI
	0x1FF6: "\u03a9\u0342",       // GREEK SMALL LETTER OMEGA WITH PERISPOMENI
	0x1FF7: "\u03a9\u0342\u0399", // GREEK SMALL LETTER OMEGA WITH PERISPOMENI AND YPOGEGRAMMENI
	0x1FFC: "\u03a9\u0399",       // GREEK CAPITAL LETTER OMEGA WITH PROSGEGRAMMENI

	// Alphabetic Presentation Forms
	0xFB00: "\u0046\u0046",       // LATIN SMALL LIGATURE FF
	0xFB01: "\u0046\u0049",       // LATIN SMALL LIGATURE FI
	0xFB02: "\u0046\u004c",       // LATIN SMALL LIGATURE FL
	0xFB03: "\u0046\u0046\u0049", // LATIN SMALL LIGATURE FFI
	0xFB04: "\u0046\u0046\u004c", // LATIN SMALL LIGATURE FFL
	0xFB05: "\u0053\u0054",       // LATIN SMALL LIGATURE LONG S T
	0xFB06: "\u0053\u0054",       // LATIN SMALL LIGATURE ST
	0xFB13: "\u0544\u0546",       // ARMENIAN SMALL LIGATURE MEN NOW
	0xFB14: "\u0544\u0535",       // ARMENIAN SMALL LIGATURE MEN ECH
	0xFB15: "\u0544\u053b",       // ARMENIAN SMALL LIGATURE MEN INI
	0xFB16: "\u054e\u0546",       // ARMENIAN SMALL LIGATURE VEW NOW
	0xFB17: "\u0544\u053d",       // ARMENIAN SMALL LIGATURE MEN XEH
}

// normalizeReference implements §4.7 label matching: case-insensitive under
// Unicode case folding, with internal whitespace collapsed.
//
// ToLower-then-ToUpper is the approximation of full case folding the reference
// implementation uses; it makes ẞ/ß and Σ/ς agree. Go's mappings are simple
// rather than full, so the round trip goes through jsFullCaseUpper.
//
// Known gap: Go's unicode tables are Unicode 15.0 where the canonical runtime's
// are newer, so 55 code points added to Unicode since then (Garay, Ol Onal and
// a few Latin/Cyrillic additions) have casing pairs Go does not know and will
// not match case-insensitively. That is a data-version difference, not a
// mapping-model one; no spec example or fixture reaches it.
func normalizeReference(rawLabel string) string {
	trimmed := jsTrim(rawLabel)
	collapsed := reWhitespaceRun.ReplaceAllString(trimmed, " ")

	// Fast path. For ASCII, lowercasing then uppercasing is just uppercasing,
	// and no ASCII code point is in jsFullCaseUpper.
	if isASCII(collapsed) {
		return strings.ToUpper(collapsed)
	}

	var b strings.Builder
	b.Grow(len(collapsed))
	for _, r := range collapsed {
		if full, ok := jsFullCaseUpper[r]; ok {
			b.WriteString(full)
			continue
		}
		b.WriteRune(unicode.ToUpper(unicode.ToLower(r)))
	}
	return b.String()
}

func isASCII(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] >= utf8.RuneSelf {
			return false
		}
	}
	return true
}

var xmlEscaper = strings.NewReplacer(
	"&", "&amp;",
	"<", "&lt;",
	">", "&gt;",
	`"`, "&quot;",
)

// escapeXML is the HTML output escaping, applied to text and, identically, to
// attribute values.
func escapeXML(s string) string { return xmlEscaper.Replace(s) }

// urlSafe is the set left unencoded by normalizeURI, matching the reference
// renderer so hrefs come out byte-identical to the spec's expected HTML.
const urlSafe = ";/?:@&=+$,-_.!~*'()#"

func isURLSafe(b byte) bool {
	switch {
	case b >= '0' && b <= '9':
		return true
	case b >= 'A' && b <= 'Z':
		return true
	case b >= 'a' && b <= 'z':
		return true
	}
	return strings.IndexByte(urlSafe, b) >= 0
}

func isHexDigit(b byte) bool {
	return (b >= '0' && b <= '9') || (b >= 'a' && b <= 'f') || (b >= 'A' && b <= 'F')
}

const upperHex = "0123456789ABCDEF"

// normalizeURI percent-encodes a destination for output, preserving sequences
// that are already valid %XX triplets so a pre-encoded URL is not
// double-encoded.
func normalizeURI(uri string) string {
	var b strings.Builder
	b.Grow(len(uri))

	for i := 0; i < len(uri); {
		c := uri[i]

		if c == '%' && i+2 < len(uri) && isHexDigit(uri[i+1]) && isHexDigit(uri[i+2]) {
			b.WriteString(uri[i : i+3])
			i += 3
			continue
		}

		if c < utf8.RuneSelf {
			if isURLSafe(c) {
				b.WriteByte(c)
			} else {
				b.WriteByte('%')
				b.WriteByte(upperHex[c>>4])
				b.WriteByte(upperHex[c&0x0F])
			}
			i++
			continue
		}

		// Multi-byte: percent-encode each UTF-8 byte of the rune. Invalid
		// bytes decode to RuneError with size 1, which encodes as U+FFFD —
		// the same substitution the spec makes for unrepresentable input.
		r, size := utf8.DecodeRuneInString(uri[i:])
		if r == utf8.RuneError && size == 1 {
			b.WriteString("%EF%BF%BD")
			i++
			continue
		}
		for _, bb := range []byte(uri[i : i+size]) {
			b.WriteByte('%')
			b.WriteByte(upperHex[bb>>4])
			b.WriteByte(upperHex[bb&0x0F])
		}
		i += size
	}

	return b.String()
}
