/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Engine-free entry point for the CommonMark parser.
//
// Nothing reachable from here imports `@tabnas/parser`. That keeps the parser
// testable (and the conformance suite runnable) without the engine present,
// and it is what lets `ts/tools/conformance.mjs` execute the sources directly
// under Node's type stripping. The tabnas plugin wiring lives in
// `markdown.ts`, which imports this — never the other way round.

import { MdNode } from './node.ts'
import { parseBlocks } from './block.ts'
import { parseInlines } from './inline.ts'
import { resolveOptions } from './options.ts'
import type { ParserOptions } from './options.ts'

/**
 * Full two-phase parse (spec Appendix A): block structure first, then inlines
 * over the accumulated string content of each leaf, using the link reference
 * map the block phase collected.
 */
export function parse(input: string, opts?: Partial<ParserOptions>): MdNode {
  const options = resolveOptions(opts)
  const { doc, refmap } = parseBlocks(input, options)
  parseInlines(doc, refmap, options)
  return doc
}

export { MdNode } from './node.ts'
export { renderHTML } from './html.ts'
export { parseBlocks } from './block.ts'
export { parseInlines } from './inline.ts'
export { DEFAULT_OPTIONS, resolveOptions } from './options.ts'
export type { ParserOptions, RefMap, RefDef } from './options.ts'
