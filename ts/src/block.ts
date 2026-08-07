/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Phase 1 of the CommonMark parse: block structure.
// Spec 0.31.2, Appendix A "A parsing strategy".
//
// The document is a tree of blocks whose right spine — the document, then last
// child, then last child of that, and so on — is *open*: each open block
// imposes a continuation condition on the next line. For every line we
//
//   1. walk that spine, consuming each block's continuation marker (`>` for a
//      block quote, the content indent for a list item, ...) and remember the
//      last container that matched;
//   2. look for new block starts at the offset that walk reached, closing the
//      unmatched tail of the spine as soon as a start actually matches;
//   3. add whatever is left of the line to the deepest open block, opening a
//      paragraph if there is nowhere else for the text to go.
//
// Unmatched blocks are closed in step 2/3 rather than in step 1 because a
// paragraph line may be a *lazy continuation*: its containers' markers are
// missing, but the paragraph carries on regardless (§5.1 rule 2).
//
// Positions on a line are tracked twice: `offset`, a UTF-16 index used to
// slice the text we keep, and `column`, the tab-expanded display column used
// for every indent decision (§2.2). The two must move together, including the
// case where a marker consumes only part of a tab — see `advanceOffset` and
// `addLine`.
//
// This file owns everything the block phase sets on a node: type, level,
// literal (code_block/html_block), info/isFenced, listData (including the
// final `tight`), sourcepos, and `stringContent` — the raw, unparsed text of
// paragraphs and headings that the inline phase consumes.

import { MdNode } from './node.ts'
import type { ListData, NodeType, SourcePos, TableAlign } from './node.ts'
import { unescapeString } from './common.ts'
import { parseReference } from './inline.ts'
import type { ParserOptions, RefMap } from './options.ts'

/** §4.4: four columns of indentation open an indented code block. */
const CODE_INDENT = 4

const C_TAB = 9
const C_SPACE = 32
const C_GREATERTHAN = 62
const C_LESSTHAN = 60
const C_OPEN_BRACKET = 91
const C_BACKSLASH = 92
const C_PIPE = 124

/** §2.1: \r\n, \n and a lone \r are all line endings. */
const RE_LINE_ENDING = /\r\n|\n|\r/

/** §4.1: 3+ matching `*`, `_` or `-`, each optionally followed by spaces/tabs. */
const RE_THEMATIC_BREAK = /^(?:\*[ \t]*){3,}$|^(?:_[ \t]*){3,}$|^(?:-[ \t]*){3,}$/

/** Fast reject: every block start begins with one of these characters. */
const RE_MAYBE_SPECIAL = /^[#`~*+_=<>0-9-]/

/**
 * The same fast reject with `|` and `:` added, used when `gfm` is on: a GFM
 * table's delimiter row may begin with either (`| --- |`, `:-: | ---:`), and
 * neither is a CommonMark block start.
 *
 * Kept as a separate constant rather than folded into the one above so that a
 * `gfm:false` parse takes exactly the path it always did — same characters
 * rejected, same lines never offered to the block starts at all.
 */
const RE_MAYBE_SPECIAL_GFM = /^[#`~*+_=<>0-9:|-]/

const RE_NON_SPACE = /[^ \t\f\v\r\n]/

/** §5.2 list markers. */
const RE_BULLET_LIST_MARKER = /^[*+-]/
const RE_ORDERED_LIST_MARKER = /^(\d{1,9})([.)])/

/** §4.2: 1–6 `#` followed by a space, a tab, or the end of the line. */
const RE_ATX_HEADING_MARKER = /^#{1,6}(?:[ \t]+|$)/

/** §4.5: an info string after a backtick fence may not contain a backtick. */
const RE_CODE_FENCE = /^`{3,}(?!.*`)|^~{3,}/
const RE_CLOSING_CODE_FENCE = /^(?:`{3,}|~{3,})(?=[ \t]*$)/

/** §4.3: a run of `=` or `-` with only spaces/tabs after it. */
const RE_SETEXT_HEADING_LINE = /^(?:=+|-+)[ \t]*$/

// --- HTML blocks (§4.6) -----------------------------------------------------
//
// Seven kinds, each with its own start and end condition. Types 1–5 end on a
// line *containing* their closing string (checked in `incorporateLine` after
// the line is added); types 6 and 7 end at a blank line (checked in the
// html_block continuation condition). Type 7 alone may not interrupt a
// paragraph.

/** Tag-shape fragments shared by HTML block type 7 (§6.6 raw HTML). */
const TAGNAME = '[A-Za-z][A-Za-z0-9-]*'
const ATTRIBUTENAME = '[a-zA-Z_:][a-zA-Z0-9:._-]*'
const UNQUOTEDVALUE = "[^\"'=<>`\\x00-\\x20]+"
const SINGLEQUOTEDVALUE = "'[^']*'"
const DOUBLEQUOTEDVALUE = '"[^"]*"'
const ATTRIBUTEVALUE =
  '(?:' + UNQUOTEDVALUE + '|' + SINGLEQUOTEDVALUE + '|' + DOUBLEQUOTEDVALUE + ')'
const ATTRIBUTEVALUESPEC = '(?:[ \\t]*=[ \\t]*' + ATTRIBUTEVALUE + ')'
const ATTRIBUTE = '(?:[ \\t]+' + ATTRIBUTENAME + ATTRIBUTEVALUESPEC + '?)'
const OPENTAG = '<' + TAGNAME + ATTRIBUTE + '*[ \\t]*/?>'
const CLOSETAG = '</' + TAGNAME + '[ \\t]*>'

/** The type 6 tag list, verbatim from §4.6 (0.31.2 has `search`, not `source`). */
const BLOCK_TAGS = [
  'address',
  'article',
  'aside',
  'base',
  'basefont',
  'blockquote',
  'body',
  'caption',
  'center',
  'col',
  'colgroup',
  'dd',
  'details',
  'dialog',
  'dir',
  'div',
  'dl',
  'dt',
  'fieldset',
  'figcaption',
  'figure',
  'footer',
  'form',
  'frame',
  'frameset',
  'h[1-6]',
  'head',
  'header',
  'hr',
  'html',
  'iframe',
  'legend',
  'li',
  'link',
  'main',
  'menu',
  'menuitem',
  'nav',
  'noframes',
  'ol',
  'optgroup',
  'option',
  'p',
  'param',
  'search',
  'section',
  'summary',
  'table',
  'tbody',
  'td',
  'tfoot',
  'th',
  'thead',
  'title',
  'tr',
  'track',
  'ul',
].join('|')

/** Indexed by HTML block type 1–7; slot 0 is unused. */
const RE_HTML_BLOCK_OPEN: RegExp[] = [
  /(?:)/,
  /^<(?:script|pre|textarea|style)(?:[ \t]|>|$)/i,
  /^<!--/,
  /^<[?]/,
  /^<![A-Za-z]/,
  /^<!\[CDATA\[/,
  new RegExp('^</?(?:' + BLOCK_TAGS + ')(?:[ \\t]|/?>|$)', 'i'),
  new RegExp('^(?:' + OPENTAG + '|' + CLOSETAG + ')[ \\t]*$', 'i'),
]

/** End conditions for types 1–5; types 6 and 7 end at a blank line instead. */
const RE_HTML_BLOCK_CLOSE: RegExp[] = [
  /(?:)/,
  /<\/(?:script|pre|textarea|style)>/i,
  /-->/,
  /\?>/,
  />/,
  /\]\]>/,
]

// --- small lexical helpers --------------------------------------------------

/** Character code at `pos`, or -1 past the end of the line. */
function peek(ln: string, pos: number): number {
  return 0 <= pos && pos < ln.length ? ln.charCodeAt(pos) : -1
}

/**
 * Number of characters (code points) in `s`. `s.length` counts UTF-16 units,
 * which double-counts astral characters; sourcepos columns count characters.
 */
function charCount(s: string): number {
  let n = 0
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    // Skip the low half of a surrogate pair; a lone surrogate still counts once.
    if (0xd800 <= c && c <= 0xdbff && i + 1 < s.length) {
      const d = s.charCodeAt(i + 1)
      if (0xdc00 <= d && d <= 0xdfff) i++
    }
    n++
  }
  return n
}

function isSpaceOrTab(c: number): boolean {
  return C_SPACE === c || C_TAB === c
}

function isBlank(s: string): boolean {
  return !RE_NON_SPACE.test(s)
}

// --- GFM tables (extension) -------------------------------------------------
//
// Everything the extension needs from a line is here: how a row splits into
// cells, and whether a line is a delimiter row. Both are pure functions of one
// line of text, which is what lets the block start below stay a few lines
// long.

/** ASCII whitespace, the set `cmark_isspace` trims from a table cell. */
function isAsciiSpace(c: number): boolean {
  return 32 === c || (9 <= c && c <= 13)
}

function trimAsciiSpace(s: string): string {
  let start = 0
  let end = s.length
  while (start < end && isAsciiSpace(s.charCodeAt(start))) start++
  while (end > start && isAsciiSpace(s.charCodeAt(end - 1))) end--
  return 0 === start && end === s.length ? s : s.slice(start, end)
}

/**
 * Split one row of a table into its cell texts.
 *
 * Cells are separated by *unescaped* pipes; a leading and a trailing pipe are
 * both optional and may differ from row to row, so they are stripped before
 * the split rather than producing empty cells at the ends. A trailing pipe is
 * only a delimiter if it is itself unescaped, which is why the backslash run
 * before it is counted.
 *
 * `\|` is resolved to a literal `|` *here*, as the extension requires ("include
 * a pipe in a cell's content by escaping it, including inside other inline
 * spans"). Doing it at split time is the whole point: the inline phase never
 * sees the backslash, so a code span in the cell yields a real pipe —
 * `` b `\|` az `` is `b <code>|</code> az`, which no amount of unescaping
 * *after* inline parsing could produce. Every other backslash escape is left
 * exactly as written for the inline phase to handle in the usual way, so no
 * cell text is unescaped twice.
 *
 * Scanning skips the character after any backslash, so `\\` cannot shield the
 * pipe that follows it: `a\\|b` is two cells, the first holding `a\\`.
 *
 * A row of nothing but whitespace has no cells at all, which is how a header
 * candidate that is really empty fails the cell-count test instead of matching
 * a one-column delimiter row.
 */
function splitTableRow(line: string): string[] {
  let start = 0
  let end = line.length
  while (start < end && isAsciiSpace(line.charCodeAt(start))) start++
  while (end > start && isAsciiSpace(line.charCodeAt(end - 1))) end--

  if (start === end) return []

  // Optional leading pipe.
  if (C_PIPE === line.charCodeAt(start)) start++

  // Optional trailing pipe — but an escaped one is content, not a delimiter.
  if (start < end && C_PIPE === line.charCodeAt(end - 1)) {
    let bs = end - 2
    while (start <= bs && C_BACKSLASH === line.charCodeAt(bs)) bs--
    // An even-length backslash run leaves the pipe unescaped.
    if (0 === (end - 2 - bs) % 2) end--
  }

  const cells: string[] = []
  // `cur` holds the parts of the current cell that have already been rewritten;
  // `seg` is the start of the run since then, so the common case (no escapes)
  // costs one slice per cell rather than one per character.
  let cur = ''
  let seg = start
  let i = start

  while (i < end) {
    const c = line.charCodeAt(i)
    if (C_BACKSLASH === c && i + 1 < end) {
      if (C_PIPE === line.charCodeAt(i + 1)) {
        cur += line.slice(seg, i) + '|'
        i += 2
        seg = i
      } else {
        i += 2
      }
      continue
    }
    if (C_PIPE === c) {
      cells.push(trimAsciiSpace(cur + line.slice(seg, i)))
      cur = ''
      i++
      seg = i
      continue
    }
    i++
  }
  cells.push(trimAsciiSpace(cur + line.slice(seg, end)))

  return cells
}

/**
 * A delimiter cell is hyphens with an optional leading and/or trailing colon,
 * and nothing else. One `+`, one space in the middle, one stray character and
 * the line is not a delimiter row.
 */
const RE_TABLE_DELIMITER_CELL = /^(:?)-+(:?)$/

/**
 * Read a line as a table's delimiter row, returning one alignment per column,
 * or null if it is not one.
 */
function parseDelimiterRow(line: string): TableAlign[] | null {
  const cells = splitTableRow(line)
  if (0 === cells.length) return null

  const align: TableAlign[] = []
  for (const cell of cells) {
    const m = RE_TABLE_DELIMITER_CELL.exec(cell)
    if (null === m) return null
    const left = ':' === m[1]
    const right = ':' === m[2]
    align.push(left && right ? 'center' : left ? 'left' : right ? 'right' : null)
  }
  return align
}

/**
 * Cap on the empty cells inserted to pad short body rows, across a whole
 * document — cmark-gfm's `MAX_AUTOCOMPLETED_CELLS`, and for the same reason.
 *
 * Padding is the one place where a table's node count is not bounded by the
 * size of its source: a 10000-column header followed by 10000 one-cell rows is
 * 60 KB of input asking for 10^8 cells. Every other part of the extension is
 * linear in the input, so this single budget is what keeps the whole of it so.
 * Reaching it needs input that is already pathological; ordinary tables are
 * nowhere near.
 */
const MAX_AUTOCOMPLETED_CELLS = 0x80000

/** §5.3: two markers make the same list only if type, bullet and delimiter agree. */
function listsMatch(listData: ListData, itemData: ListData): boolean {
  return (
    listData.type === itemData.type &&
    listData.delimiter === itemData.delimiter &&
    listData.bulletChar === itemData.bulletChar
  )
}

/**
 * §5.3 looseness input: does this block end with a blank line? For lists and
 * items the question recurses into the last child, since the blank line is
 * recorded on the deepest block that saw it. The `lastLineChecked` flag makes
 * the recursion linear over the whole document rather than quadratic.
 */
function endsWithBlankLine(block: MdNode | null): boolean {
  let cur = block
  while (cur) {
    if (cur.lastLineBlank) return true
    const t = cur.type
    if (!cur.lastLineChecked && ('list' === t || 'item' === t)) {
      cur.lastLineChecked = true
      cur = cur.lastChild
    } else {
      cur.lastLineChecked = true
      return false
    }
  }
  return false
}

// --- per-block behaviour ----------------------------------------------------

/**
 * `continue` result: 0 = matched, 1 = not matched, 2 = line fully consumed
 * (fenced code close), stop processing this line entirely.
 */
type ContinueResult = 0 | 1 | 2

type BlockHandler = {
  /** Can this block absorb the current line? */
  continue: (parser: BlockParser, block: MdNode) => ContinueResult
  /** Called once when the block is closed. */
  finalize: (parser: BlockParser, block: MdNode) => void
  canContain: (t: NodeType) => boolean
  /** Leaves that accumulate raw text: code blocks, HTML blocks, paragraphs, tables. */
  acceptsLines: boolean
  /**
   * Lines belonging to this block are literal text, so Appendix A step 2 does
   * not look for block starts while it is the last matched container.
   *
   * True for exactly the two blocks the spec names — code blocks and HTML
   * blocks. Paragraphs and GFM tables also accept lines, but a block start may
   * still interrupt either: that is how `> bar` ends a table, and how `# h`
   * ends a paragraph.
   */
  verbatim: boolean
}

const notItem = (t: NodeType): boolean => 'item' !== t
const noChildren = (): boolean => false

const BLOCK_HANDLERS: Record<string, BlockHandler> = {
  document: {
    continue: () => 0,
    finalize: () => undefined,
    canContain: notItem,
    acceptsLines: false,
    verbatim: false,
  },

  list: {
    continue: () => 0,
    finalize: (_parser, block) => {
      // §5.3: the list is loose if any item is followed by a blank line, or if
      // any item directly contains two blocks separated by one. A blank line
      // at the very end of the last item does not count — hence the
      // `item.next` / `subitem.next` guards.
      let item = block.firstChild
      while (item) {
        if (endsWithBlankLine(item) && item.next) {
          if (block.listData) block.listData.tight = false
          break
        }
        let subitem = item.firstChild
        while (subitem) {
          if (endsWithBlankLine(subitem) && (item.next || subitem.next)) {
            if (block.listData) block.listData.tight = false
            break
          }
          subitem = subitem.next
        }
        item = item.next
      }
      if (block.lastChild) block.sourcepos[1] = block.lastChild.sourcepos[1]
    },
    canContain: (t) => 'item' === t,
    acceptsLines: false,
    verbatim: false,
  },

  block_quote: {
    continue: (parser) => {
      const ln = parser.currentLine
      if (!parser.indented && C_GREATERTHAN === peek(ln, parser.nextNonspace)) {
        parser.advanceNextNonspace()
        parser.advanceOffset(1, false)
        // §5.1: one optional space of indentation after `>`, tab-aware.
        if (isSpaceOrTab(peek(ln, parser.offset))) parser.advanceOffset(1, true)
        return 0
      }
      return 1
    },
    finalize: () => undefined,
    canContain: notItem,
    acceptsLines: false,
    verbatim: false,
  },

  item: {
    continue: (parser, container) => {
      const data = container.listData
      if (null === data) return 1

      if (parser.blank) {
        // An item whose first line is blank cannot be continued by a second
        // blank line (§5.2 rule 3: at most one leading blank line).
        if (null === container.firstChild) return 1
        parser.advanceNextNonspace()
        return 0
      }

      // §5.2: subsequent lines belong to the item if indented to the content
      // column derived from the marker.
      if (parser.indent >= data.markerOffset + data.padding) {
        parser.advanceOffset(data.markerOffset + data.padding, true)
        return 0
      }
      return 1
    },
    finalize: (_parser, block) => {
      if (block.lastChild) {
        block.sourcepos[1] = block.lastChild.sourcepos[1]
      } else if (block.listData) {
        block.sourcepos[1] = [
          block.sourcepos[0][0],
          block.listData.markerOffset + block.listData.padding,
        ]
      }
    },
    canContain: notItem,
    acceptsLines: false,
    verbatim: false,
  },

  heading: {
    // A heading is always exactly one line (setext headings are converted from
    // an already-complete paragraph).
    continue: () => 1,
    finalize: () => undefined,
    canContain: noChildren,
    acceptsLines: false,
    verbatim: false,
  },

  thematic_break: {
    continue: () => 1,
    finalize: () => undefined,
    canContain: noChildren,
    acceptsLines: false,
    verbatim: false,
  },

  code_block: {
    continue: (parser, container) => {
      const ln = parser.currentLine
      const indent = parser.indent

      if (container.isFenced) {
        // §4.5: the closing fence is the same character, at least as long, at
        // most 3 columns indented, and followed only by spaces/tabs.
        const rest = ln.slice(parser.nextNonspace)
        const match =
          indent <= 3 && ln.charAt(parser.nextNonspace) === container.fenceChar
            ? rest.match(RE_CLOSING_CODE_FENCE)
            : null
        if (match && match[0].length >= container.fenceLength) {
          parser.lastLineLength = charCount(ln.slice(0, parser.offset + indent + match[0].length))
          parser.finalize(container, parser.lineNumber)
          return 2
        }
        // Content is de-indented by up to the opening fence's own indent.
        let i = container.fenceOffset
        while (0 < i && isSpaceOrTab(peek(ln, parser.offset))) {
          parser.advanceOffset(1, true)
          i--
        }
        return 0
      }

      // §4.4 indented code: four columns, or a blank line (which is kept).
      if (indent >= CODE_INDENT) {
        parser.advanceOffset(CODE_INDENT, true)
        return 0
      }
      if (parser.blank) {
        parser.advanceNextNonspace()
        return 0
      }
      return 1
    },
    finalize: (_parser, block) => {
      if (block.isFenced) {
        // The first accumulated line is the info string, not content.
        const content = block.stringContent
        const nl = content.indexOf('\n')
        const firstLine = -1 === nl ? content : content.slice(0, nl)
        block.info = unescapeString(firstLine.trim())
        block.literal = -1 === nl ? '' : content.slice(nl + 1)
      } else {
        // §4.4: trailing blank lines are not part of an indented code block.
        block.literal = block.stringContent.replace(/(\n *)+$/, '\n')
      }
      block.stringContent = ''
    },
    canContain: noChildren,
    acceptsLines: true,
    verbatim: true,
  },

  html_block: {
    continue: (parser, container) => {
      // Types 6 and 7 end at a blank line; 1–5 end on their closing string,
      // which `incorporateLine` checks after adding the line.
      const type = parser.htmlBlockType(container)
      return parser.blank && (6 === type || 7 === type) ? 1 : 0
    },
    finalize: (_parser, block) => {
      block.literal = block.stringContent.replace(/(\n *)+$/, '')
      block.stringContent = ''
    },
    canContain: noChildren,
    acceptsLines: true,
    verbatim: true,
  },

  paragraph: {
    continue: (parser) => (parser.blank ? 1 : 0),
    finalize: (parser, block) => {
      // §4.7: a paragraph may begin with link reference definitions. Consume
      // them from the front; if nothing is left, the paragraph disappears.
      let hasReferenceDefs = false
      let pos = 0
      while (
        C_OPEN_BRACKET === block.stringContent.charCodeAt(0) &&
        0 !== (pos = parseReference(block.stringContent, parser.refmap))
      ) {
        block.stringContent = block.stringContent.slice(pos)
        hasReferenceDefs = true
      }
      if (hasReferenceDefs && isBlank(block.stringContent)) block.unlink()
    },
    canContain: noChildren,
    acceptsLines: true,
    verbatim: false,
  },

  // GFM tables. The header row is built when the table opens (see
  // `tryOpenTable`); every later line is accumulated raw and split into a body
  // row at finalize, so the table behaves like a paragraph that happens to
  // print as a grid — it ends at a blank line, and any block start interrupts
  // it, because `verbatim` is false and `canContain` refuses everything the
  // block starts can open.
  table: {
    continue: (parser) => (parser.blank ? 1 : 0),
    finalize: (parser, block) => finalizeTable(parser, block),
    canContain: (t) => 'table_row' === t,
    acceptsLines: true,
    verbatim: false,
  },

  // Rows and cells are created closed, and are never the parser's tip, so
  // nothing here is reached in a normal parse. They exist so that `handlerFor`
  // — which throws for an unknown type — cannot be the way a malformed tree
  // surfaces.
  table_row: {
    continue: () => 1,
    finalize: () => undefined,
    canContain: (t) => 'table_cell' === t,
    acceptsLines: false,
    verbatim: false,
  },

  table_cell: {
    continue: () => 1,
    finalize: () => undefined,
    canContain: noChildren,
    acceptsLines: false,
    verbatim: false,
  },
}

/**
 * Append one row of `cells` to `table`, padded with empty cells or truncated
 * so that it is exactly as wide as the delimiter row said.
 *
 * Rows and cells are built closed: they are not part of the open spine, and
 * the line walk in `incorporateLine` must stop at the table itself.
 */
function addTableRow(
  parser: BlockParser,
  table: MdNode,
  cells: string[],
  isHeader: boolean,
  lineNumber: number,
  endColumn: number,
): void {
  const width = table.tableAlign?.length ?? 0

  // See MAX_AUTOCOMPLETED_CELLS: padding is the only unbounded part of a table.
  let limit = width
  if (cells.length < width) {
    const budget = MAX_AUTOCOMPLETED_CELLS - parser.autocompletedCells
    const padding = width - cells.length
    if (padding > budget) limit = cells.length + (0 < budget ? budget : 0)
    parser.autocompletedCells += limit - cells.length
  }

  // A row and its cells occupy one source line. Columns within that line are
  // not tracked: nothing reads them (the AST drops sourcepos and the renderer
  // ignores it), and threading offsets through the cell split to invent them
  // would be cost with no reader. Each node needs its own array — sourcepos is
  // mutable — so the span is rebuilt rather than shared.
  const span = (): SourcePos => [
    [lineNumber, 1],
    [lineNumber, endColumn],
  ]

  const row = new MdNode('table_row', span())
  row.isHeaderRow = isHeader
  row.open = false
  table.appendChild(row)

  for (let i = 0; i < limit; i++) {
    const cell = new MdNode('table_cell', span())
    cell.open = false
    // Consumed by the inline phase, exactly as a paragraph's content is.
    cell.stringContent = i < cells.length ? cells[i] : ''
    row.appendChild(cell)
  }
}

/**
 * Close a table: turn the raw lines it accumulated into body rows.
 *
 * The first accumulated line is the delimiter row that opened the table. It
 * carries no data — its alignments are already on the node — so it is skipped
 * here, which is also what keeps the source line numbering of the rows below
 * it straightforward: the table accepts exactly one line per source line, and
 * a blank line closes it, so there are no gaps.
 */
function finalizeTable(parser: BlockParser, table: MdNode): void {
  const lines = table.stringContent.split('\n')
  table.stringContent = ''

  // Line 0 is the delimiter row; the last element is the empty string after
  // the final newline.
  const headerLine = table.sourcepos[0][0]
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i]
    if ('' === line) continue
    addTableRow(parser, table, splitTableRow(line), false, headerLine + i + 1, charCount(line))
  }
}

function handlerFor(type: NodeType): BlockHandler {
  const handler: BlockHandler | undefined = BLOCK_HANDLERS[type]
  if (undefined === handler) throw new Error('markdown: no block handler for ' + type)
  return handler
}

// --- list markers (§5.2) ----------------------------------------------------

/**
 * Parse a list marker at `nextNonspace` and derive the item's content indent.
 *
 * The content indent is marker width + the spaces that follow it, with two
 * exceptions that both fall back to marker width + 1:
 *
 *   - rule 2, item starting with indented code: 5+ spaces after the marker
 *     means only the first space belongs to the marker, the rest is code;
 *   - rule 3, item starting with a blank line: nothing follows the marker at
 *     all, so there are no spaces to measure.
 *
 * Returns null when this is not a list item start. Advances the parser to the
 * item's content column when it is.
 */
function parseListMarker(parser: BlockParser, container: MdNode): ListData | null {
  // Four columns of indentation is indented code, never a list marker.
  if (parser.indent >= CODE_INDENT) return null

  const rest = parser.currentLine.slice(parser.nextNonspace)

  const data: ListData = {
    type: 'bullet',
    tight: true, // lists are tight until finalize proves otherwise
    start: 1,
    delimiter: '',
    bulletChar: '',
    padding: 0,
    markerOffset: parser.indent,
  }

  let match = rest.match(RE_BULLET_LIST_MARKER)
  if (match) {
    data.type = 'bullet'
    data.bulletChar = match[0][0]
  } else {
    match = rest.match(RE_ORDERED_LIST_MARKER)
    // §5.2 exception 1(b): an ordered item interrupting a paragraph must start at 1.
    if (null === match || ('paragraph' === container.type && 1 !== parseInt(match[1], 10))) {
      return null
    }
    data.type = 'ordered'
    data.start = parseInt(match[1], 10)
    data.delimiter = match[2]
  }

  // At least one space or tab (or the end of the line) must follow the marker.
  let nextc = peek(parser.currentLine, parser.nextNonspace + match[0].length)
  if (!(-1 === nextc || C_TAB === nextc || C_SPACE === nextc)) return null

  // §5.2 exception 1(a): an item interrupting a paragraph may not start blank.
  if (
    'paragraph' === container.type &&
    !RE_NON_SPACE.test(parser.currentLine.slice(parser.nextNonspace + match[0].length))
  ) {
    return null
  }

  parser.advanceNextNonspace() // to the marker
  parser.advanceOffset(match[0].length, true) // past the marker

  const spacesStartCol = parser.column
  const spacesStartOffset = parser.offset
  do {
    parser.advanceOffset(1, true)
    nextc = peek(parser.currentLine, parser.offset)
  } while (parser.column - spacesStartCol < 5 && isSpaceOrTab(nextc))

  const blankItem = -1 === peek(parser.currentLine, parser.offset)
  const spacesAfterMarker = parser.column - spacesStartCol

  if (spacesAfterMarker >= 5 || spacesAfterMarker < 1 || blankItem) {
    data.padding = match[0].length + 1
    parser.column = spacesStartCol
    parser.offset = spacesStartOffset
    if (isSpaceOrTab(peek(parser.currentLine, parser.offset))) parser.advanceOffset(1, true)
  } else {
    data.padding = match[0].length + spacesAfterMarker
  }

  return data
}

// --- block starts -----------------------------------------------------------

/** 0 = no match, 1 = container started, 2 = leaf started (stop looking). */
type StartResult = 0 | 1 | 2

type BlockStart = (parser: BlockParser, container: MdNode) => StartResult

/**
 * Tried in order, and the order is the spec's: a setext underline beats a
 * thematic break (so `Foo\n---` is a heading), a thematic break beats a list
 * item (so `- - -` is a break), and indented code comes last because any of
 * the others may be indented up to three columns.
 */
const BLOCK_STARTS: BlockStart[] = [
  // Block quote (§5.1).
  (parser) => {
    if (!parser.indented && C_GREATERTHAN === peek(parser.currentLine, parser.nextNonspace)) {
      parser.advanceNextNonspace()
      parser.advanceOffset(1, false)
      if (isSpaceOrTab(peek(parser.currentLine, parser.offset))) parser.advanceOffset(1, true)
      parser.closeUnmatchedBlocks()
      parser.addChild('block_quote', parser.nextNonspace)
      return 1
    }
    return 0
  },

  // ATX heading (§4.2).
  (parser) => {
    if (parser.indented) return 0
    const match = parser.currentLine.slice(parser.nextNonspace).match(RE_ATX_HEADING_MARKER)
    if (null === match) return 0

    parser.advanceNextNonspace()
    parser.advanceOffset(match[0].length, false)
    parser.closeUnmatchedBlocks()

    const container = parser.addChild('heading', parser.nextNonspace)
    container.level = match[0].replace(/[ \t]+$/, '').length
    // An optional closing sequence of `#`s, preceded by spaces or tabs, is
    // dropped; a line of nothing but `#`s has empty content.
    container.stringContent = parser.currentLine
      .slice(parser.offset)
      .replace(/^[ \t]*#+[ \t]*$/, '')
      .replace(/[ \t]+#+[ \t]*$/, '')
    parser.advanceOffset(parser.currentLine.length - parser.offset, false)
    return 2
  },

  // Fenced code block (§4.5).
  (parser) => {
    if (parser.indented) return 0
    const match = parser.currentLine.slice(parser.nextNonspace).match(RE_CODE_FENCE)
    if (null === match) return 0

    const fenceLength = match[0].length
    parser.closeUnmatchedBlocks()
    const container = parser.addChild('code_block', parser.nextNonspace)
    container.isFenced = true
    container.fenceLength = fenceLength
    container.fenceChar = match[0][0]
    container.fenceOffset = parser.indent
    parser.advanceNextNonspace()
    parser.advanceOffset(fenceLength, false)
    // The rest of the line is added as the block's first line: the info string.
    return 2
  },

  // HTML block (§4.6), types 1-7.
  (parser, container) => {
    if (parser.indented || C_LESSTHAN !== peek(parser.currentLine, parser.nextNonspace)) return 0

    const s = parser.currentLine.slice(parser.nextNonspace)
    for (let blockType = 1; blockType <= 7; blockType++) {
      if (!RE_HTML_BLOCK_OPEN[blockType].test(s)) continue
      // Type 7 may not interrupt a paragraph — including a paragraph we are
      // only about to continue lazily.
      if (
        7 === blockType &&
        ('paragraph' === container.type ||
          (!parser.allClosed && !parser.blank && 'paragraph' === parser.openTip.type))
      ) {
        continue
      }
      parser.closeUnmatchedBlocks()
      // The offset is deliberately not advanced: leading spaces are part of
      // the raw HTML.
      const block = parser.addChild('html_block', parser.offset)
      parser.setHtmlBlockType(block, blockType)
      return 2
    }
    return 0
  },

  // Setext heading underline (§4.3) — converts the whole open paragraph.
  (parser, container) => {
    if (parser.indented || 'paragraph' !== container.type) return 0
    const match = parser.currentLine.slice(parser.nextNonspace).match(RE_SETEXT_HEADING_LINE)
    if (null === match) return 0

    parser.closeUnmatchedBlocks()

    // §4.7: the paragraph may still have led with reference definitions; only
    // what remains becomes the heading. If nothing remains this is not a
    // setext underline at all (it falls through to thematic break / paragraph).
    let pos = 0
    while (
      C_OPEN_BRACKET === container.stringContent.charCodeAt(0) &&
      0 !== (pos = parseReference(container.stringContent, parser.refmap))
    ) {
      container.stringContent = container.stringContent.slice(pos)
    }
    if (0 === container.stringContent.length) return 0

    const start = container.sourcepos[0]
    const heading = new MdNode('heading', [[start[0], start[1]], [0, 0]])
    heading.level = '=' === match[0][0] ? 1 : 2
    heading.stringContent = container.stringContent
    container.insertAfter(heading)
    container.unlink()
    parser.tip = heading
    parser.advanceOffset(parser.currentLine.length - parser.offset, false)
    return 2
  },

  // Thematic break (§4.1).
  (parser) => {
    if (parser.indented) return 0
    if (!RE_THEMATIC_BREAK.test(parser.currentLine.slice(parser.nextNonspace))) return 0
    parser.closeUnmatchedBlocks()
    parser.addChild('thematic_break', parser.nextNonspace)
    parser.advanceOffset(parser.currentLine.length - parser.offset, false)
    return 2
  },

  // List item (§5.2). `parseListMarker` rejects a marker indented four or more
  // columns on its own, so there is no `indented` test here.
  (parser, container) => {
    const data = parseListMarker(parser, container)
    if (null === data) return 0

    parser.closeUnmatchedBlocks()

    // §5.3: a change of bullet character or ordered delimiter starts a new list.
    const tip = parser.openTip
    if ('list' !== tip.type || null === tip.listData || !listsMatch(tip.listData, data)) {
      parser.addChild('list', parser.nextNonspace).listData = data
    }

    parser.addChild('item', parser.nextNonspace).listData = data
    return 1
  },

  // Indented code block (§4.4) — may not interrupt a paragraph.
  (parser) => {
    if (!parser.indented || 'paragraph' === parser.openTip.type || parser.blank) return 0
    parser.advanceOffset(CODE_INDENT, true)
    parser.closeUnmatchedBlocks()
    parser.addChild('code_block', parser.offset)
    return 2
  },

  // GFM table (extension) — a delimiter row directly under an open paragraph.
  //
  // Last, and the position is load-bearing in both directions. It must come
  // after the setext underline so that `foo` over `---` stays an `<h2>` rather
  // than becoming a one-column table, and after the list marker so that `- | -`
  // stays a list item; cmark-gfm reaches its extensions from the same place,
  // once every built-in start has refused the line.
  (parser, container) => (parser.tryOpenTable(container) ? 2 : 0),
]

// --- the parser -------------------------------------------------------------

class BlockParser {
  options: ParserOptions
  refmap: RefMap = {}

  doc: MdNode
  /** Deepest open block; null only once the document itself is finalized. */
  tip: MdNode | null
  /** The tip as it was before the current line was walked. */
  oldtip: MdNode
  lastMatchedContainer: MdNode
  /** False while some open block failed its continuation condition. */
  allClosed = true

  currentLine = ''
  lineNumber = 0
  lastLineLength = 0

  // Memoize the last offset in `currentLine` whose character count is known,
  // so sourcepos columns cost amortized O(1). Both reset per line.
  // See `sourceColumn`.
  colAnchorOffset = 0
  colAnchorChars = 0

  /** UTF-16 index into `currentLine`. */
  offset = 0
  /** Tab-expanded display column matching `offset` (§2.2). */
  column = 0
  /** True when `offset` sits inside a tab whose columns were partly consumed. */
  partiallyConsumedTab = false

  nextNonspace = 0
  nextNonspaceColumn = 0
  /** Columns between `column` and the next non-space character. */
  indent = 0
  indented = false
  blank = false

  /** Empty cells inserted so far to pad short table rows. See MAX_AUTOCOMPLETED_CELLS. */
  autocompletedCells = 0

  /**
   * The text `addLine` last appended, and the block it went to.
   *
   * `tryOpenTable` needs the *last line* of an open paragraph, and reading it
   * back out of `stringContent` is not affordable: that content is built by
   * repeated `+=`, so every index into it flattens a fresh rope — O(n) per
   * line, O(n²) over a paragraph whose every other line looks like a delimiter
   * row, which untrusted input can write. Keeping the line itself here costs
   * two field writes and never touches the accumulated string.
   *
   * The node is recorded alongside it because "the previous line went to this
   * very paragraph" is the extension's own rule, not a cache-validity trick:
   * a header row is by definition the line immediately above the delimiter
   * row. If some other block took that line, there is no header row.
   */
  lastAddedLine = ''
  lastAddedTo: MdNode | null = null

  /** Step 2's fast reject, widened for a table's delimiter row when `gfm` is on. */
  readonly maybeSpecial: RegExp

  // MdNode has no field for the HTML block type, and the tree contract is not
  // ours to change, so open HTML blocks carry it here.
  private htmlBlockTypes = new Map<MdNode, number>()

  constructor(options: ParserOptions) {
    this.options = options
    this.maybeSpecial = options.gfm ? RE_MAYBE_SPECIAL_GFM : RE_MAYBE_SPECIAL
    this.doc = new MdNode('document', [
      [1, 1],
      [0, 0],
    ])
    this.tip = this.doc
    this.oldtip = this.doc
    this.lastMatchedContainer = this.doc
  }

  /** The deepest open block. Only valid while a line is being processed. */
  get openTip(): MdNode {
    if (null === this.tip) throw new Error('markdown: no open block')
    return this.tip
  }

  htmlBlockType(block: MdNode): number {
    return this.htmlBlockTypes.get(block) ?? 0
  }

  setHtmlBlockType(block: MdNode, type: number): void {
    this.htmlBlockTypes.set(block, type)
  }

  /**
   * Advance `count` characters (`columns` false) or `count` columns (`columns`
   * true), expanding tabs to the next 4-column tab stop (§2.2). A tab whose
   * columns are only partly consumed leaves `offset` on the tab itself and
   * sets `partiallyConsumedTab`, so `addLine` can re-emit the remainder as
   * spaces.
   */
  advanceOffset(count: number, columns: boolean): void {
    const ln = this.currentLine
    while (0 < count && this.offset < ln.length) {
      const c = ln.charAt(this.offset)
      if ('\t' === c) {
        const charsToTab = 4 - (this.column % 4)
        if (columns) {
          this.partiallyConsumedTab = charsToTab > count
          const charsToAdvance = charsToTab > count ? count : charsToTab
          this.column += charsToAdvance
          this.offset += this.partiallyConsumedTab ? 0 : 1
          count -= charsToAdvance
        } else {
          this.partiallyConsumedTab = false
          this.column += charsToTab
          this.offset += 1
          count -= 1
        }
      } else {
        this.partiallyConsumedTab = false
        this.offset += 1
        this.column += 1 // block markers are ASCII, so one char is one column
        count -= 1
      }
    }
  }

  advanceNextNonspace(): void {
    this.offset = this.nextNonspace
    this.column = this.nextNonspaceColumn
    this.partiallyConsumedTab = false
  }

  /** Locate the next non-space character and the indent leading up to it. */
  findNextNonspace(): void {
    const ln = this.currentLine
    let i = this.offset
    let cols = this.column
    let c = ''

    for (;;) {
      c = ln.charAt(i)
      if (' ' === c) {
        i++
        cols++
      } else if ('\t' === c) {
        i++
        cols += 4 - (cols % 4)
      } else {
        break
      }
    }

    this.blank = '' === c || '\n' === c || '\r' === c
    this.nextNonspace = i
    this.nextNonspaceColumn = cols
    this.indent = this.nextNonspaceColumn - this.column
    this.indented = this.indent >= CODE_INDENT
  }

  /** Add the rest of the current line to the tip's raw content. */
  addLine(): void {
    const tip = this.openTip
    let text = ''
    if (this.partiallyConsumedTab) {
      this.offset += 1 // step over the tab
      const charsToTab = 4 - (this.column % 4)
      text = ' '.repeat(charsToTab)
    }
    text += this.currentLine.slice(this.offset)
    tip.stringContent += text + '\n'
    this.lastAddedLine = text
    this.lastAddedTo = tip
  }

  /**
   * Open a block of `tag` as a child of the tip, closing blocks that cannot
   * contain it first.
   */
  /**
   * Characters of `currentLine` before UTF-16 offset `offset` — what a
   * sourcepos column counts.
   *
   * Not simply `offset`: a JavaScript string index counts UTF-16 units, so an
   * astral character (emoji, rare CJK) would advance the column by two. A
   * column means a character, so `\u{1F600} *x*` must report the same columns
   * in both runtimes.
   *
   * Counted incrementally from the last offset already measured. `addChild`
   * runs once per container opened, so counting from the start of the line
   * every time would make a line of n nested containers — `> - > - …` — cost
   * O(n²); untrusted input picks n. Offsets advance monotonically as a line is
   * consumed, making this amortized O(1); a smaller offset than the anchor is
   * still correct, just recounted from the start.
   */
  sourceColumn(offset: number): number {
    if (offset < this.colAnchorOffset) {
      this.colAnchorOffset = 0
      this.colAnchorChars = 0
    }
    this.colAnchorChars += charCount(this.currentLine.slice(this.colAnchorOffset, offset))
    this.colAnchorOffset = offset
    return this.colAnchorChars
  }

  addChild(tag: NodeType, offset: number): MdNode {
    while (!handlerFor(this.openTip.type).canContain(tag)) {
      this.finalize(this.openTip, this.lineNumber - 1)
    }
    const columnNumber = this.sourceColumn(offset) + 1 // offset 0 is column 1
    const node = new MdNode(tag, [
      [this.lineNumber, columnNumber],
      [0, 0],
    ])
    this.openTip.appendChild(node)
    this.tip = node
    return node
  }

  /**
   * GFM tables: try to read the current line as the delimiter row of a table
   * whose header row is the last line of the open paragraph `container`.
   *
   * The two rows must agree on the number of cells; if they do not, there is
   * no table and the line is nothing special (spec example 6 keeps the whole
   * thing a paragraph). The header row is the paragraph's **last** line only,
   * so a paragraph that had earlier lines is split: those lines stay a
   * paragraph — finalized here, which is what still lets them contribute link
   * reference definitions — and the table is opened after it.
   *
   * On success the table is the tip and the caller returns "leaf started", so
   * step 3 adds the delimiter row itself to the table's raw content. It is
   * skipped again by `finalizeTable`; keeping it costs nothing and makes the
   * table's accumulated lines line up one-for-one with the source lines.
   */
  tryOpenTable(container: MdNode): boolean {
    if (!this.options.gfm || this.indented || 'paragraph' !== container.type) return false

    const align = parseDelimiterRow(this.currentLine.slice(this.nextNonspace))
    if (null === align) return false

    // The header row is the line directly above, which is the line `addLine`
    // last wrote — to this paragraph, or there is no header row at all.
    if (this.lastAddedTo !== container) return false
    const headerCells = splitTableRow(this.lastAddedLine)
    if (headerCells.length !== align.length) return false

    this.closeUnmatchedBlocks()

    const headerLine = this.lineNumber - 1
    // `stringContent` ends with that same line plus the newline `addLine` put
    // after it, so the header row starts this far in — no scan needed, and the
    // content is only ever touched on the split branch, at most once per table.
    const content = container.stringContent
    const headerStart = content.length - this.lastAddedLine.length - 1
    if (0 < headerStart) {
      // Split: everything above the header row stays a paragraph.
      container.stringContent = content.slice(0, headerStart)
      this.finalize(container, headerLine - 1)
    } else {
      const parent = container.parent
      container.unlink()
      this.tip = parent
    }

    const table = this.addChild('table', this.nextNonspace)
    // The table begins at the header row, one line above the delimiter row
    // that `addChild` dated it from.
    table.sourcepos[0][0] = headerLine
    table.tableAlign = align

    addTableRow(this, table, headerCells, true, headerLine, this.lastLineLength)
    return true
  }

  /** Close every block that failed its continuation condition on this line. */
  closeUnmatchedBlocks(): void {
    if (this.allClosed) return
    while (this.oldtip !== this.lastMatchedContainer) {
      const parent = this.oldtip.parent
      this.finalize(this.oldtip, this.lineNumber - 1)
      if (null === parent) break
      this.oldtip = parent
    }
    this.allClosed = true
  }

  /** Close a block, record its end position, and run its finalizer. */
  finalize(block: MdNode, lineNumber: number): void {
    const above = block.parent
    block.open = false
    block.sourcepos[1] = [lineNumber, this.lastLineLength]
    handlerFor(block.type).finalize(this, block)
    this.htmlBlockTypes.delete(block)
    this.tip = above
  }

  /**
   * Incorporate one line into the tree — the three steps of Appendix A.
   */
  incorporateLine(ln: string): void {
    let allMatched = true
    let container = this.doc

    this.oldtip = this.openTip
    this.offset = 0
    this.column = 0
    this.blank = false
    this.partiallyConsumedTab = false
    this.lineNumber += 1
    this.currentLine = ln
    // The column anchor memoizes offsets into `currentLine`; a new line
    // invalidates it. See `sourceColumn`.
    this.colAnchorOffset = 0
    this.colAnchorChars = 0

    // Step 1: walk the open blocks, consuming continuation markers. Stop at
    // the first block that does not match; `container` is then the last
    // matched one.
    let lastChild = container.lastChild
    while (lastChild && lastChild.open) {
      container = lastChild
      this.findNextNonspace()

      const result = handlerFor(container.type).continue(this, container)
      if (1 === result) {
        allMatched = false
      } else if (2 === result) {
        // A fenced code block was closed by this line; nothing else to do.
        return
      }

      if (!allMatched) {
        const parent = container.parent
        if (parent) container = parent
        break
      }

      lastChild = container.lastChild
    }

    this.allClosed = container === this.oldtip
    this.lastMatchedContainer = container

    // Step 2: look for new block starts under the last matched container —
    // unless its lines are literal text, in which case there is nothing to
    // look for.
    let matchedLeaf = handlerFor(container.type).verbatim

    while (!matchedLeaf) {
      this.findNextNonspace()

      if (!this.indented && !this.maybeSpecial.test(ln.slice(this.nextNonspace))) {
        this.advanceNextNonspace()
        break
      }

      let i = 0
      while (i < BLOCK_STARTS.length) {
        const result = BLOCK_STARTS[i](this, container)
        if (1 === result) {
          container = this.openTip
          break
        }
        if (2 === result) {
          container = this.openTip
          matchedLeaf = true
          break
        }
        i++
      }

      if (i === BLOCK_STARTS.length) {
        // Nothing matched: the rest of the line is plain text.
        this.advanceNextNonspace()
        break
      }
    }

    // Step 3: whatever is left of the line is text for the deepest open block.
    if (!this.allClosed && !this.blank && 'paragraph' === this.openTip.type) {
      // Lazy continuation (§5.1 rule 2): the markers are missing but an open
      // paragraph absorbs the line anyway, and its containers stay open.
      this.addLine()
      this.lastLineLength = charCount(ln)
      return
    }

    this.closeUnmatchedBlocks()

    if (this.blank && container.lastChild) container.lastChild.lastLineBlank = true

    const t = container.type

    // A block quote line is never blank (it has a `>`), blank lines inside
    // fenced code do not affect list looseness, and an item's own first blank
    // line is part of the item, not a separator.
    const lastLineBlank =
      this.blank &&
      !(
        'block_quote' === t ||
        ('code_block' === t && container.isFenced) ||
        ('item' === t &&
          null === container.firstChild &&
          container.sourcepos[0][0] === this.lineNumber)
      )

    // Propagate up: a container whose last line was blank ends with a blank line.
    let cont: MdNode | null = container
    while (cont) {
      cont.lastLineBlank = lastLineBlank
      cont = cont.parent
    }

    if (handlerFor(t).acceptsLines) {
      this.addLine()
      // §4.6: HTML block types 1-5 end on the line that contains their closing
      // string, which may be the line that opened them.
      if ('html_block' === t) {
        const type = this.htmlBlockType(container)
        if (
          1 <= type &&
          type <= 5 &&
          RE_HTML_BLOCK_CLOSE[type].test(this.currentLine.slice(this.offset))
        ) {
          this.lastLineLength = charCount(ln)
          this.finalize(container, this.lineNumber)
        }
      }
    } else if (this.offset < ln.length && !this.blank) {
      // Nowhere else for the text to go: open a paragraph for it.
      this.addChild('paragraph', this.offset)
      this.advanceNextNonspace()
      this.addLine()
    }

    this.lastLineLength = charCount(ln)
  }

  parse(input: string): { doc: MdNode; refmap: RefMap } {
    // §2.3: insecure characters are replaced before anything else looks at them.
    const text = -1 === input.indexOf('\u0000') ? input : input.replace(/\0/g, '\uFFFD')

    const lines = text.split(RE_LINE_ENDING)
    let len = lines.length
    // A final line ending does not introduce a trailing blank line.
    if (0 < text.length && '' === lines[len - 1]) len -= 1

    for (let i = 0; i < len; i++) {
      this.incorporateLine(lines[i])
    }

    while (this.tip) {
      this.finalize(this.tip, len)
    }

    this.doc.gfm = this.options.gfm
    if (this.options.gfm) markTaskListItems(this.doc)

    return { doc: this.doc, refmap: this.refmap }
  }
}

// --- GFM task list items ----------------------------------------------------

/**
 * The task list item marker: optional spaces, `[`, either a whitespace
 * character or `x`/`X`, `]`, and then at least one whitespace character before
 * any other content.
 *
 * The trailing whitespace is required by the extension ("…and at least one
 * whitespace character before any other content"), so `- [x]` with nothing
 * after it is an ordinary item whose text is `[x]`. It is matched as a space
 * or a tab rather than as any whitespace: a line ending there would mean the
 * content starts on the *next* line, and consuming it would swallow the
 * paragraph's first soft break.
 */
const RE_TASK_LIST_MARKER = /^ *\[([ \tXx])\][ \t]+/

const TASK_LIST_CONTAINERS: Record<string, true> = {
  document: true,
  block_quote: true,
  list: true,
  item: true,
}

/**
 * GFM task list items: mark every item whose first block is a paragraph
 * beginning with a task list item marker, and consume the marker so it does
 * not survive into the text.
 *
 * Run at the end of the block phase, over `stringContent` — before the inline
 * phase has seen it. That is what makes the marker win over anything the
 * inline scanner would otherwise make of the brackets: with a `[x]: /url`
 * definition in the document, `- [x] foo` is still a task item rather than a
 * link. It also means the item's `checked` state is settled before `ast.ts` or
 * `html.ts` looks at the tree.
 *
 * Iterative rather than recursive: container nesting depth is chosen by the
 * input, and `'> '.repeat(20000)` is a document this parser otherwise handles.
 */
function markTaskListItems(doc: MdNode): void {
  const stack: MdNode[] = [doc]

  while (0 < stack.length) {
    const node = stack.pop() as MdNode

    if ('item' === node.type) {
      const first = node.firstChild
      if (null !== first && 'paragraph' === first.type) {
        const m = RE_TASK_LIST_MARKER.exec(first.stringContent)
        if (null !== m) {
          // Whitespace between the brackets is unchecked; `x`/`X` is checked.
          node.checked = ' ' !== m[1] && '\t' !== m[1]
          first.stringContent = first.stringContent.slice(m[0].length)
        }
      }
    }

    if (true === TASK_LIST_CONTAINERS[node.type]) {
      for (let c = node.lastChild; c; c = c.prev) stack.push(c)
    }
  }
}

/**
 * Phase 1: build the block tree and collect link reference definitions.
 * Paragraph and heading text is left raw in `stringContent` for phase 2.
 */
export function parseBlocks(
  input: string,
  options: ParserOptions,
): { doc: MdNode; refmap: RefMap } {
  return new BlockParser(options).parse(input)
}
