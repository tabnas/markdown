# @tabnas/markdown

A [Tabnas](https://github.com/tabnas/parser) grammar plugin that parses
delimited record text (header row, comma-separated fields, one record per line,
RFC-4180 quoting) into arrays of objects or arrays. Despite the name it is a
configurable CSV/TSV-family reader, available for both **TypeScript** and
**Go**, built on `@tabnas/parser` + `@tabnas/jsonic`.

```bash
# TypeScript
npm install @tabnas/markdown @tabnas/parser @tabnas/jsonic

# Go
go get github.com/tabnas/markdown/go@latest
```

## One tiny example

**TypeScript**

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('name,age\nAlice,30\nBob,25') // => [{ name: 'Alice', age: '30' }, { name: 'Bob', age: '25' }]
```

**Go**

```go
j := tabnasjsonic.Make()
j.UseDefaults(tabnasmarkdown.Markdown, tabnasmarkdown.Defaults)

result, _ := j.Parse("name,age\nAlice,30\nBob,25")
fmt.Println(result)
// [{[name age] map[age:30 name:Alice]} {[name age] map[age:25 name:Bob]}]
```

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework — one file
per quadrant, per language.

| | TypeScript | Go |
|---|---|---|
| Tutorial (learn) | [ts/doc/tutorial.md](ts/doc/tutorial.md) | [go/doc/tutorial.md](go/doc/tutorial.md) |
| How-to (recipes) | [ts/doc/guide.md](ts/doc/guide.md) | [go/doc/guide.md](go/doc/guide.md) |
| Reference (API + options + grammar) | [ts/doc/reference.md](ts/doc/reference.md) | [go/doc/reference.md](go/doc/reference.md) |
| Concepts (how it works) | [ts/doc/concepts.md](ts/doc/concepts.md) | [go/doc/concepts.md](go/doc/concepts.md) |

Per-language hubs: [ts/README.md](ts/README.md) · [go/README.md](go/README.md).

## Grammar diagram

The live grammar as a railroad/syntax diagram, generated from the embedded
grammar with [`@tabnas/railroad`](https://github.com/tabnas/railroad):

![markdown grammar railroad diagram](ts/doc/grammar.svg)

A vertical ASCII version is in [`ts/doc/grammar.txt`](ts/doc/grammar.txt). The
grammar source is the top-level
[`markdown-grammar.jsonic`](markdown-grammar.jsonic), embedded into both
implementations by [`ts/embed-grammar.js`](ts/embed-grammar.js) during
`npm run build` — edit the grammar file, then re-embed; never edit the embedded
copies directly.

## Repository layout

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript implementation (canonical). |
| [`go/`](go/) | Go port. |
| [`test/fixtures/`](test/fixtures/) | Shared conformance fixtures, run by both runtimes. |

## License

MIT. Copyright (c) Richard Rodger and other contributors.
