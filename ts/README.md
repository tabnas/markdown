# @jsonic/markdown

A [Jsonic](https://jsonic.senecajs.org) syntax plugin that parses
Markdown text into objects or arrays, with support for headers, quoted
fields, custom delimiters, streaming, and strict/non-strict modes.
Available for TypeScript and Go.


[![npm version](https://img.shields.io/npm/v/@jsonic/markdown.svg)](https://npmjs.com/package/@jsonic/markdown)
[![build](https://github.com/jsonicjs/markdown/actions/workflows/build.yml/badge.svg)](https://github.com/jsonicjs/markdown/actions/workflows/build.yml)
[![Coverage Status](https://coveralls.io/repos/github/jsonicjs/markdown/badge.svg?branch=main)](https://coveralls.io/github/jsonicjs/markdown?branch=main)
[![Known Vulnerabilities](https://snyk.io/test/github/jsonicjs/markdown/badge.svg)](https://snyk.io/test/github/jsonicjs/markdown)
[![DeepScan grade](https://deepscan.io/api/teams/5016/projects/22466/branches/663906/badge/grade.svg)](https://deepscan.io/dashboard#view=project&tid=5016&pid=22466&bid=663906)
[![Maintainability](https://api.codeclimate.com/v1/badges/10e9bede600896c77ce8/maintainability)](https://codeclimate.com/github/jsonicjs/markdown/maintainability)

| ![Voxgig](https://www.voxgig.com/res/img/vgt01r.png) | This open source module is sponsored and supported by [Voxgig](https://www.voxgig.com). |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------- |


## Quick example

**TypeScript**

```typescript
import { Jsonic } from 'jsonic'
import { Markdown } from '@jsonic/markdown'

const parse = Jsonic.make().use(Markdown)

parse("name,age\nAlice,30\nBob,25")
// [{ name: 'Alice', age: '30' }, { name: 'Bob', age: '25' }]

parse('a,b\n1,"hello, world"')
// [{ a: '1', b: 'hello, world' }]
```

**Go**

```go
import markdown "github.com/jsonicjs/markdown/go"

result, _ := markdown.Parse("name,age\nAlice,30\nBob,25")
// [{name:Alice age:30} {name:Bob age:25}]
```


## Documentation

Full documentation following the [Diataxis](https://diataxis.fr)
framework (tutorials, how-to guides, explanation, reference):

- [TypeScript documentation](doc/markdown-ts.md)
- [Go documentation](doc/markdown-go.md)


## License

Copyright (c) 2021-2025 Richard Rodger and other contributors,
[MIT License](LICENSE).
