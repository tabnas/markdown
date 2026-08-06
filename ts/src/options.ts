/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Types shared by both parse phases. Kept in their own module so `block.ts`
// and `inline.ts` can depend on them without importing `commonmark.ts`, which
// imports them in turn.

export type ParserOptions = {
  /** GFM extensions: strikethrough, tables, task lists, autolink literals. */
  gfm: boolean
  /** Render soft line breaks as hard breaks. */
  breaks: boolean
}

export const DEFAULT_OPTIONS: ParserOptions = {
  gfm: true,
  breaks: false,
}

export function resolveOptions(opts?: Partial<ParserOptions>): ParserOptions {
  return {
    gfm: opts?.gfm ?? DEFAULT_OPTIONS.gfm,
    breaks: opts?.breaks ?? DEFAULT_OPTIONS.breaks,
  }
}

/** A resolved link reference definition (§4.7). */
export type RefDef = {
  destination: string
  title: string | null
}

/** Keys are labels put through `normalizeReference`. */
export type RefMap = Record<string, RefDef>
