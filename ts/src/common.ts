/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Shared lexical helpers for the CommonMark parser: the character classes the
// spec names, backslash/entity unescaping (§6.1, §6.2), link-label
// normalisation (§4.7) and the URL + XML escaping the HTML renderer needs.
//
// These are the pieces where "close enough" silently costs conformance
// examples, so each one names the spec rule it implements.

import { ENTITIES } from './entities.ts'

/** ASCII punctuation, per §2.1 — the full set that may follow a backslash. */
export const ESCAPABLE = '[!"#$%&\'()*+,./:;<=>?@\\[\\\\\\]^_`{|}~-]'

/** §6.2: a valid entity is always semicolon-terminated. */
export const ENTITY = '&(?:#x[a-f0-9]{1,6}|#[0-9]{1,7}|[a-z][a-z0-9]{1,31});'

const reBackslashOrAmp = /[\\&]/
const reEntityOrEscapedChar = new RegExp('\\\\' + ESCAPABLE + '|' + ENTITY, 'gi')
const reEntity = new RegExp('^' + ENTITY + '$', 'i')

/**
 * §2.3 Unicode whitespace. Deliberately not `\s`: `\s` includes U+FEFF and
 * excludes some of what the spec counts, and emphasis flanking (§6.4) is
 * decided on exactly this set.
 */
export const UNICODE_WHITESPACE_RE = /[\t\n\f\r    -     　]/

/**
 * §2.1 Unicode punctuation. Used only by the flanking rules. Built from the
 * ASCII punctuation block plus the Unicode P* and S* general categories, which
 * 0.31.2 folded into the definition.
 */
export const UNICODE_PUNCTUATION_RE = /[!-#%-\*,-\/:;\?@\[-\]_\{\}¡§«¶·»¿;·՚-՟։֊־׀׃׆׳״؉؊،؍؛؝-؟٪-٭۔܀-܍߷-߹࠰-࠾࡞।॥॰৽੶૰౷಄෴๏๚๛༄-༒༔༺-༽྅࿐-࿔࿙࿚၊-၏჻፠-፨᐀᙮᚛᚜᛫-᛭᜵᜶។-៖៘-៚᠀-᠊᥄᥅᨞᨟᪠-᪦᪨-᪭᭚-᭠᭽᭾᯼-᯿᰻-᰿᱾᱿᳀-᳇᳓‐-‧‰-⁃⁅-⁑⁓-⁞⁽⁾₍₎⌈-⌋〈〉❨-❵⟅⟆⟦-⟯⦃-⦘⧘-⧛⧼⧽⳹-⳼⳾⳿⵰⸀-⸮⸰-⹏⹒-⹝、-〃〈-】〔-〟〰〽゠・꓾꓿꘍-꘏꙳꙾꛲-꛷꡴-꡷꣎꣏꣸-꣺꣼꤮꤯꥟꧁-꧍꧞꧟꩜-꩟꫞꫟꫰꫱꯫﴾﴿︐-︙︰-﹒﹔-﹡﹣﹨﹪﹫！-＃％-＊，-／：；？＠［-］＿｛｝｟-･]|\ud800[\udd00-\udd02\udf9f\udfd0]|\ud801[\udd6f]|\ud802[\udc57\udd1f\udd3f\ude50-\ude58\ude7f\udef0-\udef6\udf39-\udf3f\udf99-\udf9c]|\ud803[\udf55-\udf59]|\ud804[\udc47-\udc4d\udcbb\udcbc\udcbe-\udcc1\udd40-\udd43\udd74\udd75\uddc5-\uddc9\uddcd\udddb\udddd-\udddf\ude38-\ude3d\udea9]|\ud805[\udc4b-\udc4f\udc5b\udc5d\udcc6\uddc1-\uddd7\ude41-\ude43\ude60-\ude6c\udf3c-\udf3e]|\ud806[\udc3b\udde2\ude3f-\ude46\ude9a-\ude9c\ude9e-\udea2]|\ud807[\udc41-\udc45\udc70\udc71\udef7\udef8\udfff]|\ud809[\udc70-\udc74]|\ud80b[\udff1\udff2]|\ud81a[\ude6e\ude6f\udef5\udf37-\udf3b\udf44]|\ud81b[\ude97-\ude9a\udfe2]|\ud82f[\udc9f]|\ud836[\ude87-\ude8b]|\ud83a[\udd5e\udd5f]/

export function isUnicodeWhitespace(ch: string): boolean {
  return UNICODE_WHITESPACE_RE.test(ch)
}

export function isUnicodePunctuation(ch: string): boolean {
  return UNICODE_PUNCTUATION_RE.test(ch)
}

/**
 * §6.2. Named references resolve from the HTML5 table; numeric ones from the
 * code point, with U+0000 and anything out of range folded to U+FFFD as the
 * spec requires.
 */
export function decodeEntity(text: string): string {
  if (!reEntity.test(text)) return text

  const body = text.slice(1, -1)

  if ('#' === body[0]) {
    const isHex = 'x' === body[1] || 'X' === body[1]
    const digits = isHex ? body.slice(2) : body.slice(1)
    const cp = parseInt(digits, isHex ? 16 : 10)
    if (!isFinite(cp) || 0 === cp || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) {
      return '�'
    }
    return String.fromCodePoint(cp)
  }

  const named = ENTITIES[body]
  return undefined === named ? text : named
}

function unescapeChar(s: string): string {
  // 92 is '\'; anything else that matched is an entity.
  if (92 === s.charCodeAt(0)) return s.charAt(1)
  return decodeEntity(s)
}

/** §6.1 + §6.2 — resolve backslash escapes and entity references together. */
export function unescapeString(s: string): string {
  if (reBackslashOrAmp.test(s)) {
    return s.replace(reEntityOrEscapedChar, unescapeChar)
  }
  return s
}

/**
 * §4.7 link-label matching is case-insensitive under Unicode case folding and
 * collapses internal whitespace. `toLowerCase().toUpperCase()` is the standard
 * approximation of full case folding (it makes ẞ/ß and Σ/ς agree), which is
 * what the reference implementation uses.
 */
export function normalizeReference(rawLabel: string): string {
  return rawLabel
    .trim()
    .replace(/[ \t\r\n]+/g, ' ')
    .toLowerCase()
    .toUpperCase()
}

const XML_ESCAPES: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
}
const reXmlSpecial = /[&<>"]/g

/**
 * HTML output escaping. Applied to text and, identically, to attributes.
 * Unconditional `replace` rather than a `test`-then-`replace` guard: the regex
 * is `/g`, so `test` would advance `lastIndex` and make the function
 * stateful across calls.
 */
export function escapeXml(s: string): string {
  return s.replace(reXmlSpecial, (c) => XML_ESCAPES[c])
}

// URL encoding. The safe set matches the reference renderer's, so hrefs come
// out byte-identical to the spec's expected HTML.
const URL_SAFE = ";/?:@&=+$,-_.!~*'()#"
const reHexPair = /^[0-9a-fA-F]{2}$/

function isUrlSafe(ch: string): boolean {
  return (
    (ch >= '0' && ch <= '9') ||
    (ch >= 'A' && ch <= 'Z') ||
    (ch >= 'a' && ch <= 'z') ||
    -1 !== URL_SAFE.indexOf(ch)
  )
}

/**
 * Percent-encode a destination for output, preserving sequences that are
 * already valid `%XX` triplets so a pre-encoded URL is not double-encoded.
 */
export function normalizeURI(uri: string): string {
  let out = ''
  for (let i = 0; i < uri.length; i++) {
    const ch = uri[i]

    if ('%' === ch && reHexPair.test(uri.slice(i + 1, i + 3))) {
      out += uri.slice(i, i + 3)
      i += 2
      continue
    }

    if (isUrlSafe(ch)) {
      out += ch
      continue
    }

    // Encode the whole code point, so an astral character (two UTF-16 units)
    // becomes one 4-byte UTF-8 sequence rather than two encodings of a lone
    // surrogate. Everything encodeURIComponent leaves alone is already in
    // URL_SAFE and was handled above, so this branch always percent-encodes.
    const cp = uri.codePointAt(i) as number
    if (cp > 0xffff) i++
    try {
      out += encodeURIComponent(String.fromCodePoint(cp))
    } catch {
      // Unpaired surrogate — not representable as UTF-8.
      out += '%EF%BF%BD'
    }
  }
  return out
}
