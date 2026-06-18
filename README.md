# @tabnas/markdown

This plugin allows the [Tabnas](https://jsonic.senecajs.org) JSON parser to support markdown syntax.

This repository contains:

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation. |
| [`go/`](go/) | Go port. |
| [`test/fixtures/`](test/fixtures/) | Shared conformance fixtures, exercised by both runtimes. |

See [`ts/README.md`](ts/README.md) for usage.

## Grammar

The grammar is defined once in the top-level
[`markdown-grammar.jsonic`](markdown-grammar.jsonic) and embedded into both the
TypeScript ([`ts/src/markdown.ts`](ts/src/markdown.ts)) and Go
([`go/markdown.go`](go/markdown.go)) implementations by
[`ts/embed-grammar.js`](ts/embed-grammar.js) (run as part of `npm run build`).
Edit the grammar file, then re-embed — never edit the embedded copies directly.

## Grammar diagram

The grammar as a railroad/syntax diagram, generated from the live grammar
with [`@tabnas/railroad`](https://github.com/tabnas/railroad):

![markdown grammar railroad diagram](ts/doc/grammar.svg)

ASCII version: [`ts/doc/grammar.txt`](ts/doc/grammar.txt).

## License

MIT. Copyright (c) Richard Rodger.
