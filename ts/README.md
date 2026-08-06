# @tabnas/markdown (TypeScript)

A [Tabnas](https://github.com/tabnas/parser) grammar plugin that parses **CommonMark / GFM Markdown** into a JSON AST. Built on the bare `@tabnas/parser` engine.

[![npm version](https://img.shields.io/npm/v/@tabnas/markdown.svg)](https://npmjs.com/package/@tabnas/markdown)
[![build](https://github.com/tabnas/markdown/actions/workflows/build.yml/badge.svg)](https://github.com/tabnas/markdown/actions/workflows/build.yml)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |

## Install

```bash
npm install @tabnas/markdown @tabnas/parser
```

`@tabnas/parser` (>=0) is the only peer dependency. Requires Node >=24.

## Example

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# Hello\n\nHello *world*') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }, { type: 'paragraph', children: [{ type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [{ type: 'text', value: 'world' }] }] }] }
j.parse('# Hello') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
```

## Documentation

- [Tutorial](doc/tutorial.md) — first parse in 5 minutes.
- [How-to guide](doc/guide.md) — task recipes.
- [Reference](doc/reference.md) — API + options + AST.
- [Concepts](doc/concepts.md) — how it works.
