/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

// HTML renderer for the finished CommonMark tree.
//
// The conformance suite compares this output to the spec's expected HTML
// byte for byte, so newline placement is a correctness contract here, not a
// formatting preference. The discipline that makes it come out right is the
// reference renderer's:
//
//   * every block writes `cr()` before its opening tag and after its closing
//     tag, and `cr()` is a no-op when the buffer is already at the start of a
//     line;
//   * no block ever emits a newline on behalf of a neighbour or a child.
//
// Because a block only ever asks for "be at line start", nested blocks
// compose without doubling up: `<li>` deliberately does *not* end a line, and
// the newline in `<li>\n<p>…` is the paragraph's own leading `cr()`. A tight
// item, whose paragraph is skipped entirely, therefore renders as
// `<li>text</li>` with no interior newlines at all.
//
// The renderer reads only the finished tree — never `stringContent`, which
// the inline phase has already consumed.

import type { MdNode } from './node.ts'
import { escapeXml, normalizeURI } from './common.ts'
import { resolveOptions } from './options.ts'
import type { ParserOptions } from './options.ts'

/** An HTML attribute as [name, already-escaped value]. */
type Attr = [string, string]

/**
 * Render `doc` (a tree that has been through both parse phases) as HTML.
 *
 * Only `options.breaks` affects the output: it turns soft line breaks into
 * hard ones. `gfm` is a parse-time concern — by the time a `del` node exists
 * the renderer just prints it.
 */
export function renderHTML(doc: MdNode, options?: Partial<ParserOptions>): string {
  const opts = resolveOptions(options)

  // §6.9 soft line breaks: rendered as a newline, or as a hard break when the
  // caller asks for `breaks`.
  const softbreak = opts.breaks ? '<br />\n' : '\n'

  let buf = ''
  // Tracks whether the buffer is positioned at the start of a line, which is
  // the only thing `cr()` needs to know. An empty buffer counts as at-line-
  // start, so a leading `cr()` (every top-level block opens with one) writes
  // nothing.
  let atLineStart = true

  function lit(s: string): void {
    // An empty write must not claim the line is dirty, or the next `cr()`
    // would insert a spurious newline.
    if ('' === s) return
    buf += s
    atLineStart = '\n' === s.charAt(s.length - 1)
  }

  function cr(): void {
    if (!atLineStart) lit('\n')
  }

  function tag(name: string, attrs?: Attr[], selfClosing?: boolean): void {
    let s = '<' + name
    if (undefined !== attrs) {
      for (const attr of attrs) {
        s += ' ' + attr[0] + '="' + attr[1] + '"'
      }
    }
    if (true === selfClosing) s += ' /'
    lit(s + '>')
  }

  const walker = doc.walker()
  let event = walker.next()

  while (null !== event) {
    const node = event.node
    const entering = event.entering

    switch (node.type) {
      case 'document':
        break

      case 'paragraph': {
        // §5.3: "if a list is tight, we remove the <p> tags from the item
        // contents". The paragraph's grandparent is the list — a paragraph
        // whose grandparent is a list is necessarily an item's direct child.
        const grandparent = null === node.parent ? null : node.parent.parent
        if (
          null !== grandparent &&
          'list' === grandparent.type &&
          null !== grandparent.listData &&
          grandparent.listData.tight
        ) {
          break
        }
        if (entering) {
          cr()
          tag('p')
        } else {
          tag('/p')
          cr()
        }
        break
      }

      case 'heading': {
        const name = 'h' + node.level
        if (entering) {
          cr()
          tag(name)
        } else {
          tag('/' + name)
          cr()
        }
        break
      }

      case 'thematic_break':
        cr()
        tag('hr', undefined, true)
        cr()
        break

      case 'block_quote':
        cr()
        tag(entering ? 'blockquote' : '/blockquote')
        cr()
        break

      case 'list': {
        const data = node.listData
        const ordered = null !== data && 'ordered' === data.type
        const name = ordered ? 'ol' : 'ul'
        if (entering) {
          const attrs: Attr[] = []
          // §5.3: the start number is only rendered when it is not 1 — and
          // `start="0"` is meaningful, so this is an inequality, not a
          // truthiness test.
          if (ordered && null !== data && 1 !== data.start) {
            attrs.push(['start', String(data.start)])
          }
          cr()
          tag(name, attrs)
        } else {
          cr()
          tag('/' + name)
        }
        cr()
        break
      }

      case 'item':
        // No `cr()` after `<li>`: a tight item renders `<li>text</li>`, and a
        // loose one gets its newline from the leading `cr()` of the first
        // child block.
        if (entering) {
          tag('li')
        } else {
          tag('/li')
          cr()
        }
        break

      case 'code_block': {
        const attrs: Attr[] = []
        // §4.5: "the first word of the info string is typically used to
        // specify the language". The info string arrives from the block phase
        // already backslash-unescaped and entity-decoded; only the escaping
        // for attribute context is left to do here.
        const info = null === node.info ? '' : node.info
        const firstWord = '' === info ? '' : info.split(/\s+/)[0]
        if ('' !== firstWord) {
          attrs.push(['class', 'language-' + escapeXml(firstWord)])
        }
        cr()
        tag('pre')
        tag('code', attrs)
        // The block phase guarantees the literal already ends with a newline
        // (or is empty, for an empty fenced block, which renders as
        // `<pre><code></code></pre>`).
        lit(escapeXml(null === node.literal ? '' : node.literal))
        tag('/code')
        tag('/pre')
        cr()
        break
      }

      case 'html_block':
        // §4.6: raw HTML passes through verbatim, unescaped.
        cr()
        lit(null === node.literal ? '' : node.literal)
        cr()
        break

      case 'text':
        lit(escapeXml(null === node.literal ? '' : node.literal))
        break

      case 'softbreak':
        lit(softbreak)
        break

      case 'linebreak':
        tag('br', undefined, true)
        cr()
        break

      case 'code':
        tag('code')
        lit(escapeXml(null === node.literal ? '' : node.literal))
        tag('/code')
        break

      case 'html_inline':
        lit(null === node.literal ? '' : node.literal)
        break

      case 'emph':
        tag(entering ? 'em' : '/em')
        break

      case 'strong':
        tag(entering ? 'strong' : '/strong')
        break

      case 'del':
        tag(entering ? 'del' : '/del')
        break

      case 'link':
        if (entering) {
          const attrs: Attr[] = [['href', escapeXml(normalizeURI(destinationOf(node)))]]
          // An empty title is dropped: `[link](/url "")` renders as a bare
          // `<a href="/url">`.
          if (null !== node.title && '' !== node.title) {
            attrs.push(['title', escapeXml(node.title)])
          }
          tag('a', attrs)
        } else {
          tag('/a')
        }
        break

      case 'image': {
        if (entering) {
          const attrs: Attr[] = [
            ['src', escapeXml(normalizeURI(destinationOf(node)))],
            ['alt', escapeXml(altText(node))],
          ]
          if (null !== node.title && '' !== node.title) {
            attrs.push(['title', escapeXml(node.title)])
          }
          tag('img', attrs, true)
          // The children have been consumed into `alt`; jump the walk
          // straight to this image's exit event so they are not also
          // rendered as markup.
          walker.resumeAt(node, false)
        }
        break
      }
    }

    event = walker.next()
  }

  return buf
}

function destinationOf(node: MdNode): string {
  return null === node.destination ? '' : node.destination
}

/**
 * §6.4 images: "the image description gets represented as the `alt`
 * attribute" — as plain text. Descend the subtree keeping the text and
 * code-span literals and dropping the markup wrappers, so
 * `![foo *bar*](/url)` yields `alt="foo bar"` and a nested link contributes
 * only its text.
 *
 * Raw inline HTML and line breaks inside a description are not exercised by
 * the spec suite and the two reference implementations disagree: cmark
 * escapes the raw HTML into the attribute and turns breaks into spaces,
 * commonmark.js emits the HTML verbatim (producing a malformed attribute)
 * and turns breaks into newlines. We keep the literal — escaped, since the
 * result is attribute-context text — and follow commonmark.js on breaks.
 */
function altText(image: MdNode): string {
  let alt = ''

  const walker = image.walker()
  walker.next() // discard the image's own entering event

  let event = walker.next()
  while (null !== event && event.node !== image) {
    const node = event.node
    if (event.entering) {
      switch (node.type) {
        case 'text':
        case 'code':
        case 'html_inline':
          alt += null === node.literal ? '' : node.literal
          break
        case 'softbreak':
        case 'linebreak':
          alt += '\n'
          break
        default:
          // Containers (emph, strong, link, a nested image) contribute
          // nothing themselves; the walk still descends into their children.
          break
      }
    }
    event = walker.next()
  }

  return alt
}
