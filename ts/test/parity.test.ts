/* Copyright (c) 2025 Richard Rodger and other contributors, MIT License */

// Cross-runtime conformance, driven by the shared `test/spec/*.tsv` fixtures
// at the repo root (see ../../test/AGENTS.md).
//
// The fixture loader, the escape codec, the `ERROR:<code>` contract and the
// row loop all come from @tabnas/support, whose Go and Rust halves
// `go/parity_test.go` and `rs/tests/parity_test.rs` use to run the SAME
// files — so the three implementations cannot drift without one of them
// going red, and neither can the three loaders.
//
// What is left here is only what is specific to markdown: how to build the
// parser for a row's options.

import { Tabnas } from '@tabnas/parser'
import { findSpecDir, makeRunner } from '@tabnas/support'

import { Markdown } from '../dist/markdown'

makeRunner({
  // A fresh Tabnas per row: the `opts` column is per-case, and plugin
  // options must not leak from one row into the next.
  parse: (input, row) => {
    const opts = row.named('opts')
    return new Tabnas()
      .use(Markdown, '' === opts.trim() ? {} : JSON.parse(opts))
      .parse(input)
  },
})
  // `findSpecDir` walks up from this file — `dist-test/` at runtime — to the
  // repo root's `test/spec`, so moving the suite does not mean recounting
  // `..` hops. `dir` then auto-discovers every fixture in it, so adding a
  // .tsv runs it in every runtime without touching any runner.
  .dir(findSpecDir(__dirname))
