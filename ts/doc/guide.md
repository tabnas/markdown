# How-to guide: @tabnas/markdown (TypeScript)

Task-oriented recipes. Each one is self-contained — copy it, change the input,
run it. For the full option list see the [reference](reference.md); for *why*
the plugin behaves this way see the [concepts](concepts.md).

Every recipe starts from the same three lines:

```js ignore
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'
```


## Use the plugin

Load `jsonic` first (the base grammar), then `Markdown` on top. The order
matters — the markdown plugin overrides the entry rule and rewrites the
record/field rules of the jsonic grammar.

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('a,b\n1,2\n3,4') // => [{ a: '1', b: '2' }, { a: '3', b: '4' }]
```

A configured instance is reusable — call `.parse()` as many times as you like.


## Return arrays instead of objects

Set `object: false` to get one array per row, and `header: false` so the first
row is data rather than keys:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { header: false, object: false })

j.parse('a,b,c\n1,2,3') // => [['a', 'b', 'c'], ['1', '2', '3']]
```


## Name fields when there is no header row

With `header: false` and `field.names`, you get objects keyed by your names
instead of by a header row:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, {
  header: false,
  field: { names: ['x', 'y', 'z'] },
})

j.parse('1,2,3\n4,5,6') // => [{ x: '1', y: '2', z: '3' }, { x: '4', y: '5', z: '6' }]
```

Extra fields beyond your names get auto-generated keys (`field~3`, …); see
[handle ragged rows](#handle-ragged-rows-extra-fields).


## Use a custom field delimiter

Set `field.separation`. It can be a single character (tab, pipe) or a
multi-character string:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { field: { separation: '\t' } })

j.parse('name\tage\nAlice\t30') // => [{ name: 'Alice', age: '30' }]
```

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { field: { separation: '~~' } })

j.parse('a~~b~~c\nA~~B~~C') // => [{ a: 'A', b: 'B', c: 'C' }]
```


## Use a custom record separator

By default a record ends at a newline. Set `record.separators` to split rows on
a different character instead:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { record: { separators: '%' } })

j.parse('a,b%1,2%3,4') // => [{ a: '1', b: '2' }, { a: '3', b: '4' }]
```


## Parse numbers and keywords

In strict mode every field is a string. Enable `number` for numeric literals
and `value` for the `true` / `false` / `null` keywords:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { number: true, value: true })

j.parse('a,b,c\n1,true,null') // => [{ a: 1, b: true, c: null }]
```


## Trim whitespace from fields

Whitespace inside a field is significant by default. Enable `trim` to strip
leading and trailing whitespace (internal spacing is preserved):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { trim: true })

j.parse('a , b \n 1 , 2 ') // => [{ a: '1', b: '2' }]
```


## Skip comment lines

Enable `comment` to ignore lines that begin with `#` (and to strip trailing
`#...` comments from a field):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { comment: true })

j.parse('a,b\n# skip this line\n1,2') // => [{ a: '1', b: '2' }]
```


## Handle quoted fields

Double-quoted fields may span commas, line breaks, and escaped quotes. A
literal quote is doubled (`""`):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('a\n"b""c""d"') // => [{ a: 'b"c"d' }]
```

To use a different quote character, set `string.quote`:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, {
  header: false,
  object: false,
  string: { quote: "'" },
})

j.parse("'a''b'") // => [["a'b"]]
```


## Handle ragged rows (extra fields)

If a row has more fields than the header, the extras are kept under generated
keys formed from `field.nonameprefix` (default `field~`) plus the index:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('a,b\n1,2,3') // => [{ a: '1', b: '2', 'field~2': '3' }]
```

A missing field is filled with `field.empty` (default `''`):

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown)

j.parse('a\n1,') // => [{ a: '1', 'field~1': '' }]
```


## Reject ragged rows

Set `field.exact` to require every row to have exactly as many fields as the
header. A mismatch throws; the thrown error's `code` tells you which way:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { field: { exact: true } })

const code = (fn) => { try { fn(); return null } catch (e) { return e.code } }

code(() => j.parse('a,b\n1,2,3')) // => 'markdown_extra_field'
code(() => j.parse('a,b\n1'))     // => 'markdown_missing_field'
```


## Keep blank lines as empty records

Blank lines are skipped by default. Set `record.empty` to emit them as records
whose fields are all empty:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { record: { empty: true } })

j.parse('a\n1\n\n2') // => [{ a: '1' }, { a: '' }, { a: '2' }]
```


## Stream records instead of collecting them

Pass a `stream` callback to receive each record as it is parsed. The callback
sees `'start'`, then a `'record'` event per row, then `'end'` (and `'error'`
on failure). When streaming, `parse()` returns an empty array — nothing is kept
in memory:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const records = []

const j = new Tabnas().use(jsonic).use(Markdown, {
  stream: (what, record) => {
    if (what === 'record') records.push(record)
  },
})

j.parse('a,b\n1,2\n3,4') // => []

records // => [{ a: '1', b: '2' }, { a: '3', b: '4' }]
```


## Embed JSON in a field (non-strict mode)

Set `strict: false` to let a field hold jsonic content — JSON objects, arrays,
and quoted/escaped strings. Non-strict mode also turns `trim`, `comment`, and
`number` on by default:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { strict: false })

j.parse('a,b\ntrue,[1,2]') // => [{ a: true, b: [1, 2] }]
```

A field that starts as a container but has trailing junk is a syntax error:

```js
import { Tabnas } from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(jsonic).use(Markdown, { strict: false })

const code = (fn) => { try { fn(); return null } catch (e) { return e.code } }

code(() => j.parse('a\n{x:1}y')) // => 'unexpected'
```
