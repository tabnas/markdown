/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// @tabnas/markdown — CommonMark/GFM-subset prose parser, built as a
// tabnas plugin on the bare engine. See `dx-report.md` for the rescope
// rationale (was CSV-family, now prose).

import {
  Tabnas,
  Plugin,
  Rule,
  Context,
  Config,
  TabnasOptions,
} from '@tabnas/parser'

// ---------------------------------------------------------------------------
// Public options

export type MarkdownOptions = {
  /** Enable GFM extensions (strikethrough `~~`, task lists, tables — tables deferred, see dx-report). */
  gfm?: boolean
  /** When true, a single soft break becomes a hard break. Default false. */
  breaks?: boolean
}

// ---------------------------------------------------------------------------
// AST types — mdast-adjacent, JSON-serialisable

export type DocumentNode = { type: 'document'; children: Block[] }
export type Block =
  | HeadingNode
  | ParagraphNode
  | BlockquoteNode
  | ListNode
  | ListItemNode // also used as list child internally; top-level children are never bare items
  | CodeNode
  | HtmlNode
  | ThematicBreakNode

export type HeadingNode = { type: 'heading'; depth: 1 | 2 | 3 | 4 | 5 | 6; children: Inline[] }
export type ParagraphNode = { type: 'paragraph'; children: Inline[] }
export type BlockquoteNode = { type: 'blockquote'; children: Block[] }
export type ListNode = {
  type: 'list'
  ordered: boolean
  start: number | null
  spread: boolean
  children: ListItemNode[]
}
export type ListItemNode = {
  type: 'listItem'
  spread: boolean
  children: Block[]
}
export type CodeNode = {
  type: 'code'
  lang: string | null
  meta: string | null
  value: string
}
export type HtmlNode = { type: 'html'; value: string }
export type ThematicBreakNode = { type: 'thematicBreak' }

export type Inline =
  | TextNode
  | EmphasisNode
  | StrongNode
  | InlineCodeNode
  | LinkNode
  | ImageNode
  | BreakNode
  | DeleteNode

export type TextNode = { type: 'text'; value: string }
export type EmphasisNode = { type: 'emphasis'; children: Inline[] }
export type StrongNode = { type: 'strong'; children: Inline[] }
export type InlineCodeNode = { type: 'inlineCode'; value: string }
export type LinkNode = { type: 'link'; url: string; title: string | null; children: Inline[] }
export type ImageNode = { type: 'image'; url: string; title: string | null; alt: string }
export type BreakNode = { type: 'break' }
export type DeleteNode = { type: 'delete'; children: Inline[] }

// ---------------------------------------------------------------------------
// Grammar source — single-document entry rule `markdown`. The rule is tiny:
// blocks are parsed in JS inside the `bo` action via `parseDocument()`.
// The block structure therefore shows up as one rule in `debug.model()`
// and as one fanned `markdown → block` diagram from `@tabnas/railroad`.
// Keep the embedded grammar text trivial so `embed-grammar.js` stays happy
// and the source of truth is clearly JS (see dx-report §2.1).

// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
const grammarText = `
# Markdown prose grammar — CommonMark/GFM subset
# See dx-report.md section 2 and ts/src/markdown.ts header.
# This grammar is intentionally trivial: the prose parser is implemented
# in JS inside the markdown rule bo action (parseDocument). The
# block structure therefore shows as a single markdown -> block fan-out
# in the railroad diagram, which is honest about where the complexity
# lives. The file is kept so ts/embed-grammar.js continues to embed a
# grammar verbatim into both runtimes (see AGENTS.md).
{
  rule: markdown: open: [
    { s: '#ZZ' }
  ]
  rule: markdown: close: [
    { s: '#ZZ' }
  ]
}
`
// --- END EMBEDDED markdown-grammar.jsonic ---

// ---------------------------------------------------------------------------
// Inline helpers

function parseLinkDestination(inside: string): { url: string; title: string | null } {
  let s = inside.trim()
  if ('' === s) return { url: '', title: null }

  // Angle-bracketed URL: <http://example.com>
  if (s.startsWith('<')) {
    const close = s.indexOf('>', 1)
    if (-1 !== close) {
      const url = s.slice(1, close)
      const rest = s.slice(close + 1).trim()
      if ('' === rest) return { url, title: null }
      if (
        (rest.startsWith('"') && rest.endsWith('"') && 1 < rest.length) ||
        (rest.startsWith("'") && rest.endsWith("'") && 1 < rest.length) ||
        (rest.startsWith('(') && rest.endsWith(')') && 1 < rest.length)
      ) {
        return { url, title: rest.slice(1, -1) }
      }
      return { url: s, title: null } // malformed title → treat whole inside as url
    }
  }

  // Unbracketed URL: first token is url, remainder (if quoted) is title
  // Split on whitespace not inside quotes.
  let url = ''
  let title: string | null = null
  let i = 0
  while (i < s.length && !/\s/.test(s[i])) i++
  url = s.slice(0, i)
  const rest = s.slice(i).trim()
  if ('' !== rest) {
    if (
      (rest.startsWith('"') && rest.endsWith('"') && 1 < rest.length) ||
      (rest.startsWith("'") && rest.endsWith("'") && 1 < rest.length) ||
      (rest.startsWith('(') && rest.endsWith(')') && 1 < rest.length)
    ) {
      title = rest.slice(1, -1)
    } else {
      // Malformed — treat whole inside as url
      url = s
      title = null
    }
  }
  return { url, title }
}

function findClosingBracket(text: string, start: number): number {
  let depth = 0
  for (let i = start; i < text.length; i++) {
    const c = text[i]
    if ('\\' === c && i + 1 < text.length) { i++; continue }
    if ('[' === c) depth++
    else if (']' === c) {
      if (0 === depth) return i
      depth--
    }
  }
  return -1
}

function findClosingParen(text: string, start: number): number {
  let depth = 0
  for (let i = start; i < text.length; i++) {
    const c = text[i]
    if ('\\' === c && i + 1 < text.length) { i++; continue }
    if ('(' === c) depth++
    else if (')' === c) {
      if (0 === depth) return i
      depth--
    }
  }
  return -1
}

// ---------------------------------------------------------------------------
// Inline scanner — produces Inline[] from a text string.
// `text` may contain '\n' for soft breaks (paragraph lines joined with \n).

function parseInline(text: string, opts: Required<MarkdownOptions>): Inline[] {
  const nodes: Inline[] = []
  let i = 0
  let buf = ''

  const flush = () => {
    if ('' !== buf) { nodes.push({ type: 'text', value: buf }); buf = '' }
  }

  while (i < text.length) {
    const ch = text[i]

    // Escapes — \* \_ \~ \` etc.
    if ('\\' === ch && i + 1 < text.length && /[\\`*_\[\]{}()#+\-.!|>~]/.test(text[i + 1])) {
      buf += text[i + 1]
      i += 2
      continue
    }

    // Inline code — `code` or ``code with ` inside`` etc.
    if ('`' === ch) {
      let j = i
      while (j < text.length && '`' === text[j]) j++
      const ticks = j - i
      const fence = '`'.repeat(ticks)
      const close = text.indexOf(fence, j)
      if (-1 !== close) {
        const raw = text.slice(j, close)
        // Collapse newlines/spaces: trim at ends, internal newlines → space
        const code = raw.replace(/\r?\n/g, ' ').replace(/^\s+|\s+$/g, '')
        // Per CommonMark, `` `` → ``? Already handled by fence length.
        flush()
        nodes.push({ type: 'inlineCode', value: code })
        i = close + ticks
        continue
      }
    }

    // Image — ![alt](url "title")
    if (text.startsWith('![', i)) {
      const closeAlt = findClosingBracket(text, i + 2)
      if (-1 !== closeAlt && '(' === text[closeAlt + 1]) {
        const closeParen = findClosingParen(text, closeAlt + 2)
        if (-1 !== closeParen) {
          const alt = text.slice(i + 2, closeAlt)
          const inside = text.slice(closeAlt + 2, closeParen)
          const { url, title } = parseLinkDestination(inside)
          if ('' !== url) {
            flush()
            nodes.push({ type: 'image', url, title, alt })
            i = closeParen + 1
            continue
          }
        }
      }
    }

    // Autolink — <http://…> or <https://…>
    if ('<' === ch) {
      const close = text.indexOf('>', i)
      if (-1 !== close) {
        const url = text.slice(i + 1, close)
        if (/^https?:\/\/\S+$/.test(url) || /^mailto:[^\s@]+@[^\s@]+\.[^\s@]+$/.test(url)) {
          flush()
          const href = url.startsWith('mailto:') ? url : url
          nodes.push({ type: 'link', url: href, title: null, children: [{ type: 'text', value: url }] })
          i = close + 1
          continue
        }
        // Email autolink <a@b.com> etc. — treat same as http for now (already covered)
        if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(url)) {
          flush()
          nodes.push({ type: 'link', url: 'mailto:' + url, title: null, children: [{ type: 'text', value: url }] })
          i = close + 1
          continue
        }
      }
    }

    // Link — [label](url "title")
    if ('[' === ch) {
      const closeBracket = findClosingBracket(text, i + 1)
      if (-1 !== closeBracket) {
        const label = text.slice(i + 1, closeBracket)
        if ('(' === text[closeBracket + 1]) {
          const closeParen = findClosingParen(text, closeBracket + 2)
          if (-1 !== closeParen) {
            const inside = text.slice(closeBracket + 2, closeParen)
            const { url, title } = parseLinkDestination(inside)
            if ('' !== url) {
              flush()
              nodes.push({ type: 'link', url, title, children: parseInline(label, opts) })
              i = closeParen + 1
              continue
            }
          }
        }
      }
    }

    // Strong — **text** or __text__ (must be non-empty)
    if (text.startsWith('**', i) || text.startsWith('__', i)) {
      const delim = text.slice(i, i + 2)
      const close = text.indexOf(delim, i + 2)
      if (-1 !== close && close > i + 2) {
        const inner = text.slice(i + 2, close)
        if ('' !== inner.trim()) {
          flush()
          nodes.push({ type: 'strong', children: parseInline(inner, opts) })
          i = close + 2
          continue
        }
      }
    }

    // Strikethrough GFM — ~~text~~
    if (opts.gfm && text.startsWith('~~', i)) {
      const close = text.indexOf('~~', i + 2)
      if (-1 !== close && close > i + 2) {
        const inner = text.slice(i + 2, close)
        if ('' !== inner.trim()) {
          flush()
          nodes.push({ type: 'delete', children: parseInline(inner, opts) })
          i = close + 2
          continue
        }
      }
    }

    // Emphasis — *text* or _text_ (single char)
    if ('*' === ch || '_' === ch) {
      // Avoid handling ** as *: already handled above, but single * inside ** shouldn't reach here alone
      // If next char is same delim, we would have taken strong branch; so this is single.
      // For '_' inside a word like foo_bar_baz, CommonMark says not emphasis — we simplify to not split inside alphanum_ sequence.
      if ('_' === ch) {
        const prevIsAlnum = i > 0 && /[\w]/.test(text[i - 1])
        const nextIsAlnum = i + 1 < text.length && /[\w]/.test(text[i + 1])
        if (prevIsAlnum && nextIsAlnum) { buf += ch; i++; continue }
        // also check close side will have same word-boundary issue, but we check at open and close
      }
      const delim = ch
      let close = -1
      // Find closing delim not escaped and with word-boundary handling for _
      for (let k = i + 1; k < text.length; k++) {
        if ('\\' === text[k] && k + 1 < text.length) { k++; continue }
        if (delim === text[k]) {
          if ('_' === delim) {
            const beforeIsAlnum = k > 0 && /[\w]/.test(text[k - 1])
            const afterIsAlnum = k + 1 < text.length && /[\w]/.test(text[k + 1])
            if (beforeIsAlnum && afterIsAlnum) continue
          }
          // Ensure not part of strong's double
          if (k + 1 < text.length && delim === text[k + 1]) continue
          if (k > 0 && delim === text[k - 1] && text.startsWith(delim + delim, k - 1)) continue
          close = k
          break
        }
      }
      if (-1 !== close && close > i + 1) {
        const inner = text.slice(i + 1, close)
        if ('' !== inner.trim()) {
          flush()
          nodes.push({ type: 'emphasis', children: parseInline(inner, opts) })
          i = close + 1
          continue
        }
      }
    }

    // Hard break — two spaces + \n or backslash + \n, or plain \n when breaks:true
    if ('\n' === ch) {
      const prevTwoSpaces = i >= 2 && ' ' === text[i - 1] && ' ' === text[i - 2]
      if (prevTwoSpaces) {
        // Hard break via trailing spaces: remove the two spaces from buffered text
        // They are still in buf (not yet flushed), so trim them there.
        if (buf.endsWith('  ')) buf = buf.slice(0, -2)
        else if (buf.endsWith(' ')) buf = buf.slice(0, -1)
        flush()
        nodes.push({ type: 'break' })
      } else if (opts.breaks) {
        flush()
        nodes.push({ type: 'break' })
      } else {
        // Soft break → single space inside the current text run
        buf += ' '
      }
      i++
      continue
    }

    // Fallthrough — plain char
    buf += ch
    i++
  }

  flush()
  // If nothing was produced but non-empty text, return single text node (preserve empty paragraph case ? — caller handles)
  return nodes
}

// ---------------------------------------------------------------------------
// Block helpers

function isBlank(line: string): boolean {
  return /^[\t ]*$/.test(line)
}

function isATXHeading(line: string): RegExpMatchArray | null {
  // Up to 3 leading spaces, 1-6 #'s, then space, then content, then optional trailing #'s
  return line.match(/^ {0,3}(#{1,6})[ \t]+(.+?)(?:[ \t]+#+)?[ \t]*$/)
}

function isSetextUnderline(line: string): RegExpMatchArray | null {
  return line.match(/^ {0,3}(=+|-+)[ \t]*$/)
}

function isThematicBreak(line: string): boolean {
  if (!/^ {0,3}((\*([ \t]*\*)*)|(-([ \t]*-)*)|(_([ \t]*_)*))[ \t]*$/.test(line)) return false
  const stripped = line.replace(/[ \t]/g, '')
  if (3 > stripped.length) return false
  const c = stripped[0]
  return [...stripped].every(ch => c === ch)
}

function isFencedCodeStart(line: string): RegExpMatchArray | null {
  return line.match(/^ {0,3}(`{3,}|~{3,})[ \t]*.*$/)
}

function isIndentedCodeLine(line: string): boolean {
  return /^ {4}|\t/.test(line)
}

function isBlockquoteLine(line: string): boolean {
  return /^ {0,3}>\s?/.test(line)
}

function isUnorderedListItem(line: string): RegExpMatchArray | null {
  return line.match(/^ {0,3}([*+-])[ \t]+(.*)$/)
}

function isOrderedListItem(line: string): RegExpMatchArray | null {
  return line.match(/^ {0,3}(\d+)([.)])[ \t]+(.*)$/)
}

function isHtmlBlockLine(line: string): boolean {
  const t = line.trimStart()
  return t.startsWith('<') && (
    /^<(?:!--|!\[CDATA\[|!DOCTYPE|\?[a-z])/i.test(t) ||
    /^<\/?[a-zA-Z][\w-]*(?:\s[^>]*)?>/.test(t)
  )
}

// ---------------------------------------------------------------------------
// Block parsing

function parseDocument(src: string, opts: Required<MarkdownOptions>): DocumentNode {
  // Normalize newlines, keep trailing newline behaviour: split on \r?\n
  const lines = src.split(/\r?\n/)
  // CommonMark: final newline not significant for block parsing — but an
  // empty trailing line from split should not produce an extra block.
  const { nodes } = parseBlocks(lines, 0, opts, 0)
  return { type: 'document', children: nodes }
}

function parseBlocks(
  lines: string[],
  start: number,
  opts: Required<MarkdownOptions>,
  baseIndent: number,
): { nodes: Block[]; next: number } {
  const nodes: Block[] = []
  let i = start

  while (i < lines.length) {
    let line = lines[i]

    // End if dedented below baseIndent for list handling — caller manages.
    // Here we just skip blanks that are not meaningful inside prose.
    if (isBlank(line)) {
      i++
      continue
    }

    // Fenced code — takes precedence over other block types
    const fenced = isFencedCodeStart(line)
    if (fenced) {
      const fence = fenced[1]
      const fenceChar = fence[0]
      const langAndMeta = line.slice(line.indexOf(fence) + fence.length).trim()
      let lang: string | null = null
      let meta: string | null = null
      if ('' !== langAndMeta) {
        const spaceIdx = langAndMeta.search(/\s/)
        if (-1 === spaceIdx) lang = langAndMeta
        else { lang = langAndMeta.slice(0, spaceIdx); meta = langAndMeta.slice(spaceIdx + 1).trim() || null }
      }
      const closingRe = new RegExp('^ {0,3}' + fenceChar + '{' + fence.length + ',}[ \\t]*$')
      const content: string[] = []
      i++
      while (i < lines.length && !closingRe.test(lines[i])) {
        content.push(lines[i])
        i++
      }
      if (i < lines.length) i++ // consume closing fence
      nodes.push({ type: 'code', lang: lang || null, meta: meta || null, value: content.join('\n') })
      continue
    }

    // Indented code — line indented by ≥4 spaces and not already handled as list
    if (isIndentedCodeLine(line) && !isUnorderedListItem(line) && !isOrderedListItem(line)) {
      const content: string[] = []
      while (i < lines.length && (isIndentedCodeLine(lines[i]) || isBlank(lines[i]))) {
        if (isBlank(lines[i])) {
          // Blank line inside indented code is kept as empty line
          // but we must peek ahead for next indented line to decide if blank is part of code
          // Look ahead to next non-blank
          let j = i + 1
          while (j < lines.length && isBlank(lines[j])) j++
          if (j < lines.length && isIndentedCodeLine(lines[j])) {
            content.push('')
            i++
          } else break
        } else {
          const raw = lines[i]
          content.push(raw.startsWith('\t') ? raw.slice(1) : raw.slice(4))
          i++
        }
      }
      nodes.push({ type: 'code', lang: null, meta: null, value: content.join('\n') })
      continue
    }

    // ATX heading
    const atx = isATXHeading(line)
    if (atx) {
      const depth = atx[1].length as HeadingNode['depth']
      const content = atx[2].trim()
      nodes.push({ type: 'heading', depth, children: parseInline(content, opts) })
      i++
      continue
    }

    // Thematic break — before paragraph/setext because --- can be both
    if (isThematicBreak(line)) {
      // But `---` after a paragraph line could be a setext underline for heading level 2.
      // Handle setext case one line lookahead before emitting thematic break.
      // Setext check is done inside paragraph path below via peek.
      // Here we only emit thematic break if not a setext context.
      // For simplicity, if previous block was a paragraph's single-line source that could be setext,
      // paragraph collector would have already consumed it.
      // So safe to emit.
      nodes.push({ type: 'thematicBreak' })
      i++
      continue
    }

    // Blockquote — collect consecutive >-prefixed lines
    if (isBlockquoteLine(line)) {
      const bqLines: string[] = []
      while (i < lines.length && (isBlockquoteLine(lines[i]) || isBlank(lines[i]))) {
        // Blank line inside blockquote is kept, but only if next line is also blockquoted or we are continuing quote
        if (isBlank(lines[i])) {
          // Peek next: if next is blockquote line, this blank is inside quote
          const nxt = i + 1 < lines.length ? lines[i + 1] : ''
          if (isBlockquoteLine(nxt)) { bqLines.push(''); i++; continue }
          // else blank ends blockquote when followed by non-quote content
          // but per CommonMark blank inside quote is allowed even without > marker if we are inside
          // we treat blank as part of quote for recursion
          bqLines.push('')
          i++
          // If next line is blank and then non-quote, break to let outer parse handle
          let j = i
          while (j < lines.length && isBlank(lines[j])) j++
          if (j < lines.length && !isBlockquoteLine(lines[j]) && !isBlank(lines[j])) break
          continue
        }
        const m = lines[i].match(/^ {0,3}>[ \t]?(.*)$/)
        bqLines.push(m ? m[1] : '')
        i++
      }
      // Trim trailing blank
      while (bqLines.length && '' === bqLines[bqLines.length - 1]) bqLines.pop()
      const inner = parseBlocks(bqLines, 0, opts, 0).nodes
      nodes.push({ type: 'blockquote', children: inner })
      continue
    }

    // HTML block — raw line starting with < until blank line
    if (isHtmlBlockLine(line)) {
      const htmlLines: string[] = []
      while (i < lines.length && !isBlank(lines[i])) {
        htmlLines.push(lines[i])
        i++
        // HTML blocks typically end at blank line; we let blank consume break outer
        if (i < lines.length && isBlank(lines[i])) break
        // Also break if next line is a clear block start (heading, fence, thematic)
        const nxt = i < lines.length ? lines[i] : ''
        if (isATXHeading(nxt) || isFencedCodeStart(nxt) || isThematicBreak(nxt) || isBlockquoteLine(nxt) || isUnorderedListItem(nxt) || isOrderedListItem(nxt)) break
      }
      nodes.push({ type: 'html', value: htmlLines.join('\n') })
      continue
    }

    // Lists — must be handled together to group consecutive items
    const isUL = isUnorderedListItem(line)
    const isOL = isOrderedListItem(line)
    if (isUL || isOL) {
      const ordered = !!isOL
      const startNum = isOL ? parseInt(isOL[1], 10) : null
      const items: ListItemNode[] = []
      let spread = false
      // Collect items while next line is list marker of similar type or indented continuation
      while (i < lines.length) {
        const cur = lines[i]
        if (isBlank(cur)) { i++; continue }
        const ulM = isUnorderedListItem(cur)
        const olM = isOrderedListItem(cur)
        const markerMatch = ordered ? olM : ulM
        // If next line is not the right list kind, see if it's continuation of previous item
        if (!markerMatch) {
          // Continuation: indented by ≥2 spaces or belongs to last item's blocks?
          // Check if last item exists and current line is indented (2+ spaces) and not another block start
          if (items.length && /^ {2,}/.test(cur) && !isATXHeading(cur) && !isFencedCodeStart(cur) && !isThematicBreak(cur) && !isBlockquoteLine(cur) && !isHtmlBlockLine(cur)) {
            // Continuation belongs to last item — append to its raw lines before re-parsing
            // We will instead handle continuations inside item parsing loop below.
          }
          break
        }
        // Parse one item
        const markerLen = cur.match(/^ {0,3}(?:[*+-]|\d+[.)])[ \t]+/)![0].length
        const firstContent = ordered ? olM![3] : ulM![2]
        const itemLines: string[] = []
        itemLines.push(firstContent)
        i++

        // Collect following lines belonging to this item: blank lines, indented content, paragraph continuation
        let blankRun = 0
        while (i < lines.length) {
          const nxt = lines[i]
          if (isBlank(nxt)) {
            // Blank inside list item is kept — but peek if next line starts a new item
            const peek = i + 1 < lines.length ? lines[i + 1] : null
            const peekIsList = peek && (isUnorderedListItem(peek) || isOrderedListItem(peek))
            if (peekIsList) break // blank before next item → end of this item
            // If loose, keep blank
            itemLines.push('')
            blankRun++
            i++
            continue
          }

          // If next line is a list marker at same base indent, it is the next item
          if ((isUnorderedListItem(nxt) || isOrderedListItem(nxt)) && !/^ {4,}/.test(nxt)) break

          // If line is indented ≥2 spaces beyond base? Actually CommonMark list content indent = markerLen or 4?
          // Simplify: if line starts with spaces and not a blockquote/thematic/heading/fenced, treat as item continuation.
          if (/^ {2,}/.test(nxt) || /^\t/.test(nxt)) {
            // Strip one level of indentation (up to 4 spaces or 1 tab) but keep as content
            let stripped = nxt
            if (stripped.startsWith('\t')) stripped = stripped.slice(1)
            else {
              // Remove up to markerLen or 4 spaces? Use 2 as common
              const lead = stripped.match(/^ +/)![0].length
              stripped = stripped.slice(Math.min(4, lead))
            }
            // Check if stripped line looks like an inner blockquote/list etc. — keep as is for inner parse
            itemLines.push(stripped)
            i++
            blankRun = 0
            continue
          }

          // If line looks like another block start (heading, code, thematic, blockquote, html) at base, break?
          if (isATXHeading(nxt) || isFencedCodeStart(nxt) || isThematicBreak(nxt) || isBlockquoteLine(nxt) || isHtmlBlockLine(nxt)) {
            // Inside list items, these are content, not outer blocks, if indented earlier they'd have been taken.
            // Unindented block inside list: check if we already have content — if so it may terminate list.
            // For tight lists, indented blocks are needed. Simplify: break to let items close, next iteration will handle as outer block.
            break
          }

          // Otherwise treat as loose paragraph continuation? Only if previous line wasn't blank?
          if (blankRun) break
          // Unindented paragraph-like line inside item (tight list paragraph extension)
          // Only if item already has one line and next line is not a list marker: join as continuation
          itemLines.push(nxt)
          i++
        }

        // Determine if item had blank line inside → affects list spread (loose)
        const itemHadBlank = itemLines.includes('')
        if (itemHadBlank) spread = true

        // Trim trailing blanks
        while (itemLines.length && '' === itemLines[itemLines.length - 1]) itemLines.pop()

        // Parse item content as blocks (recursive)
        // If itemLines is single line and contains only inline, it will become a paragraph. CommonMark wraps tight items' paragraph directly.
        const strippedItemSrc = itemLines.join('\n')
        // Avoid empty item
        let itemBlocks: Block[] = []
        if (strippedItemSrc.trim()) {
          itemBlocks = parseBlocks(strippedItemSrc.split(/\r?\n/), 0, opts, 0).nodes
        }

        // Tight lists: single paragraph items are collapsed? Our AST keeps paragraph wrapper regardless — callers handle spread.
        // Record spread on item if its trailing had blank
        items.push({ type: 'listItem', spread: itemHadBlank, children: itemBlocks })
      }

      nodes.push({ type: 'list', ordered, start: startNum, spread, children: items })
      continue
    }

    // Paragraph — collect until blank or block start, handling setext underline
    {
      const paraLines: string[] = []
      while (i < lines.length && !isBlank(lines[i])) {
        const nxt = lines[i]
        // Break if next line is a block that cannot be inside a paragraph without blank line.
        // Setext underline (=== or ---) looks like a thematic break but must be kept
        // as the last para line so the post-loop Setext check can turn the paragraph
        // into a heading. Only break on thematic breaks that are NOT Setext underlines.
        const isSetext = null !== isSetextUnderline(nxt)
        const isThematicButNotSetext = isThematicBreak(nxt) && !isSetext
        if (paraLines.length && (
          isATXHeading(nxt) ||
          isFencedCodeStart(nxt) ||
          isThematicButNotSetext ||
          isBlockquoteLine(nxt) ||
          isUnorderedListItem(nxt) ||
          isOrderedListItem(nxt) ||
          isHtmlBlockLine(nxt)
        )) break
        // Also check for indented code (not part of para)
        if (paraLines.length && isIndentedCodeLine(nxt)) break
        paraLines.push(nxt)
        i++
      }

      // Check for Setext heading: last line could be underline if para is 1-2 lines of text
      // A setext underline must not be thematic break-duplicate. Pattern: === or ---
      if (paraLines.length) {
        const last = paraLines[paraLines.length - 1]
        const underline = isSetextUnderline(last)
        if (underline && paraLines.length >= 2) {
          const depth: 1 | 2 = '=' === underline[1][0] ? 1 : 2
          const hdText = paraLines.slice(0, -1).join('\n')
          nodes.push({ type: 'heading', depth, children: parseInline(hdText.trim(), opts) })
          continue
        }
        // Single-line setext: need preceding para line + underline on next line? Our peek included underline line already as part of paraLines.
        // If paraLines is 2 lines where second is underline, the above would have depth. If paraLines is 1 line underline alone, it was already handled as thematic break earlier? But `---` alone is thematic break, not setext with no para above.
        // So ok.
      }

      const paraText = paraLines.join('\n').trimEnd()
      if ('' !== paraText) {
        nodes.push({ type: 'paragraph', children: parseInline(paraText, opts) })
      }
      continue
    }
  }

  return { nodes, next: i }
}

// ---------------------------------------------------------------------------
// Plugin wiring — bare engine (no jsonic). The rule is trivial; all
// work happens in the `bo` that captures the source via `ctx.src()`.
// This is the same pattern yaml's 2559-line plugin ultimately uses:
// custom lex matchers + state actions, not declarative alts, for prose.

const Markdown: Plugin = (tn: Tabnas, options?: MarkdownOptions) => {
  const opts: Required<MarkdownOptions> = {
    gfm: options?.gfm ?? true,
    breaks: options?.breaks ?? false,
  }

  // No custom token descriptions needed for prose — the block scanner
  // reads ctx.src() directly; railroad will show the single `markdown`
  // rule as the entry point.

  // Start rule
  tn.options({ rule: { start: 'markdown' } })

  // Prose parser reads `ctx.src()` directly and does not rely on the
  // jsonic lexer. Disable lexers that would turn Markdown syntax into
  // bad tokens before the block scanner runs:
  // - string: backtick code spans would otherwise be parsed as unterminated strings
  // - comment: `# heading` must not be swallowed as a comment line
  // - number/value: `1. list` must stay as text, not a number/keyword
  tn.options({
    string: { lex: false },
    comment: { lex: false },
    number: { lex: false },
    value: { lex: false },
    lex: { emptyResult: { type: 'document', children: [] } as DocumentNode },
  })

  // Grammar is intentionally tiny — see `grammarText` above for the
  // railroad-visible spec. Install it first so `rule('markdown')` can
  // clear/override it cleanly.
  // We parse the embedded JSON via a throwaway Tabnas to get a GrammarSpec,
  // then install it with the ref map.
  // To avoid a jsonic dependency, install inline via tn.grammar instead of
  // parsing jsonic text — simpler: just define the rule directly.

  const refs: Record<string, any> = {
    '@markdown-bo': (r: Rule, ctx: Context) => {
      const src: string = ctx.src()
      // Preserve empty handling: "" → empty document
      if ('' === src || /^[\r\n\t ]*$/.test(src)) {
        r.node = { type: 'document', children: [] } as DocumentNode
        return
      }
      r.node = parseDocument(src, opts)
    },
  }

  // Define the `markdown` rule: `bo` builds the document from
  // `ctx.src()`; the alts then consume the underlying token stream so
  // the engine's trailing-content check (lex must end at #ZZ) passes.
  // `#AA` is the ANY-token wildcard and `r:'markdown'` loops at the
  // same depth, consuming one token per iteration until only #ZZ
  // remains. The empty alt `{}`
  // then falls through to the close phase, which matches #ZZ.
  tn.rule('markdown', (rs) => {
    return rs
      .clear()
      .bo(refs['@markdown-bo'] as any)
      .open([{ s: '#AA', r: 'markdown' }, {}])
      .close([{ s: '#ZZ' }, {}])
  })

  // Expose helper for external HTML rendering or inline reuse (not part of
  // tabnas contract, but useful for tests/docs).
  ;(tn as any).markdown = { parseDocument, parseInline: (t: string) => parseInline(t, opts) }
}

// Defaults for `new Tabnas().use(Markdown, opts?)` merging
Markdown.defaults = { gfm: true, breaks: false } as MarkdownOptions

export {
  Markdown,
  parseDocument,
  parseInline,
}
