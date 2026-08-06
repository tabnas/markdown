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

	decoded := html.UnescapeString(text)
	if decoded == text {
		return text
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

// normalizeReference implements §4.7 label matching: case-insensitive under
// Unicode case folding, with internal whitespace collapsed.
//
// ToLower-then-ToUpper is the same approximation of full case folding the
// reference implementation uses; it makes ẞ/ß and Σ/ς agree.
func normalizeReference(rawLabel string) string {
	trimmed := strings.TrimSpace(rawLabel)
	collapsed := reWhitespaceRun.ReplaceAllString(trimmed, " ")
	return strings.ToUpper(strings.ToLower(collapsed))
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
