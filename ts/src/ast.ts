/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Projection from the native CommonMark tree (node.ts) to the mdast-adjacent
// JSON AST that `@tabnas/markdown` has always returned.
//
// The native tree is what the HTML renderer walks, and it carries things the
// public AST has no room for (source positions, the block/inline distinction
// for raw HTML, container open/closed state). This module is the one place
// that decides what survives the crossing. Two deliberate choices:
//
//   * Soft breaks collapse into a single space inside the surrounding text
//     run, which is the long-standing documented behaviour of this package
//     (`breaks:true` promotes them to `break` nodes instead). The native tree
//     keeps them as real `softbreak` nodes, so an HTML render is unaffected.
//   * `spread` now follows mdast semantics — a list is spread when it is
//     loose, an item when it holds blank-line-separated blocks. The previous
//     implementation latched this off the file's trailing newline, so the
//     field carried no information; nothing can have depended on it.

import { MdNode } from './node.ts'
import type { ParserOptions } from './options.ts'

export type DocumentNode = { type: 'document'; children: Block[] }

export type Block =
  | HeadingNode
  | ParagraphNode
  | BlockquoteNode
  | ListNode
  | ListItemNode
  | CodeNode
  | HtmlNode
  | ThematicBreakNode

export type HeadingNode = {
  type: 'heading'
  depth: 1 | 2 | 3 | 4 | 5 | 6
  children: Inline[]
}
export type ParagraphNode = { type: 'paragraph'; children: Inline[] }
export type BlockquoteNode = { type: 'blockquote'; children: Block[] }
export type ListNode = {
  type: 'list'
  ordered: boolean
  start: number | null
  spread: boolean
  children: ListItemNode[]
}
export type ListItemNode = { type: 'listItem'; spread: boolean; children: Block[] }
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
  | HtmlNode

export type TextNode = { type: 'text'; value: string }
export type EmphasisNode = { type: 'emphasis'; children: Inline[] }
export type StrongNode = { type: 'strong'; children: Inline[] }
export type InlineCodeNode = { type: 'inlineCode'; value: string }
export type LinkNode = {
  type: 'link'
  url: string
  title: string | null
  children: Inline[]
}
export type ImageNode = {
  type: 'image'
  url: string
  title: string | null
  alt: string
}
export type BreakNode = { type: 'break' }
export type DeleteNode = { type: 'delete'; children: Inline[] }

/** Plain-text flattening of an image's inline children, for `alt`. */
function collectAltText(node: MdNode): string {
  let out = ''
  let child = node.firstChild
  while (child) {
    if ('text' === child.type || 'code' === child.type || 'html_inline' === child.type) {
      out += child.literal ?? ''
    } else if ('softbreak' === child.type || 'linebreak' === child.type) {
      out += '\n'
    } else {
      out += collectAltText(child)
    }
    child = child.next
  }
  return out
}

/**
 * An item is spread when it holds more than one block-level child separated by
 * a blank line. The block parser records looseness on the parent list, so an
 * item inside a loose list that itself holds a single paragraph is not spread.
 */
function itemIsSpread(item: MdNode): boolean {
  let count = 0
  let child = item.firstChild
  while (child) {
    count++
    if (1 < count) return true
    child = child.next
  }
  return false
}

function inlineChildren(node: MdNode, opts: ParserOptions): Inline[] {
  const out: Inline[] = []

  const pushText = (value: string) => {
    if ('' === value) return
    const last = out[out.length - 1]
    if (last && 'text' === last.type) last.value += value
    else out.push({ type: 'text', value })
  }

  let child = node.firstChild
  while (child) {
    switch (child.type) {
      case 'text':
        pushText(child.literal ?? '')
        break

      case 'softbreak':
        // Documented behaviour: a soft break reads as a space in the AST.
        if (opts.breaks) out.push({ type: 'break' })
        else pushText(' ')
        break

      case 'linebreak':
        out.push({ type: 'break' })
        break

      case 'code':
        out.push({ type: 'inlineCode', value: child.literal ?? '' })
        break

      case 'html_inline':
        out.push({ type: 'html', value: child.literal ?? '' })
        break

      case 'emph':
        out.push({ type: 'emphasis', children: inlineChildren(child, opts) })
        break

      case 'strong':
        out.push({ type: 'strong', children: inlineChildren(child, opts) })
        break

      case 'del':
        out.push({ type: 'delete', children: inlineChildren(child, opts) })
        break

      case 'link':
        out.push({
          type: 'link',
          url: child.destination ?? '',
          title: child.title ?? null,
          children: inlineChildren(child, opts),
        })
        break

      case 'image':
        out.push({
          type: 'image',
          url: child.destination ?? '',
          title: child.title ?? null,
          alt: collectAltText(child),
        })
        break

      default:
        // A block node cannot appear here in a well-formed tree; skipping is
        // safer than emitting a node the published types do not allow.
        break
    }
    child = child.next
  }

  return out
}

function blockChildren(node: MdNode, opts: ParserOptions): Block[] {
  const out: Block[] = []
  let child = node.firstChild
  while (child) {
    const block = toBlock(child, opts)
    if (null !== block) out.push(block)
    child = child.next
  }
  return out
}

function toBlock(node: MdNode, opts: ParserOptions): Block | null {
  switch (node.type) {
    case 'paragraph':
      return { type: 'paragraph', children: inlineChildren(node, opts) }

    case 'heading':
      return {
        type: 'heading',
        depth: node.level as HeadingNode['depth'],
        children: inlineChildren(node, opts),
      }

    case 'thematic_break':
      return { type: 'thematicBreak' }

    case 'block_quote':
      return { type: 'blockquote', children: blockChildren(node, opts) }

    case 'code_block': {
      const info = (node.info ?? '').trim()
      let lang: string | null = null
      let meta: string | null = null
      if ('' !== info) {
        const sp = info.search(/\s/)
        if (-1 === sp) {
          lang = info
        } else {
          lang = info.slice(0, sp)
          meta = info.slice(sp + 1).trim() || null
        }
      }
      return { type: 'code', lang, meta, value: node.literal ?? '' }
    }

    case 'html_block':
      return { type: 'html', value: node.literal ?? '' }

    case 'list': {
      const data = node.listData
      const ordered = 'ordered' === data?.type
      const items: ListItemNode[] = []
      let child = node.firstChild
      while (child) {
        if ('item' === child.type) {
          items.push({
            type: 'listItem',
            spread: itemIsSpread(child),
            children: blockChildren(child, opts),
          })
        }
        child = child.next
      }
      return {
        type: 'list',
        ordered,
        start: ordered ? (data?.start ?? 1) : null,
        spread: !(data?.tight ?? true),
        children: items,
      }
    }

    case 'item':
      // Only reachable if an item is somehow detached from its list.
      return { type: 'listItem', spread: itemIsSpread(node), children: blockChildren(node, opts) }

    default:
      return null
  }
}

/** Project a parsed native tree into the public JSON AST. */
export function toAst(doc: MdNode, opts: ParserOptions): DocumentNode {
  return { type: 'document', children: blockChildren(doc, opts) }
}
