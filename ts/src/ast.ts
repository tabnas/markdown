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
//
// The tree keeps references unresolved, as mdast does: a bracketed label is a
// `linkReference` or an `imageReference` naming it, whether or not the
// document defines it, and each link reference definition is a `definition`
// node where it stands. A block's tree then depends on nothing outside the
// block, so it is complete as the block closes (the engine path writes it
// then; see engine-block.ts). `resolveReferences` gives back the tree with the
// references resolved, for a caller that wants links. A list's and a list
// item's `spread`, and an item's `checked`, follow its `children`, since they
// are known only once the children are.

import { MdNode } from './node.ts'
import type { Definition, ReferenceType, TableAlign } from './node.ts'
import type { ParserOptions } from './options.ts'
import { normalizeReference, unescapeString } from './common.ts'

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
  | TableNode
  | DefinitionNode

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
  children: ListItemNode[]
  spread: boolean
}
export type ListItemNode = {
  type: 'listItem'
  children: Block[]
  spread: boolean
  /**
   * mdast's task list field: `true`/`false` for a GFM task list item
   * (`- [x] foo`), `null` for an ordinary item — and therefore `null`
   * everywhere when `gfm` is off.
   */
  checked: boolean | null
}
export type CodeNode = {
  type: 'code'
  lang: string | null
  meta: string | null
  value: string
}
export type HtmlNode = { type: 'html'; value: string }
export type ThematicBreakNode = { type: 'thematicBreak' }

/**
 * A GFM table, in mdast's shape.
 *
 * `align` has one entry per column — `null` where the delimiter row gave no
 * colon — and every row has exactly that many cells, since the block phase
 * pads short rows and truncates long ones.
 *
 * mdast has no header flag: the **first** row is the header row, by
 * convention, and this projection preserves that ordering from the tree.
 */
export type TableNode = {
  type: 'table'
  align: TableAlign[]
  children: TableRowNode[]
}
export type TableRowNode = { type: 'tableRow'; children: TableCellNode[] }
export type TableCellNode = { type: 'tableCell'; children: Inline[] }

/**
 * A link reference definition (§4.7), where it stands among the blocks.
 * `identifier` is the label normalized as labels are matched (white space
 * collapsed and trimmed, case folded), in lower case, as mdast's; `label` is
 * its text with escapes and character references read. Every definition is
 * a node, a label's second one too; the first of a label is the one a
 * reference resolves to.
 */
export type DefinitionNode = {
  type: 'definition'
  identifier: string
  label: string
  url: string
  title: string | null
}

export type Inline =
  | TextNode
  | EmphasisNode
  | StrongNode
  | InlineCodeNode
  | LinkNode
  | ImageNode
  | LinkReferenceNode
  | ImageReferenceNode
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
/**
 * A link whose destination a definition gives, by its label: `[text][label]`
 * (`full`), `[label][]` (`collapsed`) or `[label]` (`shortcut`). Made wherever
 * the label could name a definition, whether or not the document has one, so
 * a bracketed label with no definition is a reference here, not the literal
 * text CommonMark reads it as; `resolveReferences` turns it back into that
 * text.
 */
export type LinkReferenceNode = {
  type: 'linkReference'
  identifier: string
  label: string
  referenceType: ReferenceType
  children: Inline[]
}
/** An image whose source a definition gives, by its label, as a link's. */
export type ImageReferenceNode = {
  type: 'imageReference'
  identifier: string
  label: string
  referenceType: ReferenceType
  alt: string
}
export type BreakNode = { type: 'break' }
export type DeleteNode = { type: 'delete'; children: Inline[] }

/**
 * A label's identifier, as mdast's: normalized as CommonMark matches labels
 * (`normalizeReference`), in lower case. Two labels a definition and a
 * reference would match on have the same identifier.
 */
export function labelIdentifier(rawLabel: string): string {
  return normalizeReference(rawLabel).toLowerCase()
}

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
 * mdast semantics: an item is spread when two of its children are separated by
 * a *blank line* — not merely when it has more than one child. `- a\n  - b`
 * holds a paragraph and a nested list with no blank line between them, and is
 * not spread; `- a\n\n  - b` is.
 *
 * Derived from the block parser's `sourcepos` rather than tracked separately:
 * consecutive children whose line ranges are not adjacent must have had a
 * blank line between them.
 */
export function itemIsSpread(item: MdNode): boolean {
  // The children in the AST's order, definitions among them, each by the
  // lines it spans.
  let spread = false
  let prevEnd = -1
  const step = (start: number, end: number) => {
    if (0 <= prevEnd && start > prevEnd + 1) spread = true
    prevEnd = end
  }
  forEachBlockChild(
    item,
    (def) => step(def.start, def.end),
    (child) => step(child.sourcepos[0][0], child.sourcepos[1][0]),
  )
  return spread
}

/**
 * A container's children in the AST's order: each child, and each
 * definition where it stands among them (see `DefinitionSite`).
 */
export function forEachBlockChild(
  container: MdNode,
  onDefinition: (def: Definition) => void,
  onChild: (child: MdNode) => void,
): void {
  const defs = container.definitions
  let d = 0
  if (null !== defs) {
    while (d < defs.length && null === defs[d].after) onDefinition(defs[d++].def)
  }
  for (let c = container.firstChild; c; c = c.next) {
    onChild(c)
    if (null !== defs) {
      while (d < defs.length && c === defs[d].after) onDefinition(defs[d++].def)
    }
  }
}

/** A definition's node. */
export function definitionNode(def: Definition): DefinitionNode {
  return {
    type: 'definition',
    identifier: labelIdentifier(def.label),
    label: unescapeString(def.label),
    url: def.destination,
    title: null === def.title || '' === def.title ? null : def.title,
  }
}

export function inlineChildren(node: MdNode, opts: ParserOptions): Inline[] {
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

      case 'link_reference':
        out.push({
          type: 'linkReference',
          identifier: labelIdentifier(child.label ?? ''),
          label: unescapeString(child.label ?? ''),
          referenceType: child.referenceType ?? 'shortcut',
          children: inlineChildren(child, opts),
        })
        break

      case 'image_reference':
        out.push({
          type: 'imageReference',
          identifier: labelIdentifier(child.label ?? ''),
          label: unescapeString(child.label ?? ''),
          referenceType: child.referenceType ?? 'shortcut',
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

/** Containers whose children are blocks rather than inlines. */
const BLOCK_CONTAINERS: Record<string, true> = {
  document: true,
  block_quote: true,
  list: true,
  item: true,
}

/**
 * Project a container's block children.
 *
 * Iterative rather than recursive, because the recursion depth here is the
 * document's container nesting depth, which untrusted input controls directly:
 * `'> '.repeat(4000)` is 8 KB of Markdown that the block parser and the HTML
 * renderer both handle, but that overflowed the stack in this projection —
 * crashing the package's primary API on input the rest of the pipeline
 * survives.
 *
 * Two passes. The first walks the block tree with an explicit stack, recording
 * nodes in an order where every parent precedes its descendants. Building in
 * reverse then guarantees a node's children are already built when it is
 * reached, so `buildBlock` can look them up instead of recursing.
 *
 * The Go port keeps the recursive form: goroutine stacks grow on demand and it
 * projects 100k-deep nesting without trouble, so the same defect does not
 * arise there.
 */
function blockChildren(node: MdNode, opts: ParserOptions): Block[] {
  const order: MdNode[] = []
  const stack: MdNode[] = []

  for (let c = node.lastChild; c; c = c.prev) stack.push(c)
  while (0 < stack.length) {
    const n = stack.pop() as MdNode
    order.push(n)
    if (true === BLOCK_CONTAINERS[n.type]) {
      for (let c = n.lastChild; c; c = c.prev) stack.push(c)
    }
  }

  const built = new Map<MdNode, Block>()
  for (let i = order.length - 1; 0 <= i; i--) {
    const n = order[i]
    const block = buildBlock(n, opts, built)
    if (null !== block) built.set(n, block)
  }

  return builtChildren(node, built)
}

/**
 * Children of `n` that have already been built, in source order, with the
 * definitions among them.
 */
function builtChildren(n: MdNode, built: Map<MdNode, Block>): Block[] {
  const out: Block[] = []
  forEachBlockChild(
    n,
    (def) => out.push(definitionNode(def)),
    (c) => {
      const b = built.get(c)
      if (undefined !== b) out.push(b)
    },
  )
  return out
}

/**
 * The fields of a container block's node known when it opens, `type`
 * first, in the order the node has them: what precedes its `children`.
 * Its fields known only when it closes are `blockTail`'s.
 */
export function blockHead(node: MdNode): Record<string, unknown> {
  switch (node.type) {
    case 'block_quote':
      return { type: 'blockquote' }
    case 'list': {
      const data = node.listData
      const ordered = 'ordered' === data?.type
      return { type: 'list', ordered, start: ordered ? (data?.start ?? 1) : null }
    }
    case 'item':
      return { type: 'listItem' }
    case 'paragraph':
      return { type: 'paragraph' }
    case 'heading':
      return { type: 'heading', depth: node.level }
    case 'table':
      return { type: 'table', align: node.tableAlign ?? [] }
    case 'table_row':
      return { type: 'tableRow' }
    case 'table_cell':
      return { type: 'tableCell' }
    default:
      throw new Error('markdown: no container node for ' + node.type)
  }
}

/** The fields of a container block's node that follow its `children`. */
export function blockTail(node: MdNode): Record<string, unknown> {
  switch (node.type) {
    case 'list':
      return { spread: !(node.listData?.tight ?? true) }
    case 'item':
      return { spread: itemIsSpread(node), checked: node.checked }
    default:
      return {}
  }
}

/** A leaf block's node: a code block, an HTML block or a thematic break. */
export function leafBlock(node: MdNode): Block {
  switch (node.type) {
    case 'thematic_break':
      return { type: 'thematicBreak' }
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
      // The native tree keeps the trailing newline because the spec's HTML
      // for `<pre><code>` includes it; mdast's `code.value` does not. Strip
      // exactly one, not all, so a block that genuinely ends in a blank line
      // keeps it.
      const raw = node.literal ?? ''
      const value = raw.endsWith('\n') ? raw.slice(0, -1) : raw
      return { type: 'code', lang, meta, value }
    }
    case 'html_block':
      return { type: 'html', value: node.literal ?? '' }
    default:
      throw new Error('markdown: no leaf node for ' + node.type)
  }
}

/** A container node of the AST: its head, its children, then its tail. */
function containerNode(node: MdNode, children: unknown[]): any {
  return Object.assign(blockHead(node), { children }, blockTail(node))
}

function buildBlock(
  node: MdNode,
  opts: ParserOptions,
  built: Map<MdNode, Block>,
): Block | null {
  switch (node.type) {
    case 'paragraph':
    case 'heading':
      return containerNode(node, inlineChildren(node, opts))

    case 'thematic_break':
    case 'code_block':
    case 'html_block':
      return leafBlock(node)

    case 'block_quote':
      return containerNode(node, builtChildren(node, built))

    case 'table': {
      // Built here rather than through `built`, because a table's children are
      // rows and cells rather than blocks — `blockChildren` never descends
      // into one. The nesting is a fixed three levels deep, so unlike the
      // block containers this cannot be driven past the stack by input.
      const rows: TableRowNode[] = []
      for (let row = node.firstChild; row; row = row.next) {
        if ('table_row' !== row.type) continue
        const cells: TableCellNode[] = []
        for (let cell = row.firstChild; cell; cell = cell.next) {
          if ('table_cell' !== cell.type) continue
          cells.push(containerNode(cell, inlineChildren(cell, opts)))
        }
        rows.push(containerNode(row, cells))
      }
      return containerNode(node, rows)
    }

    case 'list': {
      const items: ListItemNode[] = []
      for (let child = node.firstChild; child; child = child.next) {
        if ('item' === child.type) items.push(containerNode(child, builtChildren(child, built)))
      }
      return containerNode(node, items)
    }

    case 'item':
      // Only reachable if an item is somehow detached from its list; a list
      // builds its own items above.
      return containerNode(node, builtChildren(node, built))

    default:
      return null
  }
}

/** Project a parsed native tree into the public JSON AST. */
export function toAst(doc: MdNode, opts: ParserOptions): DocumentNode {
  return { type: 'document', children: blockChildren(doc, opts) }
}

// --- resolving references -----------------------------------------------------

/** What `resolveReferences` reads a reference's destination from. */
type Resource = { url: string; title: string | null }

/**
 * The tree with its references resolved, as CommonMark reads a document:
 * each `linkReference` and `imageReference` whose identifier a definition
 * has becomes the `link` or the `image` that definition gives (the first
 * definition of an identifier wins), one no definition has becomes its
 * source text again (`[`, its text, `]`, then `[label]` for a full reference
 * and `[]` for a collapsed one, as mdast-util-to-hast reverts one), and the
 * definitions leave the tree. Adjacent text is merged, as the parse merges
 * it. A new tree; the one given is not changed.
 *
 * The parse reads a bracketed label as a reference whether or not the
 * document defines it, so where one with no definition decided how the text
 * around it nests (emphasis that would have crossed it, a link whose text
 * holds it), the result is the tree that reading gives, not the one
 * CommonMark gives: the loss the manifest declares. Every other document
 * resolves to the tree CommonMark reads.
 */
export function resolveReferences(doc: DocumentNode): DocumentNode {
  const defs = new Map<string, Resource>()
  collectDefinitions(doc, defs)
  return resolveNode(doc, defs) as DocumentNode
}

/** Every definition in the tree, the first of each identifier kept. */
function collectDefinitions(root: any, defs: Map<string, Resource>): void {
  const stack: any[] = [root]
  while (0 < stack.length) {
    const node = stack.pop()
    if ('definition' === node.type) {
      if (!defs.has(node.identifier)) {
        defs.set(node.identifier, { url: node.url, title: node.title })
      }
      continue
    }
    const children = node.children
    if (Array.isArray(children)) {
      for (let i = children.length - 1; 0 <= i; i--) stack.push(children[i])
    }
  }
}

/** The source text a reference no definition has reads as. */
function referenceSuffix(node: any): string {
  return 'full' === node.referenceType ? '[' + node.label + ']' : 'collapsed' === node.referenceType ? '[]' : ''
}

/**
 * A node with its references resolved. Iterative over the containers, since
 * block nesting is the input's to choose.
 */
function resolveNode(root: any, defs: Map<string, Resource>): any {
  type Frame = { node: any; out: any[]; i: number; built: any }
  const make = (node: any): Frame => {
    const built: any = {}
    for (const k of Object.keys(node)) if ('children' !== k) built[k] = node[k]
    return { node, out: [], i: 0, built }
  }
  const finish = (f: Frame): any => {
    const out: any = {}
    for (const k of Object.keys(f.node)) out[k] = 'children' === k ? f.out : f.built[k]
    return out
  }
  const push = (out: any[], node: any): void => {
    const last = out[out.length - 1]
    if ('text' === node.type && undefined !== last && 'text' === last.type && last !== node) {
      out[out.length - 1] = { type: 'text', value: last.value + node.value }
    } else {
      out.push(node)
    }
  }

  const stack: Frame[] = [make(root)]
  let result: any = null
  while (0 < stack.length) {
    const f = stack[stack.length - 1]
    const children = f.node.children
    if (f.i >= children.length) {
      stack.pop()
      const done = finish(f)
      if (0 === stack.length) {
        result = done
        break
      }
      const parent = stack[stack.length - 1]
      const node = f.node
      if ('linkReference' === node.type) {
        const res = defs.get(node.identifier)
        if (undefined !== res) {
          push(parent.out, { type: 'link', url: res.url, title: res.title, children: done.children })
        } else {
          push(parent.out, { type: 'text', value: '[' })
          for (const c of done.children) push(parent.out, c)
          push(parent.out, { type: 'text', value: ']' + referenceSuffix(node) })
        }
      } else {
        push(parent.out, done)
      }
      continue
    }
    const child = children[f.i++]
    if ('definition' === child.type) continue
    if ('imageReference' === child.type) {
      const res = defs.get(child.identifier)
      if (undefined !== res) {
        push(f.out, { type: 'image', url: res.url, title: res.title, alt: child.alt })
      } else {
        push(f.out, { type: 'text', value: '![' + child.alt + ']' + referenceSuffix(child) })
      }
      continue
    }
    if (Array.isArray(child.children)) {
      stack.push(make(child))
      continue
    }
    push(f.out, child)
  }
  return result
}
