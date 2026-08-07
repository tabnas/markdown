/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Phase 2 of the CommonMark parse (spec 0.31.2, Appendix A, "Phase 2: inline
// structure"). The block phase left the raw text of every paragraph and
// heading in `stringContent`; this module turns that text into inline
// children and clears it.
//
// The shape is the spec's own algorithm: one left-to-right scan over the
// subject that appends nodes as it goes, plus two auxiliary stacks that are
// resolved out of band, because neither construct can be decided at the
// point where its opener is seen:
//
//   * the delimiter stack (§6.4) holds `*`/`_` (and, under GFM, `~`) runs,
//     and is resolved by `processEmphasis`;
//   * the bracket stack (§6.3) holds `[` and `![`, and is resolved by
//     `parseCloseBracket` when a `]` turns up.
//
// Everything else — code spans, autolinks, raw HTML, entities, escapes,
// breaks — is decided locally at the scan position. That is what produces
// the precedence §6.3 requires: a code span, autolink or raw HTML tag is
// consumed whole during the scan, so it binds more tightly than emphasis and
// more tightly than the brackets of a link label. Conversely brackets bind
// more tightly than emphasis, because bracket matching happens during the
// scan while emphasis matching happens afterwards over the delimiter stack.

import { MdNode } from './node.ts'
import type { NodeType } from './node.ts'
import type { ParserOptions, RefMap } from './options.ts'
import {
  ENTITY,
  ESCAPABLE,
  decodeEntity,
  isUnicodePunctuation,
  isUnicodeWhitespace,
  normalizeReference,
  unescapeString,
} from './common.ts'

const C_TAB = 9
const C_NEWLINE = 10
const C_VTAB = 11
const C_FORMFEED = 12
const C_CARRIAGE_RETURN = 13
const C_SPACE = 32
const C_BANG = 33
const C_AMPERSAND = 38
const C_OPEN_PAREN = 40
const C_CLOSE_PAREN = 41
const C_ASTERISK = 42
const C_PLUS = 43
const C_COMMA = 44
const C_HYPHEN = 45
const C_PERIOD = 46
const C_COLON = 58
const C_SEMICOLON = 59
const C_LESSTHAN = 60
const C_QUESTION = 63
const C_AT = 64
const C_OPEN_BRACKET = 91
const C_BACKSLASH = 92
const C_CLOSE_BRACKET = 93
const C_UNDERSCORE = 95
const C_BACKTICK = 96
const C_TILDE = 126
const C_DEL = 127

// --- §6.6 Raw HTML: the tag grammar, spelled out because it is stricter than
// `<[^>]*>` in every part. Attribute names are XML-restricted-to-ASCII;
// unquoted attribute values exclude quotes, `=`, `<`, `>`, backtick and all
// control characters; comments/PIs/declarations/CDATA each have their own
// terminator. `\s*` stands in for "spaces, tabs, and up to one line ending":
// a paragraph can never contain a blank line, so a single `\s*` can only ever
// span one line ending anyway.

const TAGNAME = '[A-Za-z][A-Za-z0-9-]*'
const ATTRIBUTENAME = '[a-zA-Z_:][a-zA-Z0-9:._-]*'
const UNQUOTEDVALUE = "[^\"'=<>`\\x00-\\x20]+"
const SINGLEQUOTEDVALUE = "'[^']*'"
const DOUBLEQUOTEDVALUE = '"[^"]*"'
const ATTRIBUTEVALUE =
  '(?:' + UNQUOTEDVALUE + '|' + SINGLEQUOTEDVALUE + '|' + DOUBLEQUOTEDVALUE + ')'
const ATTRIBUTEVALUESPEC = '(?:\\s*=\\s*' + ATTRIBUTEVALUE + ')'
const ATTRIBUTE = '(?:\\s+' + ATTRIBUTENAME + ATTRIBUTEVALUESPEC + '?)'
const OPENTAG = '<' + TAGNAME + ATTRIBUTE + '*\\s*/?>'
const CLOSETAG = '</' + TAGNAME + '\\s*>'
// §6.6: `<!-->` and `<!--->` are comments in their own right; otherwise the
// body simply may not contain `-->`, which the lazy quantifier enforces.
const HTMLCOMMENT = '<!-->|<!--->|<!--[\\s\\S]*?-->'
const PROCESSINGINSTRUCTION = '[<][?][\\s\\S]*?[?][>]'
const DECLARATION = '<![A-Za-z][^>]*>'
const CDATA = '<!\\[CDATA\\[[\\s\\S]*?\\]\\]>'
const HTMLTAG =
  '(?:' +
  OPENTAG +
  '|' +
  CLOSETAG +
  '|' +
  HTMLCOMMENT +
  '|' +
  PROCESSINGINSTRUCTION +
  '|' +
  DECLARATION +
  '|' +
  CDATA +
  ')'

const ESCAPED_CHAR = '\\\\' + ESCAPABLE

// Every regex handed to `match()` is sticky: it must match at `pos`, and
// `lastIndex` then gives the new position. (The equivalent of anchoring with
// `^` on a sliced subject, without the slicing.)
const reHtmlTag = new RegExp(HTMLTAG, 'iy')
const reEntityHere = new RegExp(ENTITY, 'iy')
const reEscapable = new RegExp('^' + ESCAPABLE)

const reTicksHere = /`+/y
const reTicks = /`+/g

// Chunks of ordinary text: everything up to the next character that could
// start an inline construct. `~` is only special under GFM, hence two.
const reMain = /[^\n`[\]\\!<&*_]+/y
const reMainGfm = /[^\n`[\]\\!<&*_~]+/y

/**
 * Maximum `(` nesting inside a bare link destination. The spec describes
 * balanced parentheses without stating a bound; cmark uses 32, and the bound
 * is what keeps an unbalanced run from being quadratic (see
 * parseLinkDestination).
 */
const MAX_LINK_PAREN_NESTING = 32

const reSpnl = / *(?:\n *)?/y
const reInitialSpace = / */y
const reIndent = / {0,3}/y
const reFinalSpace = / *$/
const reSpaceAtEndOfLine = / *(?:\n|$)/y
const reWhitespaceChar = /[ \t\n\v\f\r]/

// §4.7: at most 999 characters between the brackets, brackets inside only if
// backslash-escaped. `s` so that an escaped line ending counts as an escape.
const reLinkLabel = /\[(?:[^\\[\]]|\\.){0,1000}\]/sy

// §6.3 link destination, pointy-bracket form: no line endings, no unescaped
// `<` or `>`.
const reLinkDestinationBraces = /<(?:[^<>\n\\\x00]|\\.)*>/y

// §6.3 link title in `"`, `'` or `(...)` delimiters. The escape alternative
// comes first so that `\"` inside a double-quoted title is consumed as an
// escape rather than closing the title.
const reLinkTitle = new RegExp(
  '(?:"(?:' +
    ESCAPED_CHAR +
    '|[^"\\x00])*"' +
    "|'(?:" +
    ESCAPED_CHAR +
    "|[^'\\x00])*'" +
    '|\\((?:' +
    ESCAPED_CHAR +
    '|[^()\\x00])*\\))',
  'y'
)

// §6.5 autolinks. Scheme: an ASCII letter then 1–31 more of letter/digit/
// `+`/`.`/`-`, i.e. 2–32 characters in total, case-insensitive.
const reAutolink = /<[A-Za-z][A-Za-z0-9.+-]{1,31}:[^<>\x00-\x20]*>/y
const reEmailAutolink =
  /<([a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)>/y

function text(literal: string): MdNode {
  const node = new MdNode('text')
  node.literal = literal
  return node
}

// §2.1 punctuation is P* ∪ S*, which `isUnicodePunctuation` already covers.
// `*$*alpha.` is the case that needs the symbol half: with `$` counted as
// punctuation the closing `*` is not right-flanking, so the line stays
// literal text.
const isPunctuation = isUnicodePunctuation

/** One `*`, `_` or `~` run on the delimiter stack (§6.4). */
type Delimiter = {
  /** Character code of the delimiter character. */
  cc: number
  /** Delimiters still unused; drops as pairs are consumed. */
  numdelims: number
  /** Run length as originally scanned — what the rule of three is computed on. */
  origdelims: number
  node: MdNode
  previous: Delimiter | null
  next: Delimiter | null
  canOpen: boolean
  canClose: boolean
}

/** One `[` or `![` on the bracket stack (§6.3). */
type Bracket = {
  node: MdNode
  previous: Bracket | null
  /** Top of the delimiter stack when this bracket opened — the emphasis floor. */
  previousDelimiter: Delimiter | null
  /** Offset of the `[` in the subject. */
  index: number
  image: boolean
  /** Cleared on earlier openers once a link is built: no links in links. */
  active: boolean
  /** Another bracket opened after this one, so its text contains a `[`. */
  bracketAfter: boolean
}

type DelimScan = { numdelims: number; canOpen: boolean; canClose: boolean }

class InlineParser {
  subject = ''
  pos = 0
  delimiters: Delimiter | null = null
  brackets: Bracket | null = null
  refmap: RefMap
  options: ParserOptions

  constructor(refmap: RefMap, options: ParserOptions) {
    this.refmap = refmap
    this.options = options
  }

  // --- scanning primitives ---

  /** Character code at `pos`, or -1 at the end of the subject. */
  peek(): number {
    return this.pos < this.subject.length ? this.subject.charCodeAt(this.pos) : -1
  }

  /** Match a sticky regex at `pos`; on success advance past it. */
  match(re: RegExp): string | null {
    re.lastIndex = this.pos
    const m = re.exec(this.subject)
    if (null === m) return null
    this.pos = re.lastIndex
    return m[0]
  }

  /** Spaces, and at most one line ending, between the parts of a link. */
  spnl(): void {
    this.match(reSpnl)
  }

  /**
   * The whole code point ending at `end`, so an astral character is
   * classified as one character by the flanking rules rather than as a lone
   * surrogate. Start of subject counts as a line ending (§6.4).
   */
  charBefore(end: number): string {
    if (end <= 0) return '\n'
    const lo = this.subject.charCodeAt(end - 1)
    if (lo >= 0xdc00 && lo <= 0xdfff && end >= 2) {
      const hi = this.subject.charCodeAt(end - 2)
      if (hi >= 0xd800 && hi <= 0xdbff) return this.subject.slice(end - 2, end)
    }
    return this.subject.charAt(end - 1)
  }

  /** The whole code point at `i`; end of subject counts as a line ending. */
  charAtPos(i: number): string {
    if (i >= this.subject.length) return '\n'
    const cp = this.subject.codePointAt(i)
    return undefined === cp ? '\n' : String.fromCodePoint(cp)
  }

  // --- §6.3 code spans ---

  /**
   * A code span runs from a backtick string to the next backtick string of
   * *exactly* the same length. Unmatched opening backticks are literal text.
   */
  parseBackticks(block: MdNode): boolean {
    const ticks = this.match(reTicksHere)
    if (null === ticks) return false

    const afterOpenTicks = this.pos
    reTicks.lastIndex = this.pos
    let m: RegExpExecArray | null

    while (null !== (m = reTicks.exec(this.subject))) {
      this.pos = reTicks.lastIndex
      if (m[0] === ticks) {
        const node = new MdNode('code')
        // Line endings become spaces; then one leading and one trailing
        // space are stripped together, but only if the content is not all
        // spaces (which is what lets a code span hold backticks).
        const contents = this.subject
          .slice(afterOpenTicks, this.pos - ticks.length)
          .replace(/\n/g, ' ')
        node.literal =
          contents.length > 2 &&
          ' ' === contents[0] &&
          ' ' === contents[contents.length - 1] &&
          null !== /[^ ]/.exec(contents)
            ? contents.slice(1, contents.length - 1)
            : contents
        block.appendChild(node)
        return true
      }
    }

    // No closing run of the same length: the opener is literal text.
    this.pos = afterOpenTicks
    block.appendChild(text(ticks))
    return true
  }

  // --- §6.1 backslash escapes, §6.7 hard line breaks ---

  parseBackslash(block: MdNode): boolean {
    this.pos += 1
    if (C_NEWLINE === this.peek()) {
      this.pos += 1
      block.appendChild(new MdNode('linebreak'))
      this.match(reInitialSpace)
    } else if (reEscapable.test(this.subject.charAt(this.pos))) {
      block.appendChild(text(this.subject.charAt(this.pos)))
      this.pos += 1
    } else {
      block.appendChild(text('\\'))
    }
    return true
  }

  // --- §6.5 autolinks ---

  parseAutolink(block: MdNode): boolean {
    let m = this.match(reEmailAutolink)
    if (null !== m) {
      const dest = m.slice(1, m.length - 1)
      block.appendChild(this.makeAutolink('mailto:' + dest, dest))
      return true
    }
    m = this.match(reAutolink)
    if (null !== m) {
      const dest = m.slice(1, m.length - 1)
      block.appendChild(this.makeAutolink(dest, dest))
      return true
    }
    return false
  }

  makeAutolink(destination: string, label: string): MdNode {
    // Autolink text is literal: no backslash escapes, no entity references.
    const node = new MdNode('link')
    // Not percent-encoded here — see parseLinkDestination.
    node.destination = destination
    node.title = null
    node.appendChild(text(label))
    return node
  }

  // --- §6.6 raw HTML ---

  parseHtmlTag(block: MdNode): boolean {
    const m = this.match(reHtmlTag)
    if (null === m) return false
    const node = new MdNode('html_inline')
    node.literal = m
    block.appendChild(node)
    return true
  }

  // --- §6.4 emphasis: delimiter runs and flanking ---

  /**
   * Classify the run of `cc` starting at `pos` without consuming it.
   *
   * §6.4: a run is left-flanking iff it is not followed by Unicode
   * whitespace and either not followed by Unicode punctuation or else
   * preceded by whitespace or punctuation; right-flanking is the mirror
   * image. `_` additionally may not open inside a word (rules 2, 4, 6, 8),
   * which is what keeps `snake_case_words` intact; `*` has no such
   * restriction.
   */
  scanDelims(cc: number): DelimScan | null {
    const startpos = this.pos
    let numdelims = 0

    while (cc === this.peek()) {
      numdelims += 1
      this.pos += 1
    }
    if (0 === numdelims) return null

    const charBefore = this.charBefore(startpos)
    const charAfter = this.charAtPos(this.pos)

    const afterIsWhitespace = isUnicodeWhitespace(charAfter)
    const afterIsPunctuation = isPunctuation(charAfter)
    const beforeIsWhitespace = isUnicodeWhitespace(charBefore)
    const beforeIsPunctuation = isPunctuation(charBefore)

    const leftFlanking =
      !afterIsWhitespace &&
      (!afterIsPunctuation || beforeIsWhitespace || beforeIsPunctuation)
    const rightFlanking =
      !beforeIsWhitespace &&
      (!beforeIsPunctuation || afterIsWhitespace || afterIsPunctuation)

    let canOpen: boolean
    let canClose: boolean
    if (C_UNDERSCORE === cc) {
      canOpen = leftFlanking && (!rightFlanking || beforeIsPunctuation)
      canClose = rightFlanking && (!leftFlanking || afterIsPunctuation)
    } else {
      canOpen = leftFlanking
      canClose = rightFlanking
    }

    this.pos = startpos
    return { numdelims, canOpen, canClose }
  }

  /** Emit a delimiter run as text and push it on the delimiter stack. */
  handleDelim(cc: number, block: MdNode): boolean {
    const res = this.scanDelims(cc)
    if (null === res) return false

    const numdelims = res.numdelims
    const startpos = this.pos
    this.pos += numdelims

    const node = text(this.subject.slice(startpos, this.pos))
    block.appendChild(node)

    // GFM strikethrough only recognises runs of one or two tildes; longer
    // runs stay literal.
    const stackable = C_TILDE === cc ? numdelims <= 2 : true

    if ((res.canOpen || res.canClose) && stackable) {
      const delim: Delimiter = {
        cc,
        numdelims,
        origdelims: numdelims,
        node,
        previous: this.delimiters,
        next: null,
        canOpen: res.canOpen,
        canClose: res.canClose,
      }
      if (null !== delim.previous) delim.previous.next = delim
      this.delimiters = delim
    }

    return true
  }

  removeDelimiter(delim: Delimiter): void {
    if (null !== delim.previous) delim.previous.next = delim.next
    if (null === delim.next) {
      // Top of stack.
      this.delimiters = delim.previous
    } else {
      delim.next.previous = delim.previous
    }
  }

  /**
   * The spec's "process emphasis" procedure. `stackBottom` is the floor: for
   * the whole-block call it is null, and for the inlines of a link it is the
   * delimiter that was on top when the link's `[` opened.
   */
  processEmphasis(stackBottom: Delimiter | null): void {
    // openers_bottom, keyed by (delimiter char, whether the closer can also
    // open, closer run length mod 3). Once a closer of some class fails to
    // find an opener, no later closer of the same class needs to look past
    // that point either — this is what keeps the search linear.
    const openersBottom = new Map<string, Delimiter | null>()

    // First closer above stackBottom.
    let closer = this.delimiters
    while (null !== closer && closer.previous !== stackBottom) {
      closer = closer.previous
    }

    while (null !== closer) {
      if (!closer.canClose) {
        closer = closer.next
        continue
      }

      const closercc = closer.cc
      const key = closercc + ':' + (closer.canOpen ? 1 : 0) + ':' + (closer.origdelims % 3)
      const bottom = openersBottom.has(key)
        ? (openersBottom.get(key) as Delimiter | null)
        : stackBottom

      let opener = closer.previous
      let openerFound = false
      while (null !== opener && opener !== stackBottom && opener !== bottom) {
        if (opener.cc === closercc && opener.canOpen) {
          if (C_TILDE === closercc) {
            // GFM strikethrough: opener and closer runs must be the same
            // length, so `~~x~` is not a match.
            if (opener.numdelims === closer.numdelims) {
              openerFound = true
              break
            }
          } else {
            // §6.4 rules 9 and 10, the "rule of three": when either side of
            // the pair could both open and close, the two run lengths may
            // not sum to a multiple of 3 — unless both are themselves
            // multiples of 3.
            const oddMatch =
              (closer.canOpen || opener.canClose) &&
              0 !== closer.origdelims % 3 &&
              0 === (opener.origdelims + closer.origdelims) % 3
            if (!oddMatch) {
              openerFound = true
              break
            }
          }
        }
        opener = opener.previous
      }

      const oldCloser = closer

      if (openerFound && null !== opener) {
        // Two delimiters make strong, one makes emph; strikethrough always
        // consumes the whole (equal-length) run.
        const useDelims =
          C_TILDE === closercc
            ? closer.numdelims
            : closer.numdelims >= 2 && opener.numdelims >= 2
              ? 2
              : 1
        const nodeType: NodeType =
          C_TILDE === closercc ? 'del' : 1 === useDelims ? 'emph' : 'strong'

        const openerInl = opener.node
        const closerInl = closer.node

        opener.numdelims -= useDelims
        closer.numdelims -= useDelims
        openerInl.literal = (openerInl.literal ?? '').slice(
          0,
          (openerInl.literal ?? '').length - useDelims
        )
        closerInl.literal = (closerInl.literal ?? '').slice(
          0,
          (closerInl.literal ?? '').length - useDelims
        )

        // Everything between the two delimiter text nodes becomes the
        // content of the new node.
        const emph = new MdNode(nodeType)
        let tmp = openerInl.next
        while (null !== tmp && tmp !== closerInl) {
          const nxt = tmp.next
          tmp.unlink()
          emph.appendChild(tmp)
          tmp = nxt
        }
        openerInl.insertAfter(emph)

        // Delimiters between the pair can never match anything now.
        if (opener.next !== closer) {
          opener.next = closer
          closer.previous = opener
        }

        if (0 === opener.numdelims) {
          openerInl.unlink()
          this.removeDelimiter(opener)
        }
        if (0 === closer.numdelims) {
          closerInl.unlink()
          const tempstack = closer.next
          this.removeDelimiter(closer)
          closer = tempstack
        }
      } else {
        closer = closer.next
        openersBottom.set(key, oldCloser.previous)
        if (!oldCloser.canOpen) {
          // It found no opener and cannot be one: it is inert.
          this.removeDelimiter(oldCloser)
        }
      }
    }

    // Drop everything above the floor.
    while (null !== this.delimiters && this.delimiters !== stackBottom) {
      this.removeDelimiter(this.delimiters)
    }
  }

  // --- §6.3 links and images ---

  /** Link title without its delimiters, unescaped; null if none here. */
  parseLinkTitle(): string | null {
    const title = this.match(reLinkTitle)
    if (null === title) return null
    return unescapeString(title.slice(1, title.length - 1))
  }

  /**
   * Link destination: either `<...>`, or a bare run with balanced unescaped
   * parentheses and no ASCII control characters or spaces.
   */
  /**
   * A destination is backslash-unescaped and entity-decoded, but **not**
   * percent-encoded: `[l](/ä)` yields `/ä`, not `/%C3%A4`.
   *
   * Encoding is a rendering concern and `html.ts` applies `normalizeURI` when
   * it writes the attribute. Doing it here too would be invisible in the HTML
   * (`normalizeURI` is idempotent over its own output) but would put the
   * encoded form in the public AST, where mdast — and every previous release
   * of this package — promises the decoded one.
   */
  parseLinkDestination(): string | null {
    const braced = this.match(reLinkDestinationBraces)
    if (null !== braced) {
      return unescapeString(braced.slice(1, braced.length - 1))
    }
    if (C_LESSTHAN === this.peek()) return null

    const savepos = this.pos
    let openparens = 0
    let c = this.peek()

    while (-1 !== c) {
      if (C_BACKSLASH === c && reEscapable.test(this.subject.charAt(this.pos + 1))) {
        this.pos += 1
        if (-1 !== this.peek()) this.pos += 1
      } else if (C_OPEN_PAREN === c) {
        // Bail rather than scan on once nesting passes the limit. Without
        // this, input like `[a](b` repeated makes every `]` scan to end of
        // input looking for parens that never balance, which is quadratic in
        // the document size. cmark caps nesting at the same depth, and no
        // spec example nests more than two deep.
        if (MAX_LINK_PAREN_NESTING <= openparens) return null
        this.pos += 1
        openparens += 1
      } else if (C_CLOSE_PAREN === c) {
        if (openparens < 1) break
        this.pos += 1
        openparens -= 1
      } else if (c <= C_SPACE || C_DEL === c) {
        // Spaces and ASCII control characters end the bare form.
        break
      } else {
        this.pos += 1
      }
      c = this.peek()
    }

    // An empty destination is only legal immediately before the closing
    // paren of an inline link, as in `[link]()`.
    if (this.pos === savepos && C_CLOSE_PAREN !== c) return null
    if (0 !== openparens) return null

    return unescapeString(this.subject.slice(savepos, this.pos))
  }

  /** Length of the link label at `pos`, including brackets; 0 if invalid. */
  parseLinkLabel(): number {
    const m = this.match(reLinkLabel)
    // §4.7: at most 999 characters between the brackets.
    if (null === m || m.length > 1001) return 0
    return m.length
  }

  addBracket(node: MdNode, index: number, image: boolean): void {
    if (null !== this.brackets) this.brackets.bracketAfter = true
    this.brackets = {
      node,
      previous: this.brackets,
      previousDelimiter: this.delimiters,
      index,
      image,
      active: true,
      bracketAfter: false,
    }
  }

  removeBracket(): void {
    if (null !== this.brackets) this.brackets = this.brackets.previous
  }

  parseOpenBracket(block: MdNode): boolean {
    const startpos = this.pos
    this.pos += 1
    const node = text('[')
    block.appendChild(node)
    this.addBracket(node, startpos, false)
    return true
  }

  /** `!` only matters when it introduces an image. */
  parseBang(block: MdNode): boolean {
    const startpos = this.pos
    this.pos += 1
    if (C_OPEN_BRACKET === this.peek()) {
      this.pos += 1
      const node = text('![')
      block.appendChild(node)
      this.addBracket(node, startpos + 1, true)
    } else {
      block.appendChild(text('!'))
    }
    return true
  }

  /**
   * The spec's "look for link or image" procedure. On a `]`, walk back to
   * the newest bracket opener and try, in order: an inline `(dest "title")`,
   * a full `[label]`, a collapsed `[]` or a shortcut reference. Anything that
   * fails backtracks to just after the `]` and leaves a literal `]`.
   */
  parseCloseBracket(block: MdNode): boolean {
    this.pos += 1
    const startpos = this.pos

    const opener = this.brackets
    if (null === opener) {
      block.appendChild(text(']'))
      return true
    }
    if (!opener.active) {
      // Deactivated by an enclosing link: not an opener any more.
      block.appendChild(text(']'))
      this.removeBracket()
      return true
    }

    const isImage = opener.image
    let dest: string | null = null
    let title: string | null = null
    let matched = false

    const savepos = this.pos

    // Inline link: `](` destination title `)`.
    if (C_OPEN_PAREN === this.peek()) {
      this.pos += 1
      this.spnl()
      const d = this.parseLinkDestination()
      if (null !== d) {
        this.spnl()
        // A title has to be separated from the destination by whitespace,
        // so `[a](/url"t")` is not a titled link.
        let t: string | null = null
        if (reWhitespaceChar.test(this.subject.charAt(this.pos - 1))) {
          t = this.parseLinkTitle()
        }
        this.spnl()
        if (C_CLOSE_PAREN === this.peek()) {
          this.pos += 1
          dest = d
          title = t
          matched = true
        }
      }
      if (!matched) this.pos = savepos
    }

    if (!matched) {
      // Reference forms. A second label gives the full form; an empty or
      // absent one means the link text is itself the label (collapsed and
      // shortcut) — which is only possible if that text holds no bracket.
      let reflabel: string | null = null
      const beforelabel = this.pos
      const n = this.parseLinkLabel()
      if (n > 2) {
        reflabel = this.subject.slice(beforelabel + 1, beforelabel + n - 1)
      } else if (!opener.bracketAfter) {
        reflabel = this.subject.slice(opener.index + 1, startpos - 1)
      }
      if (0 === n) this.pos = savepos

      if (null !== reflabel) {
        const link = this.refmap[normalizeReference(reflabel)]
        if (undefined !== link) {
          dest = link.destination
          title = link.title
          matched = true
        }
      }
    }

    if (!matched) {
      this.removeBracket()
      this.pos = startpos
      block.appendChild(text(']'))
      return true
    }

    const node = new MdNode(isImage ? 'image' : 'link')
    node.destination = dest
    node.title = null === title || '' === title ? null : title

    let tmp = opener.node.next
    while (null !== tmp) {
      const nxt = tmp.next
      tmp.unlink()
      node.appendChild(tmp)
      tmp = nxt
    }
    block.appendChild(node)

    // Emphasis inside the link text resolves now, and only down to the
    // delimiter that was current when the `[` opened.
    this.processEmphasis(opener.previousDelimiter)
    this.removeBracket()
    opener.node.unlink()

    // Links may not contain links (§6.3): every earlier link opener is
    // spent. Image openers survive, since images may nest in links.
    if (!isImage) {
      let b = this.brackets
      while (null !== b) {
        if (!b.image) b.active = false
        b = b.previous
      }
    }

    return true
  }

  // --- §6.2 entities, §6.8/§6.9 breaks and plain text ---

  parseEntity(block: MdNode): boolean {
    const m = this.match(reEntityHere)
    if (null === m) return false
    block.appendChild(text(decodeEntity(m)))
    return true
  }

  parseString(block: MdNode): boolean {
    const m = this.match(this.options.gfm ? reMainGfm : reMain)
    if (null === m) return false
    block.appendChild(text(m))
    return true
  }

  /**
   * §6.7/§6.8: two or more spaces before the line ending make a hard break,
   * anything else a soft one. Trailing spaces are dropped either way, as is
   * the indentation of the next line.
   */
  parseNewline(block: MdNode): boolean {
    this.pos += 1
    const lastc = block.lastChild
    if (
      null !== lastc &&
      'text' === lastc.type &&
      null !== lastc.literal &&
      ' ' === lastc.literal.charAt(lastc.literal.length - 1)
    ) {
      const hardbreak = ' ' === lastc.literal.charAt(lastc.literal.length - 2)
      lastc.literal = lastc.literal.replace(reFinalSpace, '')
      block.appendChild(new MdNode(hardbreak ? 'linebreak' : 'softbreak'))
    } else {
      block.appendChild(new MdNode('softbreak'))
    }
    this.match(reInitialSpace)
    return true
  }

  /** One inline construct at `pos`; false only at the end of the subject. */
  parseInline(block: MdNode): boolean {
    const c = this.peek()
    if (-1 === c) return false

    let res = false
    switch (c) {
      case C_NEWLINE:
        res = this.parseNewline(block)
        break
      case C_BACKSLASH:
        res = this.parseBackslash(block)
        break
      case C_BACKTICK:
        res = this.parseBackticks(block)
        break
      case C_ASTERISK:
      case C_UNDERSCORE:
        res = this.handleDelim(c, block)
        break
      case C_TILDE:
        // Only GFM gives `~` a meaning; otherwise it is ordinary text (and
        // the non-GFM `reMain` lets parseString swallow whole runs of it).
        res = this.options.gfm ? this.handleDelim(c, block) : this.parseString(block)
        break
      case C_OPEN_BRACKET:
        res = this.parseOpenBracket(block)
        break
      case C_BANG:
        res = this.parseBang(block)
        break
      case C_CLOSE_BRACKET:
        res = this.parseCloseBracket(block)
        break
      case C_LESSTHAN:
        res = this.parseAutolink(block) || this.parseHtmlTag(block)
        break
      case C_AMPERSAND:
        res = this.parseEntity(block)
        break
      default:
        res = this.parseString(block)
        break
    }

    if (!res) {
      // Nothing claimed it: the character is literal text.
      this.pos += 1
      block.appendChild(text(String.fromCharCode(c)))
    }

    return true
  }

  /** Parse one paragraph or heading's raw content into inline children. */
  parse(block: MdNode): void {
    // Trimming is what stops trailing spaces at the end of a block from
    // making a hard break, and drops the leading indentation of the first line.
    this.subject = block.stringContent.trim()
    this.pos = 0
    this.delimiters = null
    this.brackets = null

    while (this.parseInline(block));

    block.stringContent = ''
    this.processEmphasis(null)
  }

  /**
   * §4.7: consume leading link reference definitions. Called by the block
   * phase while finalizing a paragraph, repeatedly, until it returns 0.
   */
  parseReference(s: string): number {
    this.subject = s
    this.pos = 0

    const startpos = this.pos

    // §4.7 allows up to three spaces of indentation before the label. The
    // block phase normally strips a paragraph line's indentation before it
    // gets here, so this usually consumes nothing.
    this.match(reIndent)

    const labelStart = this.pos
    const matchChars = this.parseLinkLabel()
    if (0 === matchChars) return 0
    const rawlabel = this.subject.slice(labelStart + 1, labelStart + matchChars - 1)

    if (C_COLON === this.peek()) {
      this.pos += 1
    } else {
      this.pos = startpos
      return 0
    }

    this.spnl()

    const dest = this.parseLinkDestination()
    if (null === dest) {
      this.pos = startpos
      return 0
    }

    const beforetitle = this.pos
    this.spnl()
    let title: string | null = null
    if (this.pos !== beforetitle) {
      title = this.parseLinkTitle()
    }
    if (null === title) {
      this.pos = beforetitle
    }

    // Nothing but spaces may follow on the definition's last line.
    let atLineEnd = true
    if (null === this.match(reSpaceAtEndOfLine)) {
      if (null === title) {
        atLineEnd = false
      } else {
        // What looked like a title is something else; the definition may
        // still be valid without it.
        title = null
        this.pos = beforetitle
        atLineEnd = null !== this.match(reSpaceAtEndOfLine)
      }
    }
    if (!atLineEnd) {
      this.pos = startpos
      return 0
    }

    const normlabel = normalizeReference(rawlabel)
    if ('' === normlabel) {
      // A label must hold at least one non-whitespace character.
      this.pos = startpos
      return 0
    }

    // First definition wins.
    if (undefined === this.refmap[normlabel]) {
      this.refmap[normlabel] = { destination: dest, title }
    }

    return this.pos - startpos
  }
}

// --- GFM extended autolinks (autolink literals) -----------------------------
//
// GFM recognises `www.x.com`, `https://x.com` and `a@x.com` without the `<…>`
// CommonMark requires. This runs as a post-pass over the finished inline tree
// rather than as another branch of the scan above, which is how cmark-gfm does
// it and is the only shape that keeps 652/652 safe: the scanner that decides
// code spans, raw HTML, emphasis and links is not touched at all, and an
// autolink can therefore never win against a construct CommonMark says comes
// first.
//
// The pass rewrites `text` nodes into text/link/text runs. It does not descend
// into `link` (links may not contain links) or `image` (its children are the
// alt text), and `code`/`html_inline` are leaves it never looks inside.

/** RFC 1035's limit on a fully-qualified domain name. See `scanDomainEnd`. */
const MAX_AUTOLINK_DOMAIN = 253

/** RFC 5321's limit on the local part of an address, before the `@`. */
const MAX_EMAIL_LOCAL = 64

/** The three schemes the extension recognises without angle brackets. */
const AUTOLINK_SCHEMES = ['https://', 'http://', 'ftp://']

function isAsciiAlnum(c: number): boolean {
  return (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)
}

function isAutolinkSpace(c: number): boolean {
  return (
    C_SPACE === c ||
    C_TAB === c ||
    C_NEWLINE === c ||
    C_VTAB === c ||
    C_FORMFEED === c ||
    C_CARRIAGE_RETURN === c
  )
}

/** A domain segment holds alphanumerics, `_` and `-`; `.` separates them. */
function isDomainChar(c: number): boolean {
  return isAsciiAlnum(c) || C_UNDERSCORE === c || C_HYPHEN === c || C_PERIOD === c
}

/** Before the `@`: alphanumerics and `.`, `-`, `_`, `+`. */
function isEmailLocalChar(c: number): boolean {
  return isAsciiAlnum(c) || C_PERIOD === c || C_HYPHEN === c || C_UNDERSCORE === c || C_PLUS === c
}

/**
 * "All such recognized autolinks can only come at the beginning of a line,
 * after whitespace, or any of the delimiting characters `*`, `_`, `~`, `(`."
 *
 * Offset 0 has no preceding character in `s` at all, so the caller answers for
 * it through `atStart` — see `startsAfterDelimiter`.
 */
function isAutolinkStart(s: string, i: number, atStart: boolean): boolean {
  if (0 === i) return atStart
  const c = s.charCodeAt(i - 1)
  return (
    isAutolinkSpace(c) ||
    C_ASTERISK === c ||
    C_UNDERSCORE === c ||
    C_TILDE === c ||
    C_OPEN_PAREN === c
  )
}

/**
 * Whether offset 0 of a text node may begin an autolink, given the sibling in
 * front of it.
 *
 * The rule above is about the source character immediately before the match,
 * and by the time this pass runs the tree no longer carries source offsets —
 * but the previous sibling's *type* names that character exactly:
 *
 *   * `emph` and `strong` end on the `*` or `_` that closed them, and `del` on
 *     its second `~`. All three are delimiters the rule allows, so
 *     `*a*www.b.com` links.
 *   * `code` ends on a backtick, `link` and `image` on `)`, `]` or `>`, and
 *     `html_inline` on `>`. None are delimiters, so `` `x`www.a.com ``,
 *     `[l](/u)www.a.com` and `<b>www.a.com` do not link.
 *   * a `softbreak` or `linebreak` puts offset 0 at the beginning of a line,
 *     and so does having no previous sibling — that is the first inline of the
 *     block, or of the emphasis span this pass descended into.
 *
 * A `text` node never appears here: `linkifyChildren` merges every run of
 * adjacent text siblings before handing the first of them over.
 */
function startsAfterDelimiter(prev: MdNode | null): boolean {
  if (null === prev) return true
  const t = prev.type
  return 'softbreak' === t || 'linebreak' === t || 'emph' === t || 'strong' === t || 'del' === t
}

/** ASCII case-insensitive comparison against an already-lowercase literal. */
function matchesIgnoreCase(s: string, at: number, lower: string): boolean {
  if (at + lower.length > s.length) return false
  for (let k = 0; k < lower.length; k++) {
    let c = s.charCodeAt(at + k)
    if (c >= 65 && c <= 90) c += 32
    if (c !== lower.charCodeAt(k)) return false
  }
  return true
}

/**
 * End of the run of domain characters at `from`, capped at
 * `MAX_AUTOLINK_DOMAIN`.
 *
 * The cap is what keeps this pass linear. `_` both continues a domain and
 * introduces a valid autolink start, so without a bound an input like
 * `'_www.a_b.c'.repeat(n)` gives every underscore a candidate whose domain run
 * reaches the end of the text — quadratic. Anything past the cap is still
 * scanned, just as the path rather than as the domain, and no real domain
 * comes near it.
 */
function scanDomainEnd(s: string, from: number): number {
  const max = Math.min(s.length, from + MAX_AUTOLINK_DOMAIN)
  let i = from
  while (i < max && isDomainChar(s.charCodeAt(i))) i++
  return i
}

/**
 * A valid domain: at least one period, and no underscore in either of the last
 * two segments. `from`..`to` must already be a run of domain characters.
 */
function isValidDomain(s: string, from: number, to: number): boolean {
  if (to <= from) return false
  let periods = 0
  let underscoresInLast = 0
  let underscoresInPrev = 0
  for (let i = from; i < to; i++) {
    const c = s.charCodeAt(i)
    if (C_UNDERSCORE === c) {
      underscoresInLast++
    } else if (C_PERIOD === c) {
      underscoresInPrev = underscoresInLast
      underscoresInLast = 0
      periods++
    }
  }
  return 0 < periods && 0 === underscoresInLast && 0 === underscoresInPrev
}

/**
 * "Extended autolink path validation" — pull the end of the match back off
 * trailing characters that read as sentence punctuation rather than as part of
 * a URL. One loop rather than three passes, because dropping a `)` can expose
 * a `.` and vice versa.
 *
 *   * `?` `!` `.` `,` `:` `*` `_` `~` are dropped wherever they trail;
 *   * a trailing `)` is dropped only while the match holds more `)` than `(`,
 *     so `(www.a.com/x_(y))` keeps its inner pair and loses the outer one;
 *   * a trailing `;` preceded by something shaped like an entity reference
 *     takes the whole `&…;` with it.
 *
 * The parenthesis counts are taken once and then maintained, not recomputed
 * per drop: a deeply parenthesised URL is otherwise quadratic in its own
 * length.
 */
function trimAutolinkEnd(s: string, start: number, end: number): number {
  let opening = -1
  let closing = -1

  while (start < end) {
    const c = s.charCodeAt(end - 1)

    if (
      C_QUESTION === c ||
      C_BANG === c ||
      C_PERIOD === c ||
      C_COMMA === c ||
      C_COLON === c ||
      C_ASTERISK === c ||
      C_UNDERSCORE === c ||
      C_TILDE === c
    ) {
      end--
      continue
    }

    if (C_CLOSE_PAREN === c) {
      if (opening < 0) {
        opening = 0
        closing = 0
        for (let i = start; i < end; i++) {
          const d = s.charCodeAt(i)
          if (C_OPEN_PAREN === d) opening++
          else if (C_CLOSE_PAREN === d) closing++
        }
      }
      if (closing <= opening) return end
      closing--
      end--
      continue
    }

    if (C_SEMICOLON === c) {
      // `&` followed by one or more alphanumerics, then this `;`.
      let j = end - 2
      while (j > start && isAsciiAlnum(s.charCodeAt(j))) j--
      if (j < end - 2 && C_AMPERSAND === s.charCodeAt(j)) {
        end = j
        continue
      }
      return end
    }

    return end
  }

  return end
}

/** One recognised autolink literal: `s.slice(start, end)` linking to `href`. */
type AutolinkMatch = { start: number; end: number; href: string }

/**
 * Every autolink literal in one text run, left to right and non-overlapping.
 * Returns null when there is nothing to do, which is the overwhelmingly common
 * case and saves the caller an array and a splice.
 */
function scanAutolinks(s: string, atStart: boolean): AutolinkMatch[] | null {
  const len = s.length
  let out: AutolinkMatch[] | null = null

  // The maximal run of non-space, non-`<` characters containing the current
  // position — "zero or more non-space non-`<` characters may follow" the
  // domain. Cached because candidates are tried left to right, so each run is
  // scanned once however many candidates start inside it.
  let runStart = -1
  let runEnd = -1

  let i = 0
  let lastEnd = 0

  while (i < len) {
    const c = s.charCodeAt(i)

    // --- email: found by its `@`, then rewound to the start of the local part
    if (C_AT === c) {
      // The rewind is bounded to keep this pass linear, and RFC 5321 bounds a
      // local part in the same place anyway, so the cap costs no real address.
      const capped = i - MAX_EMAIL_LOCAL
      const floor = Math.max(lastEnd, capped)
      let k = i
      while (k > floor && isEmailLocalChar(s.charCodeAt(k - 1))) k--

      // Stopping *on* the cap is not the same as finding a boundary. If the
      // character just outside it still belongs to the local part, the local
      // part is longer than the cap allows and there is no address here at
      // all: accepting the match would link the 64-character tail of a longer
      // run and leave the rest as text, inventing an address the source never
      // wrote. `k` can only reach `capped` when the cap, rather than `lastEnd`,
      // is what ended the loop; and `k` of 0 is the start of the string, where
      // there is no character outside the cap to disqualify anything.
      if (k === capped && 0 < k && isEmailLocalChar(s.charCodeAt(k - 1))) {
        i++
        continue
      }

      if (k < i && isAutolinkStart(s, k, atStart)) {
        // After the `@`: alphanumerics, `.`, `-`, `_`; at least one period; a
        // trailing `.` is not part of the address, and a trailing `-` or `_`
        // invalidates the whole thing.
        let end = i + 1
        while (end < len && isDomainChar(s.charCodeAt(end))) end++
        while (end > i + 1 && C_PERIOD === s.charCodeAt(end - 1)) end--

        const last = end > i + 1 ? s.charCodeAt(end - 1) : 0
        if (
          end > i + 1 &&
          C_HYPHEN !== last &&
          C_UNDERSCORE !== last &&
          hasPeriod(s, i + 1, end)
        ) {
          if (null === out) out = []
          out.push({ start: k, end, href: 'mailto:' + s.slice(k, end) })
          lastEnd = end
          i = end
          continue
        }
      }
      i++
      continue
    }

    // --- www / scheme: both need a valid start and a valid domain
    if (!couldStartUrl(c) || !isAutolinkStart(s, i, atStart)) {
      i++
      continue
    }

    let domainStart = -1
    let prefix = ''
    if (matchesIgnoreCase(s, i, 'www.')) {
      // The domain of a `www.` autolink includes the `www.` itself, so the
      // required period is already there and `www.foo_bar.com` is rejected on
      // the same footing as `http://foo_bar.com`.
      domainStart = i
      prefix = 'http://'
    } else {
      for (const scheme of AUTOLINK_SCHEMES) {
        if (matchesIgnoreCase(s, i, scheme)) {
          domainStart = i + scheme.length
          break
        }
      }
    }
    if (0 > domainStart) {
      i++
      continue
    }

    const domainEnd = scanDomainEnd(s, domainStart)
    // Checked before the run scan below, so a candidate that cannot possibly
    // match costs only its own domain run.
    if (!isValidDomain(s, domainStart, domainEnd)) {
      i++
      continue
    }

    if (i >= runEnd || i < runStart) {
      runStart = i
      runEnd = i
      while (runEnd < len) {
        const d = s.charCodeAt(runEnd)
        if (isAutolinkSpace(d) || C_LESSTHAN === d) break
        runEnd++
      }
    }

    const end = trimAutolinkEnd(s, i, runEnd)
    // Trailing punctuation can eat back into the domain — `www.commonmark.org.`
    // loses its last period — so the domain is validated again over what
    // actually survived.
    if (end <= domainStart || !isValidDomain(s, domainStart, Math.min(domainEnd, end))) {
      i++
      continue
    }

    if (null === out) out = []
    out.push({ start: i, end, href: prefix + s.slice(i, end) })
    lastEnd = end
    i = end
  }

  return out
}

/** `www.`, `http://`, `https://` and `ftp://` all start with one of these. */
function couldStartUrl(c: number): boolean {
  return 119 === c || 87 === c || 104 === c || 72 === c || 102 === c || 70 === c
}

function hasPeriod(s: string, from: number, to: number): boolean {
  for (let i = from; i < to; i++) {
    if (C_PERIOD === s.charCodeAt(i)) return true
  }
  return false
}

/** Replace one text node with the text/link/text run its literal implies. */
function linkifyTextNode(node: MdNode): void {
  const s = node.literal ?? ''
  const matches = scanAutolinks(s, startsAfterDelimiter(node.prev))
  if (null === matches) return

  let at = 0
  for (const m of matches) {
    if (at < m.start) node.insertBefore(text(s.slice(at, m.start)))
    const link = new MdNode('link')
    // Not percent-encoded here, for the same reason as parseLinkDestination:
    // encoding belongs to the renderer, and the AST promises the decoded form.
    link.destination = m.href
    link.title = null
    link.appendChild(text(s.slice(m.start, m.end)))
    node.insertBefore(link)
    at = m.end
  }
  if (at < s.length) node.insertBefore(text(s.slice(at)))

  node.unlink()
}

/**
 * Autolink the text children of one container, and report the child containers
 * still to visit.
 */
function linkifyChildren(parent: MdNode, pending: MdNode[]): void {
  let child: MdNode | null = parent.firstChild

  while (null !== child) {
    if ('text' === child.type) {
      // Consolidate the run of adjacent text nodes first. The scan above emits
      // a node per delimiter run and per character it could not claim, so
      // `a.b-c_d@a.b` arrives as three siblings and the address is only
      // visible once they are one string. Merging changes nothing downstream:
      // the renderer writes text literals back to back, and the AST projection
      // already coalesces adjacent runs.
      let sibling: MdNode | null = child.next
      if (null !== sibling && 'text' === sibling.type) {
        let merged = child.literal ?? ''
        while (null !== sibling && 'text' === sibling.type) {
          merged += sibling.literal ?? ''
          const next: MdNode | null = sibling.next
          sibling.unlink()
          sibling = next
        }
        child.literal = merged
      }

      const after: MdNode | null = child.next
      linkifyTextNode(child)
      child = after
      continue
    }

    // No links inside links (§6.3), and an image's children are its alt text.
    if ('link' !== child.type && 'image' !== child.type && child.isContainer()) {
      pending.push(child)
    }
    child = child.next
  }
}

/**
 * Recognise GFM autolink literals throughout one finished block's inlines.
 *
 * Iterative rather than recursive: emphasis nests as deeply as the input says.
 */
export function linkifyAutolinks(block: MdNode): void {
  const pending: MdNode[] = [block]
  while (0 < pending.length) {
    linkifyChildren(pending.pop() as MdNode, pending)
  }
}

/** Blocks whose raw content the block phase left for this phase to parse. */
const INLINE_CONTENT_BLOCKS: Record<string, true> = {
  paragraph: true,
  heading: true,
  // GFM table cells hold inlines and nothing else — "block-level elements
  // cannot be inserted in a table" — so a cell is parsed exactly as a
  // paragraph is, autolink post-pass included.
  table_cell: true,
}

/**
 * Phase 2 entry point: walk the block tree and parse the string content of
 * every paragraph, heading and table cell into inline children.
 */
export function parseInlines(doc: MdNode, refmap: RefMap, options: ParserOptions): void {
  const parser = new InlineParser(refmap, options)
  const walker = doc.walker()

  let ev = walker.next()
  while (null !== ev) {
    const node = ev.node
    if (!ev.entering && true === INLINE_CONTENT_BLOCKS[node.type]) {
      parser.parse(node)
      if (options.gfm) linkifyAutolinks(node)
    }
    ev = walker.next()
  }
}

// Reference definitions are parsed during the block phase, before any
// InlineParser for the document exists, so this one is kept aside for it.
// Nothing here depends on the parse options.
const referenceParser = new InlineParser({}, { gfm: false, breaks: false })

/**
 * Consume link reference definitions from the front of a paragraph's raw
 * content, adding them to `refmap`. Returns the number of characters
 * consumed, or 0 if `s` does not start with a definition.
 */
export function parseReference(s: string, refmap: RefMap): number {
  referenceParser.refmap = refmap
  return referenceParser.parseReference(s)
}
