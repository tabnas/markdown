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
  normalizeURI,
  unescapeString,
} from './common.ts'

const C_NEWLINE = 10
const C_SPACE = 32
const C_BANG = 33
const C_AMPERSAND = 38
const C_OPEN_PAREN = 40
const C_CLOSE_PAREN = 41
const C_ASTERISK = 42
const C_COLON = 58
const C_LESSTHAN = 60
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
    node.destination = normalizeURI(destination)
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
  parseLinkDestination(): string | null {
    const braced = this.match(reLinkDestinationBraces)
    if (null !== braced) {
      return normalizeURI(unescapeString(braced.slice(1, braced.length - 1)))
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

    return normalizeURI(unescapeString(this.subject.slice(savepos, this.pos)))
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

/**
 * Phase 2 entry point: walk the block tree and parse the string content of
 * every paragraph and heading into inline children.
 */
export function parseInlines(doc: MdNode, refmap: RefMap, options: ParserOptions): void {
  const parser = new InlineParser(refmap, options)
  const walker = doc.walker()

  let ev = walker.next()
  while (null !== ev) {
    const node = ev.node
    if (!ev.entering && ('paragraph' === node.type || 'heading' === node.type)) {
      parser.parse(node)
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
