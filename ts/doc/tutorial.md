# Tutorial: parsing records with @tabnas/markdown (TypeScript)

This is a guided first run. By the end you will have installed the plugin,
parsed a small record file, and understood the shape of the result. It takes
about five minutes and assumes only that you have Node.js (>=24) and a project
to work in.

> The package name says *markdown*, but the plugin parses **delimited
> records** — comma-separated fields, one record per line, with a header row.
> Think of it as a configurable CSV/TSV reader built on the tabnas engine.


## 1. Install

The plugin layers on the tabnas parsing engine (`@tabnas/parser`) and the
relaxed-JSON base grammar (`@tabnas/jsonic`). Install all three:

```bash
npm install @tabnas/markdown @tabnas/parser @tabnas/jsonic
```

`@tabnas/parser` (>=2) and `@tabnas/jsonic` are peer dependencies — the plugin
runs *inside* a tabnas instance that already has the jsonic grammar loaded.


## 2. Parse your first record file

Create a tabnas instance, load the jsonic base grammar, then load the markdown
plugin on top. Call `.parse()` with some text:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('name,age\nAlice,30\nBob,25') // => [{ name: 'Alice', age: '30' }, { name: 'Bob', age: '25' }]
```

What happened:

- The **first line** (`name,age`) is the header. Its fields become the keys.
- Each following line is one **record**, split on commas into **fields**.
- You get back an **array of objects**, one object per data row.

Note that every value is a **string** (`'30'`, not `30`). In the default
*strict* mode the plugin does not interpret field contents — a record file is
text, and `30` is the text `"30"`. You will turn that off in a moment.


## 3. Read the result

The return value is a plain JavaScript array. Use it like any other:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

const rows = j.parse('name,age\nAlice,30\nBob,25')

rows.length      // => 2
rows[0].name     // => 'Alice'
rows[1].age      // => '25'
```


## 4. Get numbers instead of strings

A header file is more useful when numbers are numbers. Turn on the `number`
option (and `value` for `true`/`false`/`null` keywords):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { number: true, value: true })

j.parse('a,b,c\n1,true,null') // => [{ a: 1, b: true, c: null }]
```

Now `1` is the number `1`, `true` is the boolean, and `null` is the null value.


## 5. Quoted fields

A field can be wrapped in double quotes so it may contain commas, line breaks,
or quotes. A literal quote inside a quoted field is written as `""` (the RFC
4180 convention):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('a\n"b""c"') // => [{ a: 'b"c' }]
```

The two quotes in `"b""c"` collapse to one, so the field value is `b"c`.


## You have arrived

You can now:

- install the plugin and load it onto a tabnas instance,
- parse a header-and-records file into an array of objects,
- get numbers and booleans with `number` / `value`,
- handle quoted fields.

Where to go next:

- **[How-to guide](guide.md)** — task recipes: custom delimiters, no-header
  mode, streaming, error handling, embedded JSON.
- **[Reference](reference.md)** — the full option list with defaults and the
  exact grammar accepted.
- **[Concepts](concepts.md)** — how the plugin works on the engine, and why
  strict mode is the default.
