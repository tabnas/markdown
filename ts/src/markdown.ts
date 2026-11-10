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

// `Tabnas` is the only runtime binding needed here (`make` builds one); the
// rest are types, so they are imported as such. Without `import type`,
// Node's type stripping keeps them in the emitted import and demands runtime
// exports that do not exist.
import { Tabnas } from '@tabnas/parser'
import type { Plugin } from '@tabnas/parser'

import {
  END_TOKEN,
  LINE_TOKEN,
  NODE_RULES,
  makeMdActions,
  makeMdBlockMatcher,
} from './engine-block.ts'
import { makeInlineTn, parseBlockEngine, parseInlinesEngine } from './engine-inline.ts'
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
// The grammar is LIVE and lives in code: the rules below (`markdown`, the
// `children` loop, and a rule for each node of the AST), the `mdBlock`
// matcher (engine-block.ts) and the inline instance's rule and matcher set
// (engine-inline.ts). `debug.model()` serializes it and
// `ts/tools/gen-railroad.mjs` draws ts/doc/grammar.{svg,txt} from a live
// instance, so the documentation is derived from the rules the engine
// actually parses with. The historical `markdown-grammar.jsonic` file and
// its embed step are gone: they documented an inert single-rule bypass that
// no longer exists (dx-report §47).

// ---------------------------------------------------------------------------
// Public parse API (engine-free)

/**
 * Parse Markdown to the mdast-adjacent JSON AST. References stay
 * unresolved, as mdast keeps them: a bracketed label is a `linkReference`
 * or an `imageReference`, and each definition a `definition` node where it
 * stands. `resolveReferences` resolves them.
 */
export function parseDocument(src: string, opts?: MarkdownOptions | ParserOptions) {
  const options = resolveOptions(opts)
  return toAst(parse(src, options, true), options)
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
// The engine parse IS the parse, and its rules open and close every block:
// the `mdBlock` matcher (engine-block.ts) cuts one `#LB` token per physical
// line through the lexer's window, runs the shared block algorithm over it,
// and hands out ahead of it the tokens of the blocks that line closed and
// opened;
// each node of the AST is a rule that opens on its token and, for a
// container, closes on `#EN`; and the document's children, like every
// container's, are the `children` rule, a replace loop, so ten thousand
// blocks run at the depth one does. The block decisions are the shared
// engine-free core's (the anti-drift rule, dx-report §42), which is what
// keeps this path's AST identical to `parseDocument`'s (asserted by the
// differential gate and the engine-path conformance harness).

const Markdown: Plugin = (tn: Tabnas, options?: MarkdownOptions) => {
  const opts = resolveOptions(options)

  // Human descriptions for the block tokens, surfaced in railroad diagram
  // legends (read off the live config by @tabnas/railroad).
  const tokenDesc: Record<string, string> = {
    [LINE_TOKEN]: 'one physical line (the blocks it makes follow it)',
    [END_TOKEN]: "a container's end, with the fields after its children",
  }
  for (const n of NODE_RULES) {
    tokenDesc[n.token] = n.desc + (n.container ? ' opens, with its fields' : ', whole')
  }
  tn.options({
    config: {
      modify: {
        'md-tokendesc': (cfg: any) => {
          cfg.tokenDesc = Object.assign(cfg.tokenDesc || {}, tokenDesc)
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
  // the complete lexical description of this phase: `mdBlock`, at an order
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
      match: {
        mdBlock: {
          order: 1e5,
          make: makeMdBlockMatcher(opts, (st, block) =>
            parseBlockEngine(inlineTn, st.inline, block),
          ),
        },
      },
    },
    parse: { prepare: { 'md-reset': actions.prepare } },
  })

  const lineTin = tn.token(LINE_TOKEN)
  const endTin = tn.token(END_TOKEN)
  const slotTins = [lineTin, ...NODE_RULES.map((n) => tn.token(n.token))]

  // The document: its children, then the end of the source.
  tn.rule('markdown', (rs) => {
    return rs
      .clear()
      .open([{ p: 'children', a: actions.begin }])
      .close([{ s: '#ZZ', a: actions.finish }])
  })

  // A container's children, one node or one line per iteration: a node's
  // token pushes that node's rule, a line is consumed, and the loop goes on
  // by replacement (`r`), never by a push chain, while the next token is
  // either; anything else (`#EN`, `#ZZ`) ends it. The GFM nodes are the
  // `gfm` group's alts, which `gfm: false` subtracts.
  tn.rule('children', (rs) => {
    const open: any[] = NODE_RULES.map((n) => {
      const alt: any = { s: n.token, b: 1, p: n.rule }
      if (n.gfm) alt.g = 'gfm'
      return alt
    })
    open.push({ s: LINE_TOKEN }, {})
    return rs
      .clear()
      .open(open)
      .close([{ s: [slotTins], b: 1, r: 'children' }, {}])
  })

  // A rule per node: a container opens on its token, pushes its children,
  // and closes on `#EN`; a leaf opens on its token and closes at once.
  for (const n of NODE_RULES) {
    tn.rule(n.rule, (rs) => {
      rs.clear()
      return n.container
        ? rs
            .open([{ s: n.token, p: 'children', a: actions.open }])
            .close([{ s: [[endTin]], a: actions.close }])
        : rs.open([{ s: n.token, a: actions.leaf }]).close([{}])
    })
  }

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

/**
 * A tabnas engine with the Markdown plugin installed: the parser this
 * package makes whole, `new Tabnas().use(Markdown, opts)`, reading a source
 * a window at a time. The one matcher cuts a line through the lexer's
 * window and fills it past the line's end, and nothing else reads the
 * source, so a parse from a reader (`tn.parseReader`, admin ADR-30) holds
 * the text of the lines it has not finished with, and not the whole text.
 * The option is set here, on the parser this package makes, not in the
 * plugin.
 */
function make(opts?: MarkdownOptions): Tabnas {
  const tn = new Tabnas().use(Markdown, opts)
  tn.options({ lex: { window: true } })
  return tn
}

export { VERSION, Markdown, make }

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
  LinkReferenceNode,
  ImageReferenceNode,
  DefinitionNode,
} from './ast.ts'

export { resolveReferences } from './ast.ts'

export { MdNode } from './node.ts'
export type { ParserOptions } from './options.ts'

// Re-exported so the package entry offers the same pair as the Go package,
// where `RenderHTML` is exported alongside `ToHTML`. Without this, rendering a
// tree you built or mutated yourself meant a deep import of `commonmark.ts`,
// and the two runtimes' public surfaces did not match.
export { renderHTML } from './html.ts'

export { translate } from './translate'
export type { TranslationPart, TranslationParts } from './translate'

// VERSION is this package's version. It MUST equal package.json "version":
// the release orchestrator rewrites both, and the version test fails the
// build if they drift. Mirrors `const VERSION` in go/markdown.go.
const VERSION = '0.7.14'
