/* Copyright (c) 2026 Richard Rodger, MIT License */

// The engine-facing block driver: a custom lexer matcher that cuts the source
// one physical line at a time, runs the shared block algorithm over each line,
// and hands the engine the blocks that line opened and closed as tokens; and
// the rule actions that build the AST from those tokens.
//
// The grammar's rules open and close every block (markdown.ts): a container's
// rule opens on the token that opens it (`#BQ` for a block quote, `#LS` for a
// list, ...) and closes on `#EN`, and a leaf's rule opens and closes on its
// one token (`#HR`, `#CD`, `#DF`, ...). A block's inline content is parsed
// when the block closes, since the block bounds it, and its inline nodes are
// tokens and rules too, so the rule passes carry the whole tree, a block at a
// time, and the value the rules build is that tree.
//
// The block decisions themselves are the shared `BlockParser`'s, the same
// code the engine-free path runs (the anti-drift rule, dx-report §42): which
// line continues which block, where a block starts, lazy continuation, and
// when a block is finished. This driver turns what it decided into tokens in
// the order it decided it (`BlockParser.events`), and nothing here recognizes
// any Markdown of its own.
//
// Each line is one `#LB` token, cut through the lexer's window, so a grammar
// that declares `lex.window` is read a window at a time. The tokens of the
// blocks the line closed and opened come first, of no width, where the line
// starts, one each time the lexer asks, and then the line itself: the tokens
// are in source order, a block that ends before the line closing there and
// one the line starts opening there. A container opens on the line that
// starts it and closes on the line that ends it; a leaf block is written
// whole when it closes, a paragraph or a heading with the inline nodes its
// text makes, and a table a row at a time. The last line's tokens also close
// every block still open, since no token follows the last line but the end.
//
// This module may import engine types: it is reachable only from
// `markdown.ts`, never from `commonmark.ts`, so the engine-free layering
// rule holds (see AGENTS.md).

import type { Context, MakeLexMatcher, Rule, Tabnas } from '@tabnas/parser'

import { BlockParser } from './block.ts'
import type { BlockEvent } from './block.ts'
import { blockHead, blockTail, definitionNode, inlineChildren, leafBlock } from './ast.ts'
import type { Inline } from './ast.ts'
import { InlineParser } from './inline.ts'
import { MdNode } from './node.ts'
import type { ParserOptions } from './options.ts'

/** One physical line, read: the blocks it made follow it. */
export const LINE_TOKEN = '#LB'

/** A container's end: its value is the fields that follow its children. */
export const END_TOKEN = '#EN'

/**
 * A node of the AST, its rule and the token its rule opens on. A container's
 * token carries its head (the fields before its `children`, `type` first),
 * and its `#EN` its tail; a leaf's token carries the whole node. The rule is
 * named for the node's type. The GFM nodes are the `gfm` group's.
 */
export type NodeRule = {
  rule: string
  token: string
  container: boolean
  gfm: boolean
  desc: string
}

export const NODE_RULES: NodeRule[] = [
  { rule: 'blockquote', token: '#BQ', container: true, gfm: false, desc: 'block quote' },
  { rule: 'list', token: '#LS', container: true, gfm: false, desc: 'list' },
  { rule: 'listItem', token: '#LI', container: true, gfm: false, desc: 'list item' },
  { rule: 'paragraph', token: '#PA', container: true, gfm: false, desc: 'paragraph' },
  { rule: 'heading', token: '#HD', container: true, gfm: false, desc: 'ATX or setext heading' },
  { rule: 'table', token: '#TB', container: true, gfm: true, desc: 'GFM table' },
  { rule: 'tableRow', token: '#TR', container: true, gfm: true, desc: 'table row' },
  { rule: 'tableCell', token: '#TC', container: true, gfm: true, desc: 'table cell' },
  { rule: 'emphasis', token: '#EM', container: true, gfm: false, desc: 'emphasis' },
  { rule: 'strong', token: '#SG', container: true, gfm: false, desc: 'strong emphasis' },
  { rule: 'delete', token: '#DL', container: true, gfm: true, desc: 'strikethrough' },
  { rule: 'link', token: '#LK', container: true, gfm: false, desc: 'link' },
  { rule: 'linkReference', token: '#LR', container: true, gfm: false, desc: 'link reference' },
  { rule: 'thematicBreak', token: '#HR', container: false, gfm: false, desc: 'thematic break' },
  { rule: 'code', token: '#CD', container: false, gfm: false, desc: 'fenced or indented code' },
  { rule: 'html', token: '#HT', container: false, gfm: false, desc: 'HTML block or raw HTML' },
  { rule: 'definition', token: '#DF', container: false, gfm: false, desc: 'link reference definition' },
  { rule: 'text', token: '#TT', container: false, gfm: false, desc: 'text' },
  { rule: 'inlineCode', token: '#IC', container: false, gfm: false, desc: 'code span' },
  { rule: 'break', token: '#BR', container: false, gfm: false, desc: 'hard line break' },
  { rule: 'image', token: '#IM', container: false, gfm: false, desc: 'image' },
  { rule: 'imageReference', token: '#IR', container: false, gfm: false, desc: 'image reference' },
]

/** The token a node of each AST type opens with. */
const TOKEN_OF: Record<string, string> = Object.fromEntries(
  NODE_RULES.map((n) => [n.rule, n.token]),
)

/** A token the line matcher makes: its name and its value. */
type Made = [string, unknown]

/** The per-parse state carried on `ctx.u.md`, seeded by `parse.prepare`. */
export type MdParseState = {
  bp: BlockParser
  meta: Record<string, any> | undefined
  /** The caller asked for the native tree back (`meta.md.keepTree`). */
  keepTree: boolean
  /** The inline parse a block's AST is made with: references kept. */
  inline: InlineParser
  /**
   * The tokens the line at the cursor made, handed out one a call before
   * the line's own token (`next` the one due), and whether that line has
   * been read: its blocks are decided, and its `#LB` is what is left.
   */
  made: Made[]
  next: number
  read: boolean
}

/**
 * One physical line at the cursor, read through the lexer's window: up to
 * (not including) the next `\n`, `\r` or `\r\n`, with §2.3's NUL
 * replacement. `len` covers the terminator, `last` says whether the source
 * ends after it (which is why a final line ending does not make a trailing
 * blank line). The window is filled past the line's end, so a token never
 * ends at the window's end, where the lexer would cut it again over a wider
 * one. A line is read twice, once to decide its blocks and once for its own
 * token, and gives the same both times.
 */
function cutLine(
  lex: any,
): { text: string; src: string; len: number; terminated: boolean; last: boolean } | null {
  let w: string = lex.window()
  if (0 === w.length) {
    if (!lex.fill(1)) return null
    w = lex.window()
  }

  // A line longer than the window is read on in doubling steps, so that the
  // window's text is fetched a logarithmic number of times, not once a read.
  let i = 0
  for (;;) {
    while (i < w.length) {
      const c = w.charCodeAt(i)
      if (10 === c || 13 === c) break
      i++
    }
    if (i < w.length) break
    const had = w.length
    lex.fill(2 * had + 1)
    w = lex.window()
    if (w.length === had) break
  }

  let next = i
  let terminated = false
  if (i < w.length) {
    terminated = true
    if (13 === w.charCodeAt(i)) {
      if (i + 1 >= w.length && lex.fill(i + 2)) w = lex.window()
      next = i + 1 < w.length && 10 === w.charCodeAt(i + 1) ? i + 2 : i + 1
    } else {
      next = i + 1
    }
  }

  const last = !lex.fill(next + 1)
  if (!last) w = lex.window()

  let text = w.slice(0, i)
  if (-1 !== text.indexOf('\u0000')) text = text.replace(/\0/g, '�')
  return { text, src: w.slice(0, next), len: next, terminated, last }
}

/**
 * Build the `mdBlock` matcher. Each invocation returns one token. At a line
 * not yet read, it reads the line, feeds it to the shared `BlockParser`
 * (and, for the last line, closes every block still open), and gathers the
 * tokens of the blocks the line closed and opened; those are handed out
 * first, of no width, where the line starts, and then the line itself as a
 * `#LB` token. The matcher owns the engine's point: `sI` advances past the
 * terminator with the `#LB`, `rI` counts rows, and `cI` resets per line (the
 * block algorithm computes spec §2.2 tab-expanded columns itself). Nothing
 * reads the source but `cutLine`, through the window.
 */
export function makeMdBlockMatcher(
  opts: ParserOptions,
  parseBlock: (st: MdParseState, block: MdNode) => void,
): MakeLexMatcher {
  return (_cfg, _opts) => {
    return (lex: any, _rule: Rule) => {
      const st: MdParseState | undefined = lex.ctx && lex.ctx.u && lex.ctx.u.md
      if (undefined === st) return undefined

      const pnt = lex.pnt
      if (st.next < st.made.length) {
        const [name, val] = st.made[st.next++]
        return lex.token(name, val, '', pnt)
      }

      const line = cutLine(lex)
      if (null === line) return undefined

      if (!st.read) {
        st.bp.incorporateLine(line.text)
        if (line.last) st.bp.finish()
        const made: Made[] = []
        const events = st.bp.events as BlockEvent[]
        for (let i = 0; i < events.length; i++) eventTokens(st, opts, parseBlock, events[i], made)
        events.length = 0
        if (0 < made.length) {
          st.made = made
          st.next = 1
          st.read = true
          return lex.token(made[0][0], made[0][1], '', pnt)
        }
      }

      st.made = []
      st.next = 0
      st.read = false

      const tkn = lex.token(LINE_TOKEN, undefined, line.src, pnt)

      // Row/column bookkeeping is observability data (token positions,
      // traces): a consumed terminator starts a new row; an unterminated
      // final line just widens the current one.
      pnt.sI += line.len
      if (line.terminated) {
        pnt.rI += 1
        pnt.cI = 1
      } else {
        pnt.cI += line.text.length
      }

      return tkn
    }
  }
}

/** The tokens one block event makes, appended to `out`. */
function eventTokens(
  st: MdParseState,
  opts: ParserOptions,
  parseBlock: (st: MdParseState, block: MdNode) => void,
  ev: BlockEvent,
  out: Made[],
): void {
  switch (ev.kind) {
    case 'open': {
      const head = blockHead(ev.node)
      out.push([TOKEN_OF[head.type as string], head])
      return
    }

    case 'row': {
      out.push(['#TR', blockHead(ev.node)])
      for (let cell = ev.node.firstChild; cell; cell = cell.next) {
        inlineBlockTokens(st, opts, parseBlock, cell, out)
      }
      out.push([END_TOKEN, blockTail(ev.node)])
      return
    }

    case 'definition': {
      out.push(['#DF', definitionNode(ev.def)])
      return
    }

    case 'close': {
      const node = ev.node
      switch (node.type) {
        case 'block_quote':
        case 'list':
        case 'item':
        case 'table':
          out.push([END_TOKEN, blockTail(node)])
          return

        case 'paragraph':
        case 'heading':
          // A paragraph that held nothing but definitions is gone: its
          // definitions were written as they were read.
          if (null === node.parent) return
          inlineBlockTokens(st, opts, parseBlock, node, out)
          return

        case 'thematic_break':
        case 'code_block':
        case 'html_block': {
          const leaf = leafBlock(node)
          out.push([TOKEN_OF[leaf.type], leaf])
          if (!st.keepTree) release(node)
          return
        }

        default:
          return
      }
    }
  }
}

/**
 * A block whose content is inline nodes (a paragraph, a heading, a table
 * cell), parsed now: its head, its inline nodes, its end. The block's raw
 * text stays on the native node when the caller wants the native tree back,
 * which is parsed again, references resolved, once the document ends.
 */
function inlineBlockTokens(
  st: MdParseState,
  opts: ParserOptions,
  parseBlock: (st: MdParseState, block: MdNode) => void,
  node: MdNode,
  out: Made[],
): void {
  const head = blockHead(node)
  let target = node
  if (st.keepTree) {
    target = new MdNode(node.type, node.sourcepos)
    target.stringContent = node.stringContent
  }
  parseBlock(st, target)
  out.push([TOKEN_OF[head.type as string], head])
  inlineTokens(inlineChildren(target, opts), out)
  out.push([END_TOKEN, blockTail(node)])
  if (!st.keepTree) release(node)
}

/**
 * The tokens of a run of inline nodes, in document order: a container's
 * head, its children's, its end; a leaf whole. Iterative, since inline
 * nesting is the input's to choose.
 */
function inlineTokens(nodes: Inline[], out: Made[]): void {
  const stack: { list: any[]; i: number }[] = [{ list: nodes, i: 0 }]
  while (0 < stack.length) {
    const top = stack[stack.length - 1]
    if (top.i >= top.list.length) {
      stack.pop()
      if (0 < stack.length) out.push([END_TOKEN, {}])
      continue
    }
    const node: any = top.list[top.i++]
    if (Array.isArray(node.children)) {
      const head: Record<string, unknown> = {}
      for (const k of Object.keys(node)) if ('children' !== k) head[k] = node[k]
      out.push([TOKEN_OF[node.type], head])
      stack.push({ list: node.children, i: 0 })
    } else {
      out.push([TOKEN_OF[node.type], node])
    }
  }
}

/**
 * Drop what a written block holds that nothing reads again: its raw text,
 * its literal and its inline nodes. The block itself stays in the native
 * tree, whose later lines still read its flags.
 */
function release(node: MdNode): void {
  node.stringContent = ''
  node.literal = null
  node.firstChild = null
  node.lastChild = null
}

/**
 * The actions of the grammar's rules, bound to resolved options at plugin
 * install time. `resolveInlines` is the phase-2 driver that resolves
 * references (the engine inline path from `engine-inline.ts`), which a
 * `keepTree` parse runs over the native tree once the document ends.
 */
export function makeMdActions(
  opts: ParserOptions,
  resolveInlines: (doc: MdNode, refmap: Record<string, any>) => void,
) {
  return {
    /** `parse.prepare`: fresh block-parser state for every parse. */
    prepare: (_tn: Tabnas, ctx: Context, meta?: any) => {
      const bp = new BlockParser(opts)
      bp.events = []
      const keepTree = !!(meta && meta.md && true === meta.md.keepTree)
      const state: MdParseState = {
        bp,
        meta,
        keepTree,
        inline: new InlineParser({}, opts, true),
        made: [],
        next: 0,
        read: false,
      }
      ;(ctx.u as any).md = state
    },

    /** `markdown` open: the document's children gather here. */
    begin: (r: Rule) => {
      r.node = []
    },

    /**
     * `markdown` close (`#ZZ`): the document. When the caller passed
     * `meta.md.keepTree`, the native tree, with its references resolved,
     * is handed back on the meta object: the engine-path conformance
     * harness renders it.
     */
    finish: (r: Rule, ctx: Context) => {
      const state: MdParseState = (ctx.u as any).md
      const md = state.meta && state.meta.md
      if (state.keepTree && md) {
        resolveInlines(state.bp.doc, state.bp.refmap)
        md.tree = state.bp.doc
      }
      r.node = { type: 'document', children: r.node }
    },

    /** A container node's rule opens: its children gather in a new array. */
    open: (r: Rule) => {
      const u = r.u as any
      u.into = r.node
      u.head = (r.o0 as any).val
      r.node = []
    },

    /**
     * A container node's rule closes on `#EN`: its node is its head, its
     * children and its tail, in that order, added to its parent's children.
     */
    close: (r: Rule) => {
      const u = r.u as any
      const node: Record<string, unknown> = Object.assign({}, u.head)
      node.children = r.node
      Object.assign(node, (r.c0 as any).val)
      u.into.push(node)
    },

    /** A leaf node's rule opens: the token's value is the node. */
    leaf: (r: Rule) => {
      ;(r.node as any).push((r.o0 as any).val)
    },
  }
}
