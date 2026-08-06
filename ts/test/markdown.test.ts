/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

import { describe, test } from 'node:test'
import assert from 'node:assert'

import { Tabnas } from '@tabnas/parser'
import { Markdown } from '../dist/markdown'

function md(src: string, opts?: any) {
  return new Tabnas().use(Markdown, opts).parse(src)
}

describe('markdown — prose', () => {
  test('empty and blank', () => {
    assert.deepEqual(md(''), { type: 'document', children: [] })
    assert.deepEqual(md('\n'), { type: 'document', children: [] })
    assert.deepEqual(md('   \n\t\n'), { type: 'document', children: [] })
  })

  test('ATX headings', () => {
    assert.deepEqual(md('# Hello'), {
      type: 'document',
      children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Hello' }] }],
    })
    assert.deepEqual(md('## Heading 2'), {
      type: 'document',
      children: [{ type: 'heading', depth: 2, children: [{ type: 'text', value: 'Heading 2' }] }],
    })
    assert.deepEqual(md('###### H6'), {
      type: 'document',
      children: [{ type: 'heading', depth: 6, children: [{ type: 'text', value: 'H6' }] }],
    })
    // up to 3 leading spaces, trailing hashes stripped
    assert.deepEqual(md('   ### Hello ###  '), {
      type: 'document',
      children: [{ type: 'heading', depth: 3, children: [{ type: 'text', value: 'Hello' }] }],
    })
    // inline inside heading
    assert.deepEqual(md('# Hello *world*'), {
      type: 'document',
      children: [{
        type: 'heading', depth: 1,
        children: [{ type: 'text', value: 'Hello ' }, { type: 'emphasis', children: [{ type: 'text', value: 'world' }] }]
      }],
    })
  })

  test('Setext headings', () => {
    assert.deepEqual(md('Foo\n==='), {
      type: 'document',
      children: [{ type: 'heading', depth: 1, children: [{ type: 'text', value: 'Foo' }] }],
    })
    assert.deepEqual(md('Bar\n---'), {
      type: 'document',
      children: [{ type: 'heading', depth: 2, children: [{ type: 'text', value: 'Bar' }] }],
    })
    assert.deepEqual(md('Foo\n===\n\nBar\n---'), {
      type: 'document',
      children: [
        { type: 'heading', depth: 1, children: [{ type: 'text', value: 'Foo' }] },
        { type: 'heading', depth: 2, children: [{ type: 'text', value: 'Bar' }] },
      ],
    })
  })

  test('thematic breaks', () => {
    assert.deepEqual(md('---'), { type: 'document', children: [{ type: 'thematicBreak' }] })
    assert.deepEqual(md('***'), { type: 'document', children: [{ type: 'thematicBreak' }] })
    assert.deepEqual(md('___'), { type: 'document', children: [{ type: 'thematicBreak' }] })
    assert.deepEqual(md('  - - -  '), { type: 'document', children: [{ type: 'thematicBreak' }] })
    assert.deepEqual(md('* * *'), { type: 'document', children: [{ type: 'thematicBreak' }] })
  })

  test('paragraphs and soft breaks', () => {
    assert.deepEqual(md('Hello world'), {
      type: 'document',
      children: [{ type: 'paragraph', children: [{ type: 'text', value: 'Hello world' }] }],
    })
    assert.deepEqual(md('Hello\n\nWorld'), {
      type: 'document',
      children: [
        { type: 'paragraph', children: [{ type: 'text', value: 'Hello' }] },
        { type: 'paragraph', children: [{ type: 'text', value: 'World' }] },
      ],
    })
    // paragraph with soft line break inside (joined, inline sees newline as space when breaks:false)
    const doc = md('Paragraph line1\nline2 second')
    assert.equal(doc.children[0].type, 'paragraph')
  })

  test('fenced code', () => {
    assert.deepEqual(md('```\ncode here\n```'), {
      type: 'document',
      children: [{ type: 'code', lang: null, meta: null, value: 'code here' }],
    })
    assert.deepEqual(md('```js\nconsole.log("hi")\n```'), {
      type: 'document',
      children: [{ type: 'code', lang: 'js', meta: null, value: 'console.log("hi")' }],
    })
    assert.deepEqual(md('~~~python\nprint("hi")\n~~~'), {
      type: 'document',
      children: [{ type: 'code', lang: 'python', meta: null, value: 'print("hi")' }],
    })
    assert.deepEqual(md('```js meta info\ncode\n```'), {
      type: 'document',
      children: [{ type: 'code', lang: 'js', meta: 'meta info', value: 'code' }],
    })
  })

  test('indented code', () => {
    assert.deepEqual(md('    indented\n    second'), {
      type: 'document',
      children: [{ type: 'code', lang: null, meta: null, value: 'indented\nsecond' }],
    })
  })

  test('blockquotes', () => {
    assert.deepEqual(md('> hello\n> world'), {
      type: 'document',
      children: [{
        type: 'blockquote',
        children: [{ type: 'paragraph', children: [{ type: 'text', value: 'hello world' }] }]
      }],
    })
    assert.deepEqual(md('> hello\n> - item 1\n> - item 2'), {
      type: 'document',
      children: [{
        type: 'blockquote',
        children: [
          { type: 'paragraph', children: [{ type: 'text', value: 'hello' }] },
          {
            type: 'list', ordered: false, start: null, spread: false,
            children: [
              { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'item 1' }] }] },
              { type: 'listItem', spread: false, children: [{ type: 'paragraph', children: [{ type: 'text', value: 'item 2' }] }] },
            ]
          }
        ]
      }],
    })
  })

  test('unordered lists', () => {
    const doc = md('- a\n- b\n- c') as any
    assert.equal(doc.children[0].type, 'list')
    assert.equal(doc.children[0].ordered, false)
    assert.equal(doc.children[0].children.length, 3)
    assert.deepEqual(doc.children[0].children[0], {
      type: 'listItem', spread: false,
      children: [{ type: 'paragraph', children: [{ type: 'text', value: 'a' }] }]
    })
  })

  test('ordered lists', () => {
    const doc = md('1. a\n2. b\n3. c') as any
    assert.equal(doc.children[0].ordered, true)
    assert.equal(doc.children[0].start, 1)
    const doc2 = md('5. a\n6. b') as any
    assert.equal(doc2.children[0].start, 5)
  })

  test('html blocks', () => {
    assert.deepEqual(md('<div>\n<p>hi</p>\n</div>'), {
      type: 'document',
      children: [{ type: 'html', value: '<div>\n<p>hi</p>\n</div>' }],
    })
  })

  test('inline — emphasis, strong, code', () => {
    assert.deepEqual(md('Hello *world*'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [
          { type: 'text', value: 'Hello ' },
          { type: 'emphasis', children: [{ type: 'text', value: 'world' }] },
        ]
      }],
    })
    assert.deepEqual(md('Hello **world**'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [
          { type: 'text', value: 'Hello ' },
          { type: 'strong', children: [{ type: 'text', value: 'world' }] },
        ]
      }],
    })
    assert.deepEqual(md('`code`'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{ type: 'inlineCode', value: 'code' }],
      }],
    })
    // foo_bar_baz inside word should NOT be emphasis
    assert.deepEqual(md('foo_bar_baz'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{ type: 'text', value: 'foo_bar_baz' }],
      }],
    })
  })

  test('inline — links and images', () => {
    assert.deepEqual(md('[link](https://example.com)'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{
          type: 'link', url: 'https://example.com', title: null,
          children: [{ type: 'text', value: 'link' }]
        }],
      }],
    })
    assert.deepEqual(md('[link](https://example.com "title")'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{
          type: 'link', url: 'https://example.com', title: 'title',
          children: [{ type: 'text', value: 'link' }]
        }],
      }],
    })
    assert.deepEqual(md('![alt](https://example.com/img.png)'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{ type: 'image', url: 'https://example.com/img.png', title: null, alt: 'alt' }],
      }],
    })
    assert.deepEqual(md('<https://example.com>'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{
          type: 'link', url: 'https://example.com', title: null,
          children: [{ type: 'text', value: 'https://example.com' }]
        }],
      }],
    })
  })

  test('inline — escapes', () => {
    assert.deepEqual(md('Text with \\*escaped\\* stars'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{ type: 'text', value: 'Text with *escaped* stars' }],
      }],
    })
  })

  test('inline — strikethrough gfm', () => {
    assert.deepEqual(md('~~delete~~'), {
      type: 'document',
      children: [{
        type: 'paragraph',
        children: [{ type: 'delete', children: [{ type: 'text', value: 'delete' }] }],
      }],
    })
    const noGfm = new Tabnas().use(Markdown, { gfm: false }).parse('~~delete~~') as any
    assert.equal(noGfm.children[0].children[0].type, 'text')
    assert.equal(noGfm.children[0].children[0].value, '~~delete~~')
  })

  test('mixed document', () => {
    const src = '# Title\n\nParagraph with *em* and **strong**.\n\n- item 1\n- item 2\n\n> quote\n\n```js\ncode\n```'
    const doc = md(src) as any
    assert.equal(doc.children[0].type, 'heading')
    assert.equal(doc.children[1].type, 'paragraph')
    assert.equal(doc.children[2].type, 'list')
    assert.equal(doc.children[3].type, 'blockquote')
    assert.equal(doc.children[4].type, 'code')
  })

  test('bare Tabnas without jsonic', () => {
    // Prose parser must work on the bare engine per dx-report §2.4
    const j = new Tabnas().use(Markdown)
    const doc = j.parse('# hi')
    assert.equal(doc.children[0].type, 'heading')
  })

  test('also works when jsonic already installed (compat)', async () => {
    const { jsonic } = await import('@tabnas/jsonic')
    const j = new Tabnas().use(jsonic).use(Markdown)
    const doc = j.parse('# hi')
    assert.equal(doc.children[0].type, 'heading')
  })
})
