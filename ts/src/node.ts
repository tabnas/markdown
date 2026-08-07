/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Native CommonMark node tree — the internal parse result.
//
// This is deliberately a linked tree (parent/firstChild/lastChild/prev/next)
// rather than a children array, because both phases of the CommonMark
// algorithm (see spec 0.31.2 Appendix A, "A parsing strategy") splice nodes
// in and out mid-walk: the block phase keeps a spine of open blocks and
// closes them from the tail, and the inline phase's delimiter stack rewrites
// sibling runs in place. An array-of-children shape forces an index rewrite
// on every one of those operations.
//
// The public, mdast-adjacent AST that `@tabnas/markdown` has always returned
// is produced from this tree by `ast.ts`. That projection is lossy on
// purpose (it drops raw inline HTML boundaries and source positions); the
// tree is what `html.ts` renders for CommonMark conformance.

export type NodeType =
  // containers
  | 'document'
  | 'block_quote'
  | 'list'
  | 'item'
  // leaf blocks
  | 'paragraph'
  | 'heading'
  | 'thematic_break'
  | 'code_block'
  | 'html_block'
  // inlines
  | 'text'
  | 'softbreak'
  | 'linebreak'
  | 'code'
  | 'html_inline'
  | 'emph'
  | 'strong'
  | 'link'
  | 'image'
  | 'del'

const CONTAINER_TYPES: Record<string, true> = {
  document: true,
  block_quote: true,
  list: true,
  item: true,
  paragraph: true,
  heading: true,
  emph: true,
  strong: true,
  link: true,
  image: true,
  del: true,
}

export type ListType = 'bullet' | 'ordered'

export type ListData = {
  type: ListType
  /** Tight lists omit the <p> wrapper around item paragraphs. Decided at finalize. */
  tight: boolean
  /** Ordered-list start number; ignored for bullet lists. */
  start: number
  /** '.' or ')' for ordered lists — a change of delimiter starts a new list. */
  delimiter: string
  /** '-', '+' or '*' for bullet lists — a change of char starts a new list. */
  bulletChar: string
  /** Columns of padding between the marker and the item content. */
  padding: number
  /** Column at which the marker starts. */
  markerOffset: number
}

/** [startLine, startCol], [endLine, endCol] — all 1-based, as in the spec. */
export type SourcePos = [[number, number], [number, number]]

export class MdNode {
  type: NodeType

  parent: MdNode | null = null
  firstChild: MdNode | null = null
  lastChild: MdNode | null = null
  prev: MdNode | null = null
  next: MdNode | null = null

  sourcepos: SourcePos

  /** Text payload for text/code/html_block/html_inline/code_block. */
  literal: string | null = null

  /** Heading level 1..6. */
  level = 0

  /** Link/image destination, already entity-decoded and backslash-unescaped. */
  destination: string | null = null
  /** Link/image title, already entity-decoded and backslash-unescaped. */
  title: string | null = null

  /** Fenced code info string (raw, entity-decoded); null for indented code. */
  info: string | null = null
  /** True for fenced code blocks, false for indented. */
  isFenced = false
  fenceChar = ''
  fenceLength = 0
  fenceOffset = 0

  listData: ListData | null = null

  /**
   * GFM task list items (`- [x] foo`): `true`/`false` for a checked/unchecked
   * item, `null` for an ordinary one. Set on `item` nodes by the block phase
   * when `gfm` is on, once the marker has been consumed from the item's first
   * paragraph. mdast's field and mdast's semantics — `ast.ts` projects it
   * straight through to `listItem.checked`.
   */
  checked: boolean | null = null

  /**
   * The `gfm` parse option, recorded on the **document** node by the block
   * phase and meaningless anywhere else.
   *
   * The renderer needs it because GFM's disallowed-raw-HTML filter is a
   * render-time concern, and `renderHTML(tree)` may be called with no options
   * at all. Without this the default (`gfm: true`) would apply the filter to a
   * tree that was parsed as plain CommonMark, which is exactly the case the
   * conformance runner exercises.
   */
  gfm = true

  // --- block-phase bookkeeping (not part of the rendered tree) ---

  /** Open blocks accept further lines; closed ones are finalized. */
  open = true
  /** Accumulated raw content, consumed by the inline phase. */
  stringContent = ''
  /** A paragraph/heading whose last line was blank cannot be lazily continued. */
  lastLineBlank = false
  lastLineChecked = false

  constructor(type: NodeType, sourcepos?: SourcePos) {
    this.type = type
    this.sourcepos = sourcepos ?? [
      [0, 0],
      [0, 0],
    ]
  }

  /** Containers may hold children; leaves carry only `literal`. */
  isContainer(): boolean {
    return true === CONTAINER_TYPES[this.type]
  }

  appendChild(child: MdNode): void {
    child.unlink()
    child.parent = this
    if (this.lastChild) {
      this.lastChild.next = child
      child.prev = this.lastChild
      this.lastChild = child
    } else {
      this.firstChild = child
      this.lastChild = child
    }
  }

  prependChild(child: MdNode): void {
    child.unlink()
    child.parent = this
    if (this.firstChild) {
      this.firstChild.prev = child
      child.next = this.firstChild
      this.firstChild = child
    } else {
      this.firstChild = child
      this.lastChild = child
    }
  }

  insertAfter(sibling: MdNode): void {
    sibling.unlink()
    sibling.next = this.next
    if (sibling.next) sibling.next.prev = sibling
    sibling.prev = this
    this.next = sibling
    sibling.parent = this.parent
    if (!sibling.next && sibling.parent) sibling.parent.lastChild = sibling
  }

  insertBefore(sibling: MdNode): void {
    sibling.unlink()
    sibling.prev = this.prev
    if (sibling.prev) sibling.prev.next = sibling
    sibling.next = this
    this.prev = sibling
    sibling.parent = this.parent
    if (!sibling.prev && sibling.parent) sibling.parent.firstChild = sibling
  }

  /** Detach from the tree, leaving the node itself intact. */
  unlink(): void {
    if (this.prev) this.prev.next = this.next
    else if (this.parent) this.parent.firstChild = this.next

    if (this.next) this.next.prev = this.prev
    else if (this.parent) this.parent.lastChild = this.prev

    this.parent = null
    this.next = null
    this.prev = null
  }

  /** Depth-first walker. Containers are entered and exited; leaves once. */
  walker(): NodeWalker {
    return new NodeWalker(this)
  }
}

export type WalkEvent = { entering: boolean; node: MdNode }

export class NodeWalker {
  private current: MdNode | null
  private root: MdNode
  private entering = true

  constructor(root: MdNode) {
    this.current = root
    this.root = root
  }

  next(): WalkEvent | null {
    const cur = this.current
    const entering = this.entering
    if (null === cur) return null

    const container = cur.isContainer()

    if (entering && container) {
      if (cur.firstChild) {
        this.current = cur.firstChild
        this.entering = true
      } else {
        this.entering = false
      }
    } else if (cur === this.root) {
      this.current = null
    } else if (null === cur.next) {
      this.current = cur.parent
      this.entering = false
    } else {
      this.current = cur.next
      this.entering = true
    }

    return { entering, node: cur }
  }

  resumeAt(node: MdNode, entering: boolean): void {
    this.current = node
    this.entering = entering
  }
}
