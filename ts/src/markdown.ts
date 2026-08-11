/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// @tabnas/markdown — a CommonMark 0.31.2 parser, built as a tabnas plugin on
// the bare engine.
//
// This file is only the plugin wiring and the public surface. The parser
// itself is engine-free and lives in `commonmark.ts` and the modules it pulls
// in (`block.ts`, `inline.ts`, `html.ts`, `common.ts`, `node.ts`), so it can
// be exercised — and the conformance suite run — without `@tabnas/parser`
// installed. Nothing under `commonmark.ts` may import the engine.
//
// The parse result is a native CommonMark node tree; `ast.ts` projects it to
// the mdast-adjacent JSON AST this package has always returned, and `html.ts`
// renders it for `toHtml`.

// `Tabnas` is the only runtime binding needed here; the rest are types, so
// they are imported as such. Without `import type`, Node's type stripping
// keeps them in the emitted import and demands runtime exports that do not
// exist.
import type { Tabnas, Plugin, Rule, Context } from '@tabnas/parser'

import { parse } from './commonmark.ts'
import { toAst } from './ast.ts'
import { renderHTML } from './html.ts'
import { resolveOptions } from './options.ts'
import type { ParserOptions } from './options.ts'
import { MdNode } from './node.ts'

// ---------------------------------------------------------------------------
// Public options

export type MarkdownOptions = {
  /**
   * Enable the GFM extensions — tables, strikethrough, task list items,
   * autolink literals and the disallowed-raw-HTML filter. Default true.
   */
  gfm?: boolean
  /** When true, a single soft break becomes a hard break. Default false. */
  breaks?: boolean
}

// ---------------------------------------------------------------------------
// Grammar source.
//
// The prose grammar is intentionally trivial: block structure is decided by
// the line-oriented algorithm in `block.ts`, not by declarative alts, so the
// `markdown` rule exists only to consume the token stream. The file is kept
// because `ts/embed-grammar.js` embeds it verbatim into both runtimes; see
// AGENTS.md. It is documentation of the entry rule, not a source of behaviour.

// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
const grammarText = `
# Markdown entry rule — CommonMark 0.31.2
#
# This grammar is deliberately trivial, and it is NOT where the parser
# lives. Block structure is decided by the two-phase line algorithm of
# spec Appendix A, implemented in block.ts / block.go, and inline
# structure by the delimiter and bracket stacks in inline.ts / inline.go.
# None of that is expressible as declarative alts, which is why none of
# it is here.
#
# The markdown rule exists only to give the engine an entry point and to
# consume the token stream so its trailing-content check passes. The bo
# action reads the whole source via ctx.src() and hands it to the parser.
# Everything the rule does is in those two lines below.
#
# Consequences worth knowing rather than discovering:
#   - The railroad diagram generated from this file is empty: a bare
#     track with no boxes on it. That is accurate, not a rendering fault.
#   - Editing this file changes documentation, not behaviour. The one
#     exception is the open/close alts, which really are the rule.
#
# The file is kept because ts/embed-grammar.js embeds it verbatim into
# both runtimes as grammarText (see AGENTS.md). It may not contain
# backticks: the Go copy is a raw string literal.
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
// Public parse API (engine-free)

/** Parse Markdown to the mdast-adjacent JSON AST. */
export function parseDocument(src: string, opts?: MarkdownOptions | ParserOptions) {
  const options = resolveOptions(opts)
  return toAst(parse(src, options), options)
}

/** Parse a single run of inline Markdown to an `Inline[]`. */
export function parseInline(text: string, opts?: MarkdownOptions | ParserOptions) {
  const doc = parseDocument(text, opts)
  const first = doc.children[0]
  return first && 'paragraph' === first.type ? first.children : []
}

/** Parse Markdown and render it to CommonMark-conformant HTML. */
export function toHtml(src: string, opts?: MarkdownOptions | ParserOptions): string {
  const options = resolveOptions(opts)
  return renderHTML(parse(src, options), options)
}

/** Parse Markdown to the native CommonMark node tree. */
export function parseTree(src: string, opts?: MarkdownOptions | ParserOptions): MdNode {
  return parse(src, resolveOptions(opts))
}

// ---------------------------------------------------------------------------
// Plugin wiring
//
// The `markdown` rule's `bo` action reads the whole source via `ctx.src()` and
// hands it to the parser; the alts then consume the token stream so the
// engine's trailing-content check (lex must end at #ZZ) passes. `#AA` is the
// ANY-token wildcard and `r:'markdown'` loops at the same depth, consuming one
// token per iteration until only #ZZ remains.

const Markdown: Plugin = (tn: Tabnas, options?: MarkdownOptions) => {
  const opts = resolveOptions(options)

  tn.options({ rule: { start: 'markdown' } })

  // The parser reads `ctx.src()` directly rather than the token stream, so
  // disable the lexers that would corrupt Markdown syntax before it gets
  // there: backtick code spans lex as unterminated strings, `# heading` as a
  // comment, and `1. list` as a number.
  tn.options({
    string: { lex: false },
    comment: { lex: false },
    number: { lex: false },
    value: { lex: false },
    lex: { emptyResult: { type: 'document', children: [] } },
  })

  const refs: Record<string, any> = {
    '@markdown-bo': (r: Rule, ctx: Context) => {
      const src: string = ctx.src()
      r.node = parseDocument(src, opts)
    },
  }

  tn.rule('markdown', (rs) => {
    return rs
      .clear()
      .bo(refs['@markdown-bo'] as any)
      .open([{ s: '#AA', r: 'markdown' }, {}])
      .close([{ s: '#ZZ' }, {}])
  })

  // Not part of the tabnas contract, but useful for tests, docs and callers
  // that want HTML or the native tree without a second import.
  ;(tn as any).markdown = {
    parseDocument: (t: string) => parseDocument(t, opts),
    parseInline: (t: string) => parseInline(t, opts),
    toHtml: (t: string) => toHtml(t, opts),
    parseTree: (t: string) => parseTree(t, opts),
  }
}

Markdown.defaults = { gfm: true, breaks: false } as MarkdownOptions

export { VERSION, Markdown, grammarText }

export type {
  DocumentNode,
  Block,
  HeadingNode,
  ParagraphNode,
  BlockquoteNode,
  ListNode,
  ListItemNode,
  CodeNode,
  HtmlNode,
  ThematicBreakNode,
  TableNode,
  TableRowNode,
  TableCellNode,
  Inline,
  TextNode,
  EmphasisNode,
  StrongNode,
  InlineCodeNode,
  LinkNode,
  ImageNode,
  BreakNode,
  DeleteNode,
} from './ast.ts'

export { MdNode } from './node.ts'
export type { ParserOptions } from './options.ts'

// Re-exported so the package entry offers the same pair as the Go package,
// where `RenderHTML` is exported alongside `ToHTML`. Without this, rendering a
// tree you built or mutated yourself meant a deep import of `commonmark.ts`,
// and the two runtimes' public surfaces did not match.
export { renderHTML } from './html.ts'

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and the version test fails the
// build if they drift. Mirrors `const VERSION` in go/markdown.go.
const VERSION = '0.6.2'
