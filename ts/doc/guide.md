# How-to guide: @tabnas/markdown (TypeScript)

Task-oriented recipes. Each one is self-contained.

Every recipe starts from:

```js ignore
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'
```

## Use the plugin

Load `Markdown` on a bare engine. Order matters only if you also use another plugin — Markdown must be last to own the `markdown` start rule.

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# Hello') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }] }
```

A configured instance is reusable — call `.parse()` many times.

## Parse headings

ATX (`#`…`######`) and Setext (`===`/`---` underline):

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('# H1') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'H1' }] }] }
j.parse('## H2') // => { type: 'document', children: [{ type: 'heading', depth: 2, children: [{ type: 'text', value: 'H2' }] }] }
j.parse('Foo\n===') // => { type: 'document', children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Foo' }] }] }
```

## Parse lists

Unordered (`-` `*` `+`) and ordered (`1.` `1)`). Start number is preserved.

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('- a\n- b') // => { type: 'document', children: [{ type: 'list', ordered: false, start: null, spread: false, children: [{ type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'a' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'b' }] }] }] }] }
j.parse('1. a\n2. b\n3. c') // => { type: 'document', children: [{ type: 'list', ordered: true, start: 1, spread: false, children: [{ type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'a' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'b' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'c' }] }] }] }] }
j.parse('5. a\n6. b') // => { type: 'document', children: [{ type: 'list', ordered: true, start: 5, spread: false, children: [{ type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'a' }] }] }, { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'b' }] }] }] }] }
```

## Parse code blocks

Fenced (```` ``` ```` / `~~~`) and indented (4 spaces):

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('```js\nconsole.log("hi")\n```') // => { type: 'document', children: [{ type: 'code', lang: 'js', meta: null, value: 'console.log("hi")' }] }
j.parse('    indented\n    code') // => { type: 'document', children: [{ type: 'code', lang: null, meta: null, value: 'indented\ncode' }] }
```

## Parse blockquotes

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('> hello\n> world') // => { type: 'document', children: [{ type: 'blockquote', children: [{ type: 'paragraph', children: [{ type: 'text', value: 'hello world' }] }] }] }
```

## Parse inline markup

Emphasis, strong, code spans, links, images, autolinks, strikethrough:

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown)

j.parse('Hello *world*') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [{ type: 'text', value: 'world' }] }] }] }
j.parse('Hello **world**') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'text', value: 'Hello ' }, { type: 'strong', children: [{ type: 'text', value: 'world' }] }] }] }
j.parse('`code`') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'inlineCode', value: 'code' }] }] }
j.parse('[link](https://example.com)') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'link', url: 'https://example.com', title: null, children: [{ type: 'text', value: 'link' }] }] }] }
j.parse('![alt](https://example.com/img.png)') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'image', url: 'https://example.com/img.png', title: null, alt: 'alt' }] }] }
```

## Disable GFM strikethrough

Strikethrough (`~~text~~`) is on when `gfm:true` (default):

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const j = new Tabnas().use(Markdown, { gfm: false })

j.parse('~~delete~~') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'text', value: '~~delete~~' }] }] }
```

```js
import { Tabnas } from '@tabnas/parser'
import { Markdown } from '@tabnas/markdown'

const gfm = new Tabnas().use(Markdown, { gfm: true })

gfm.parse('~~delete~~') // => { type: 'document', children: [{ type: 'paragraph', children: [{ type: 'delete', children: [{ type: 'text', value: 'delete' }] }] }] }
```
