/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// Types shared by both parse phases. Kept in their own module so `block.ts`
// and `inline.ts` can depend on them without importing `commonmark.ts`, which
// imports them in turn.

export type ParserOptions = {
  /**
   * Enable the GFM extensions. Default true.
   *
   * Five are implemented, and this one flag gates all of them together:
   * tables, strikethrough (`~~x~~`), task list items (`- [x] foo`), autolink
   * literals (bare `www.` / `http://` / `https://` / `ftp://` / `a@b.co`) and
   * the disallowed-raw-HTML filter. Footnotes are not implemented.
   *
   * The first four are parse-time; the raw-HTML filter is applied by the
   * renderer, which is why `html.ts` reads this option too. With `gfm:false`
   * the output is plain CommonMark, byte for byte.
   */
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
