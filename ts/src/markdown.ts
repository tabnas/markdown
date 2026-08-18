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
import type { Tabnas, Plugin } from '@tabnas/parser'

import { makeMdActions, makeMdLineMatcher } from './engine-block.ts'
import { makeInlineTn, parseInlinesEngine } from './engine-inline.ts'
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
// Grammar.
//
// The grammar is LIVE and lives in code: the `markdown`/`line` rules below,
// the `mdLine` matcher (engine-block.ts) and the inline instance's rule and
// matcher set (engine-inline.ts). `debug.model()` serializes it and
// `ts/tools/gen-railroad.mjs` draws ts/doc/grammar.{svg,txt} from a live
// instance, so the documentation is derived from the rules the engine
// actually parses with. The historical `markdown-grammar.jsonic` file and
// its embed step are gone: they documented an inert single-rule bypass that
// no longer exists (dx-report §47).

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
// The engine parse IS the parse: the `mdLine` custom matcher (engine-block.ts)
// lexes one `#LB` token per physical line, `parse.prepare` seeds a fresh
// `BlockParser` on `ctx.u.md`, the `line` rule feeds each matched line to the
// shared `incorporateLine`, and the `markdown` rule's close action on `#ZZ`
// finalizes, runs the shared inline phase, and projects the public AST into
// the rule's node. No recognition or block decision happens here — the
// engine owns tokenization, dispatch and state carriage; the algorithm stays
// in the engine-free core (the anti-drift rule, dx-report §42), which is
// what keeps this path byte-identical to `parseDocument` (asserted by the
// differential gate and the engine-path conformance harness).

const Markdown: Plugin = (tn: Tabnas, options?: MarkdownOptions) => {
  const opts = resolveOptions(options)

  // Human descriptions for the block token, surfaced in railroad diagram
  // legends (read off the live config by @tabnas/railroad).
  tn.options({
    config: {
      modify: {
        'md-tokendesc': (cfg: any) => {
          cfg.tokenDesc = Object.assign(cfg.tokenDesc || {}, {
            '#LB': 'one physical line (text, blank and table-arming bits in use)',
          })
        },
      },
    },
  })

  // The inline phase's own engine instance — nested parsing, one instance
  // per plugin install, reused across every parse and every leaf block (see
  // engine-inline.ts).
  const inlineTn = makeInlineTn(opts)
  const actions = makeMdActions(opts, (doc, refmap) =>
    parseInlinesEngine(inlineTn, doc, refmap, opts),
  )

  tn.options({ rule: { start: 'markdown' } })

  // Every built-in matcher is off — deliberate configuration, not defense.
  // Markdown's block alphabet is *lines*, so the one registered matcher is
  // the complete lexical description of this phase: `mdLine`, at an order
  // ahead of every built-in, consuming each physical line whole. (The
  // built-ins would misread the syntax anyway: backtick code spans lex as
  // unterminated strings, `# heading` as a comment, `1. list` as a number.)
  tn.options({
    space: { lex: false },
    line: { lex: false },
    string: { lex: false },
    comment: { lex: false },
    number: { lex: false },
    value: { lex: false },
    text: { lex: false },
    lex: {
      emptyResult: { type: 'document', children: [] },
      match: { mdLine: { order: 1e5, make: makeMdLineMatcher } },
    },
    parse: { prepare: { 'md-reset': actions.prepare } },
  })

  tn.rule('markdown', (rs) => {
    return rs
      .clear()
      .open([{ p: 'line' }])
      .close([{ s: '#ZZ', a: actions.finish }, {}])
  })

  // One `#LB` consumed per iteration, tail-recursing via `r:` until only
  // `#ZZ` remains; the empty alts are the fall-through that hands control
  // back to `markdown`'s close.
  //
  // The first alt is the GFM extension seam: tagged `g: 'gfm'`, gated on
  // the token's `tblArm` bit, arming the shared table probe for this line.
  // It is injected exactly the way a downstream dialect would extend this
  // grammar — an alt ahead of the base one, subtractable by its group tag —
  // and `gfm: false` removes it below via `rule.exclude`, so the base
  // dialect is the grammar minus the tagged alts, visibly (`debug.model()`).
  tn.rule('line', (rs) => {
    return rs
      .clear()
      .open([
        {
          s: '#LB',
          c: (_r: any, ctx: any) => true === ctx.t0.use.tblArm,
          r: 'line',
          a: actions.lineGfm,
          g: 'gfm',
        },
        { s: '#LB', r: 'line', a: actions.line },
        {},
      ])
      .close([{}])
  })

  // The GFM dialect is subtracted, not branched around: without the option,
  // the tagged alts above simply do not exist in this instance's grammar.
  if (true !== opts.gfm) {
    tn.options({ rule: { exclude: 'gfm' } })
  }

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

export { VERSION, Markdown }

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
const VERSION = '0.7.2'
