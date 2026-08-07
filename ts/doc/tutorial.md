# Tutorial: parsing Markdown with @tabnas/markdown (TypeScript)

This is a guided first run. By the end you will have installed the package,
parsed a document into an AST, rendered the same document to HTML, and changed
one option. Takes about five minutes and assumes Node.js (>=24).

Two answers before you start, because they shape everything below:

- **The AST is the primary output.** `parseDocument()` returns it. Nothing else
  runs — the HTML renderer is never touched.
- **Yes, there is an HTML emitter**: `toHtml()`. The CommonMark test suite
  scores HTML output, so the renderer is the instrument that makes this
  package's 652/652 conformance score measurable. You get to use it too.

You will meet both in this lesson, in that order.

## 1. Install

```bash
npm install @tabnas/markdown
```

Make a file called `notes.mjs` next to it. Every code block below goes in that
file, and you run it with `node notes.mjs`.

## 2. Parse a document

Put this in `notes.mjs`:

```js
import { parseDocument } from '@tabnas/markdown'

const src = `# Notes

A *short* note with a [link](https://example.com).

- one
- two
`

const doc = parseDocument(src)

console.log(JSON.stringify(doc, null, 2))
```

Run it:

```bash
node notes.mjs
```

You will see the AST. It is plain JSON — no classes, no cycles, nothing that
needs a special printer:

```json
{
  "type": "document",
  "children": [
    {
      "type": "heading",
      "depth": 1,
      "children": [{ "type": "text", "value": "Notes" }]
    },
    {
      "type": "paragraph",
      "children": [
        { "type": "text", "value": "A " },
        { "type": "emphasis", "children": [{ "type": "text", "value": "short" }] },
        { "type": "text", "value": " note with a " },
        {
          "type": "link",
          "url": "https://example.com",
          "title": null,
          "children": [{ "type": "text", "value": "link" }]
        },
        { "type": "text", "value": "." }
      ]
    },
    {
      "type": "list",
      "ordered": false,
      "start": null,
      "spread": false,
      "children": [
        {
          "type": "listItem",
          "spread": false,
          "checked": null,
          "children": [
            { "type": "paragraph", "children": [{ "type": "text", "value": "one" }] }
          ]
        },
        {
          "type": "listItem",
          "spread": false,
          "checked": null,
          "children": [
            { "type": "paragraph", "children": [{ "type": "text", "value": "two" }] }
          ]
        }
      ]
    }
  ]
}
```

The top of the tree is always a `document` node, and its `children` are the
blocks of the file in source order: the heading, the paragraph, the list.

## 3. Read the AST

Blocks that hold prose — headings, paragraphs, list items — have their own
`children`, holding the inline nodes. Replace the body of `notes.mjs` with:

```js
import { parseDocument } from '@tabnas/markdown'

const doc = parseDocument('# Notes\n\nA *short* note.\n')

doc.type // => 'document'
doc.children.length // => 2
doc.children[0].type // => 'heading'
doc.children[0].depth // => 1
doc.children[1].type // => 'paragraph'
doc.children[1].children[1] // => { type: 'emphasis', children: [{ type: 'text', value: 'short' }] }
```

To see any of these for yourself, wrap the line in `console.log()`. The `// =>`
comments are what the value is; they are checked as tests in this repository,
so they are never out of date.

Notice the shape: `*short*` did not stay as text with asterisks in it. It became
an `emphasis` node with a `text` child. That is the whole point of the AST —
you read structure, not punctuation.

## 4. Render the same document to HTML

The same input, one different function:

```js
import { toHtml } from '@tabnas/markdown'

toHtml('# Notes\n\nA *short* note.\n') // => '<h1>Notes</h1>\n<p>A <em>short</em> note.</p>\n'
```

That string is byte-for-byte what the CommonMark 0.31.2 test suite expects,
newlines included.

One caution, from your very first render: **the HTML is not sanitized.**
CommonMark passes raw HTML through by specification, so `<script>alert(1)</script>`
in the Markdown comes out as `<script>alert(1)</script>` in the HTML. That is
correct behaviour, not a bug. When you get to rendering Markdown that other
people wrote, run a sanitizer over the output; the [how-to guide](guide.md)
has a recipe.

## 5. Change one option: breaks

A single newline inside a paragraph is a *soft* break. By default it reads as a
space in the AST, and as a newline in the HTML:

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('line one\nline two').children[0].children // => [{ type: 'text', value: 'line one line two' }]
toHtml('line one\nline two') // => '<p>line one\nline two</p>\n'
```

Pass `breaks: true` and every soft break becomes a hard one — a `break` node in
the AST, a `<br />` in the HTML:

```js
import { parseDocument, toHtml } from '@tabnas/markdown'

parseDocument('line one\nline two', { breaks: true }).children[0].children // => [{ type: 'text', value: 'line one' }, { type: 'break' }, { type: 'text', value: 'line two' }]
toHtml('line one\nline two', { breaks: true }) // => '<p>line one<br />\nline two</p>\n'
```

Both functions take the same options object, and both were parsed by the same
parser. That is the pattern for everything else in this package.

## 6. Where to go next

You have the two outputs and one option. That is enough to be useful.

- [How-to guide](guide.md) — recipes: walking the AST, rewriting links,
  rendering untrusted input safely, source positions, running the conformance
  suite.
- [Reference](reference.md) — every export, every option, every node type.
- [Concepts](concepts.md) — why the parser is built the way it is.
