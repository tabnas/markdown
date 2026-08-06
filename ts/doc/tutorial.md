# Tutorial: parsing Markdown with @tabnas/markdown (TypeScript)

This is a guided first run. By the end you will have installed the plugin,
parsed Markdown, and understood the AST. Takes about five minutes and assumes
Node.js (>=24).

## 1. Install

Install the engine and the plugin:

```bash
npm install @tabnas/markdown @tabnas/parser
```

## 2. Parse your first document

Create a Tabnas instance and load the Markdown plugin:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# Hello\n') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
```

What happened: `# Hello` became a `heading` node with `depth: 1` and a `text` child.

## 3. Read the result

The return value is always a `document` node:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# Title\n\nHello *world*') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Title' }] }, { type: 'paragraph', children: [{ type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [{ type: 'text', value: 'world' }] }] }] }
const doc = j.parse('# Title\n\nHello *world*')
doc.type // => 'document'
doc.children[0].type // => 'heading'
doc.children[1].type // => 'paragraph'
```

## 4. Inline markup

Paragraphs and headings contain inline children:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('Hello **world**') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'text', value: 'Hello ' }, { type: 'strong', children: [{ type: 'text', value: 'world' }] }] }] }
j.parse('[link](https://example.com)') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'link', url: 'https://example.com', title: null, children: [{ type: 'text', value: 'link' }] }] }] }
```

## 5. Lists and code

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('- a\n- b\n- c') // => { type: 'document', children: [{ type: 'list', ordered: false, start: null, spread: false, children: [{ type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'a' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'b' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'c' }] }] }] }] }
j.parse('```js\nconsole.log("hi")\n```') // => { type: 'document', children: [{ type: 'code', lang: 'js', meta: null, value: 'console.log("hi")' }] }
```

Next: the [how-to guide](guide.md) for task recipes, or the [reference](reference.md) for the full AST shape.
