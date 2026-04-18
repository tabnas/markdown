# CSV plugin for Jsonic (TypeScript)

A Jsonic syntax plugin that parses CSV text into JavaScript arrays
of objects or arrays, with support for headers, quoted fields,
custom delimiters, streaming, and strict/non-strict modes.

```bash
npm install @jsonic/csv
```

Requires `jsonic` >= 2 as a peer dependency.


## Tutorials

### Parse a basic CSV file

Parse CSV text with a header row into an array of objects:

```typescript
import { Jsonic } from 'jsonic'
import { Csv } from '@jsonic/csv'

const j = Jsonic.make().use(Csv)

j("name,age\nAlice,30\nBob,25")
// [{ name: 'Alice', age: '30' }, { name: 'Bob', age: '25' }]
```

### Parse CSV without headers

Return rows as arrays instead of objects, with no header row:

```typescript
import { Jsonic } from 'jsonic'
import { Csv } from '@jsonic/csv'

const j = Jsonic.make().use(Csv, { header: false, object: false })

j("a,b,c\n1,2,3")
// [['a', 'b', 'c'], ['1', '2', '3']]
```

### Parse CSV with quoted fields

Double-quoted fields handle commas, newlines, and escaped quotes:

```typescript
import { Jsonic } from 'jsonic'
import { Csv } from '@jsonic/csv'

const j = Jsonic.make().use(Csv)

j('name,bio\nAlice,"Likes ""cats"" and dogs"\nBob,"Line1\nLine2"')
// [
//   { name: 'Alice', bio: 'Likes "cats" and dogs' },
//   { name: 'Bob', bio: 'Line1\nLine2' }
// ]
```


## How-to guides

### Use a custom field delimiter

Set `field.separation` to use a delimiter other than comma:

```typescript
const j = Jsonic.make().use(Csv, {
  field: { separation: '\t' }
})

j("name\tage\nAlice\t30")
// [{ name: 'Alice', age: '30' }]
```

### Enable number and value parsing

By default in strict mode, all values are strings. Enable `number`
and `value` to parse numeric and boolean values:

```typescript
const j = Jsonic.make().use(Csv, {
  number: true,
  value: true,
})

j("a,b,c\n1,true,null")
// [{ a: 1, b: true, c: null }]
```

### Trim whitespace from fields

Enable `trim` to remove leading and trailing whitespace from field
values:

```typescript
const j = Jsonic.make().use(Csv, { trim: true })

j("a , b \n 1 , 2 ")
// [{ a: '1', b: '2' }]
```

### Stream records as they are parsed

Use the `stream` callback to receive records one at a time without
storing them all in memory:

```typescript
const records: any[] = []

const j = Jsonic.make().use(Csv, {
  stream: (what, record) => {
    if (what === 'record') records.push(record)
  },
})

j("a,b\n1,2\n3,4")
// returns [] (empty, records were streamed)
// records === [{ a: '1', b: '2' }, { a: '3', b: '4' }]
```

### Provide explicit field names

Set `field.names` when the CSV has no header row but you want
object output with named fields:

```typescript
const j = Jsonic.make().use(Csv, {
  header: false,
  field: { names: ['x', 'y', 'z'] },
})

j("1,2,3\n4,5,6")
// [{ x: '1', y: '2', z: '3' }, { x: '4', y: '5', z: '6' }]
```

### Enforce exact field counts

Set `field.exact` to error when a row has more or fewer fields
than the header:

```typescript
const j = Jsonic.make().use(Csv, {
  field: { exact: true },
})

// j("a,b\n1,2,3")  // throws: unexpected extra field value
// j("a,b\n1")      // throws: missing field
```

### Use non-strict mode for embedded JSON

Disable `strict` to allow Jsonic syntax inside CSV fields,
including JSON objects, arrays, and expressions:

```typescript
const j = Jsonic.make().use(Csv, { strict: false })

j("a,b\ntrue,[1,2]")
// [{ a: true, b: [1, 2] }]
```

### Enable comment lines

Enable `comment` to skip lines starting with `#`:

```typescript
const j = Jsonic.make().use(Csv, { comment: true })

j("a,b\n# skip this\n1,2")
// [{ a: '1', b: '2' }]
```

### Preserve empty records

By default, blank lines are skipped. Set `record.empty` to
preserve them as empty-field records:

```typescript
const j = Jsonic.make().use(Csv, { record: { empty: true } })

j("a\n1\n\n2")
// [{ a: '1' }, { a: '' }, { a: '2' }]
```


## Explanation

### Strict vs non-strict mode

In **strict mode** (default), the CSV plugin disables Jsonic's
built-in JSON parsing. All field values are treated as raw strings
unless `number` or `value` options are enabled. This matches the
behaviour of standard CSV parsers.

In **non-strict mode** (`strict: false`), the plugin preserves
Jsonic's ability to parse JSON values. Fields can contain objects
(`{x:1}`), arrays (`[1,2]`), booleans, numbers, and quoted strings
using Jsonic syntax. Non-strict mode enables `trim`, `comment`, and
`number` by default.

### How quoted fields work

The plugin includes a custom CSV string matcher that handles the
RFC 4180 double-quote escaping convention:

- A field wrapped in double quotes can contain commas, newlines,
  and quotes.
- A literal quote inside a quoted field is represented as `""`.
- For example: `"a""b"` parses to `a"b`.


## Reference

### `Csv` (Plugin)

The plugin function. Register with `Jsonic.make().use(Csv, options)`.

### `CsvOptions`

```typescript
type CsvOptions = {
  // Trim surrounding whitespace. Default: null (false in strict, true in non-strict)
  trim: boolean | null

  // Enable # line comments. Default: null (false in strict, true in non-strict)
  comment: boolean | null

  // Parse numeric values. Default: null (false in strict, true in non-strict)
  number: boolean | null

  // Parse value keywords (true/false/null). Default: null (false in strict, false in non-strict)
  value: boolean | null

  // First row is a header row. Default: true
  header: boolean

  // Return records as objects (true) or arrays (false). Default: true
  object: boolean

  // Stream callback. Default: null
  stream: null | ((what: string, record?: Record<string, any> | Error) => void)

  // Strict CSV mode (disables Jsonic syntax). Default: true
  strict: boolean

  field: {
    // Field separator string. Default: null (uses comma)
    separation: null | string

    // Prefix for unnamed extra fields. Default: 'field~'
    nonameprefix: string

    // Value for empty fields. Default: ''
    empty: any

    // Explicit field names (overrides header). Default: undefined
    names: undefined | string[]

    // Error on field count mismatch. Default: false
    exact: boolean
  }

  record: {
    // Custom record separator characters. Default: null
    separators: null | string

    // Preserve empty lines as records. Default: false
    empty: boolean
  }

  string: {
    // Quote character. Default: '"'
    quote: string

    // Force CSV string mode (null=auto). Default: null
    csv: null | boolean
  }
}
```

### `buildCsvStringMatcher` (Function)

Exported for advanced use. Creates the custom CSV double-quote
string matcher used internally by the plugin.
