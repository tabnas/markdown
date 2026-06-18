# @tabnas/markdown (TypeScript)

A [Tabnas](https://github.com/tabnas/parser) grammar plugin that parses
delimited record text — a header row, comma-separated fields, one record per
line, with RFC-4180 quoting — into arrays of objects or arrays. Despite the
name it is a configurable CSV/TSV-family reader, with support for headers,
quoted fields, custom delimiters, streaming, and strict / non-strict modes.

[![npm version](https://img.shields.io/npm/v/@tabnas/markdown.svg)](https://npmjs.com/package/@tabnas/markdown)
[![build](https://github.com/tabnas/markdown/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/markdown/actions/workflows/build.yml)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |

## Install

```bash
npm install @tabnas/markdown @tabnas/parser @tabnas/jsonic
```

`@tabnas/parser` (>=2) and `@tabnas/jsonic` are peer dependencies. Requires
Node >=24.

## Example

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('name,age\nAlice,30\nBob,25') // => [{ name: 'Alice', age: '30' }, { name: 'Bob', age: '25' }]

// quoted fields use "" to escape a quote:
j.parse('a\n"b""c"') // => [{ a: 'b"c' }]
```

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md) — a guided first run.
- [How-to guide](doc/guide.md) — task recipes (delimiters, no-header, streaming, errors, embedded JSON).
- [Reference](doc/reference.md) — the full API, every option, and the grammar accepted.
- [Concepts](doc/concepts.md) — how the plugin works on the engine and why.

For the Go version, see [../go/README.md](../go/README.md).

## Grammar diagram

The live grammar as a railroad diagram (regenerated with
[`@tabnas/railroad`](https://github.com/tabnas/railroad)):

![markdown grammar railroad diagram](doc/grammar.svg)

ASCII version: [`doc/grammar.txt`](doc/grammar.txt). The grammar source is the
top-level [`markdown-grammar.jsonic`](../markdown-grammar.jsonic), embedded by
[`embed-grammar.js`](embed-grammar.js) during `npm run build`.

## License

Copyright (c) 2021-2025 Richard Rodger and other contributors,
[MIT License](LICENSE).
