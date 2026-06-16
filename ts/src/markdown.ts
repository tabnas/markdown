/* Copyright (c) 2021-2025 Richard Rodger, MIT License */

// The engine is the tabnas parser; jsonic supplies the relaxed-JSON
// grammar that the embedded grammar text is authored in.
import {
  Tabnas,
  Rule,
  RuleSpec,
  Plugin,
  Context,
  Config,
  TabnasOptions,
  Lex,
  Token,
} from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

// See defaults below for commentary.
type MarkdownOptions = {
  trim: boolean | null
  comment: boolean | null
  number: boolean | null
  value: boolean | null
  header: boolean
  object: boolean
  stream: null | ((what: string, record?: Record<string, any> | Error) => void)
  strict: boolean
  field: {
    separation: null | string
    nonameprefix: string
    empty: any
    names: undefined | string[]
    exact: boolean
  }
  record: {
    separators: null | string
    empty: boolean
  }
  string: {
    quote: string
    markdown: null | boolean
  }
}

// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
const grammarText = `
# Markdown Grammar Definition
# Parsed by a standard Jsonic instance and passed to jsonic.grammar()
# Function references (@ prefixed) are resolved against the refs map
#
# Token naming:
#   #LN - line ending (removed from per-instance IGNORE set)
#   #SP - whitespace  (removed from per-instance IGNORE set in strict mode)
#   #CA - comma / field separator
#   #ZZ - end of input
#   #VAL - token set: text, string, number, value literals
#
# Rules markdown, newline, record, text are fully defined here.
# Rules list, elem, val are modified in code (strict mode defines from scratch;
# non-strict prepends to existing defaults to preserve JSON parsing).

{
  rule: markdown: open: [
    { s: '#ZZ' }
    { s: '#LN' p: newline c: '@not-record-empty' }
    { p: record }
  ]

  rule: newline: open: [
    { s: '#LN #LN' r: newline }
    { s: '#LN' r: newline }
    { s: '#ZZ' }
    { r: record }
  ]
  rule: newline: close: [
    { s: '#LN #LN' r: newline }
    { s: '#LN' r: newline }
    { s: '#ZZ' }
    { r: record }
  ]

  rule: record: open: [
    { p: list }
  ]
  rule: record: close: [
    { s: '#ZZ' }
    { s: '#LN #ZZ' b: 1 }
    { s: '#LN' r: '@record-close-next' }
  ]

  rule: text: open: [
    { s: ['#VAL' '#SP'] b: 1 r: text n: { text: 1 } g: 'markdown,space,follows' a: '@text-follows' }
    { s: ['#SP' '#VAL'] r: text n: { text: 1 } g: 'markdown,space,leads' a: '@text-leads' }
    { s: ['#SP' '#CA #LN #ZZ'] b: 1 n: { text: 1 } g: 'markdown,end' a: '@text-end' }
    { s: '#SP' n: { text: 1 } g: 'markdown,space' a: '@text-space' p: '@text-space-push' }
    {}
  ]
}
`
// --- END EMBEDDED markdown-grammar.jsonic ---

// Plugin implementation.
const Markdown: Plugin = (tn: Tabnas, options: MarkdownOptions) => {
  // Human descriptions for the tokens, surfaced in railroad diagram legends
  // (read off the live config by @tabnas/railroad). Newline and whitespace are
  // structurally significant (record separator / field text); the JSON
  // brace/bracket tokens only appear inside embedded jsonic field values.
  tn.options({
    config: {
      modify: {
        'markdown-tokendesc': (cfg: any) => {
          cfg.tokenDesc = Object.assign(cfg.tokenDesc || {}, {
            '#LN': 'newline (ends a record / row)',
            '#SP': 'whitespace (significant: field text)',
            '#OB': 'open brace { (embedded JSON map)',
            '#CB': 'close brace } (embedded JSON map)',
            '#OS': 'open bracket [ (embedded JSON list)',
            '#CS': 'close bracket ] (embedded JSON list)',
            '#CL': 'colon : (key:value in embedded JSON)',
            'KEY': 'embedded JSON map key',
            'VAL': 'field value: text, number, string, or keyword',
          })
        },
      },
    },
  })

  // Normalize boolean options.
  const strict = !!options.strict
  const objres = !!options.object
  const header = !!options.header

  // These may be changed below by superior options.
  let trim = !!options.trim
  let comment = !!options.comment
  let opt_number = !!options.number
  let opt_value = !!options.value
  let record_empty = !!options.record?.empty

  const stream = options.stream

  // In strict mode, Jsonic field content is not parsed.
  if (strict) {
    if (false !== options.string.markdown) {
      tn.options({
        lex: {
          match: {
            stringmarkdown: { order: 1e5, make: buildMarkdownStringMatcher(options) },
          },
        },
      })
    }
    tn.options({
      rule: { exclude: 'jsonic,imp' },
    })
  }

  // Fields may contain Jsonic content.
  else {
    if (true === options.string.markdown) {
      tn.options({
        lex: {
          match: {
            stringmarkdown: { order: 1e5, make: buildMarkdownStringMatcher(options) },
          },
        },
      })
    }
    trim = null === options.trim ? true : trim
    comment = null === options.comment ? true : comment
    opt_number = null === options.number ? true : opt_number
    opt_value = null === options.value ? true : opt_value
    tn.options({
      rule: { exclude: 'imp' },
    })
  }

  // Stream rows as they are parsed, do not store in result.
  if (stream) {
    let parser = tn.internal().parser
    let origStart = parser.start.bind(parser) as (...args: any[]) => any
    parser.start = ((...args: any[]) => {
      try {
        return origStart(...args)
      } catch (e: any) {
        stream('error', e)
      }
    }) as typeof parser.start
  }

  let token: Record<string, any> = {}
  if (strict) {
    // Disable JSON structure tokens
    token = {
      '#OB': null,
      '#CB': null,
      '#OS': null,
      '#CS': null,
      '#CL': null,
    }
  }

  // Custom "comma"
  if (options.field.separation) {
    token['#CA'] = options.field.separation
  }

  // Jsonic option overrides.
  let jsonicOptions: any = {
    rule: {
      start: 'markdown',
    },
    fixed: {
      token,
    },
    tokenSet: {
      IGNORE: [
        strict ? null : undefined, // Handle #SP space
        null, // Handle #LN newlines
        undefined, // Still ignore #CM comments
      ],
    },
    number: {
      lex: opt_number,
    },
    value: {
      lex: opt_value,
    },
    comment: {
      lex: comment,
    },
    lex: {
      emptyResult: [],
    },
    line: {
      single: record_empty,
      chars:
        null == options.record.separators
          ? undefined
          : options.record.separators,
      rowChars:
        null == options.record.separators
          ? undefined
          : options.record.separators,
    },
    error: {
      markdown_extra_field: 'unexpected extra field value: $fsrc',
      markdown_missing_field: 'missing field',
    },
    hint: {
      markdown_extra_field: `Row $row has too many fields (the first of which is: $fsrc). Only $len
fields per row are expected.`,
      markdown_missing_field: `Row $row has too few fields. $len fields per row are expected.`,
    },
  }

  tn.options(jsonicOptions)


  // Named function references for declarative grammar definition.
  const refs: Record<string, Function> = {

    // === State actions (auto-wired by @rulename-{bo,ao,bc,ac} convention) ===

    '@markdown-bo': (r: Rule, ctx: Context) => {
      ctx.u.recordI = 0
      stream && stream('start')
      r.node = []
    },

    '@markdown-ac': (_r: Rule) => {
      stream && stream('end')
    },

    '@record-bc': (r: Rule, ctx: Context) => {
      let fields: string[] = ctx.u.fields || options.field.names

      if (0 === ctx.u.recordI && header) {
        ctx.u.fields = undefined === r.child.node ? [] : r.child.node
      } else {
        let record: any = r.child.node || []

        if (objres) {
          let obj: Record<string, any> = {}
          let i = 0

          if (fields) {
            if (options.field.exact) {
              if (record.length !== fields.length) {
                return ctx.t0.bad(
                  record.length > fields.length
                    ? 'markdown_extra_field'
                    : 'markdown_missing_field',
                )
              }
            }

            let fI = 0
            for (; fI < fields.length; fI++) {
              obj[fields[fI]] =
                undefined === record[fI] ? options.field.empty : record[fI]
            }
            i = fI
          }

          for (; i < record.length; i++) {
            let field_name = options.field.nonameprefix + i
            obj[field_name] =
              undefined === record[i] ? options.field.empty : record[i]
          }

          record = obj
        } else {
          for (let i = 0; i < record.length; i++) {
            record[i] =
              undefined === record[i] ? options.field.empty : record[i]
          }
        }

        if (stream) {
          stream('record', record)
        } else {
          r.node.push(record)
        }
      }

      ctx.u.recordI++
    },

    '@text-bc': (r: Rule) => {
      r.parent.node = undefined === r.child.node ? r.node : r.child.node
    },


    // === Alt actions ===

    '@elem-open-empty': (r: Rule) => {
      r.node.push(options.field.empty)
      r.u.done = true
    },

    '@elem-close-trailing': (r: Rule) => {
      r.node.push(options.field.empty)
    },

    '@text-follows': (r: Rule) => {
      let v = 1 === r.n.text ? r : r.prev
      r.node = v.node = (1 === r.n.text ? '' : r.prev.node) + r.o0.val
    },

    '@text-leads': (r: Rule) => {
      let v = 1 === r.n.text ? r : r.prev
      r.node = v.node =
        (1 === r.n.text ? '' : r.prev.node) +
        (2 <= r.n.text || !trim ? r.o0.src : '') +
        r.o1.src
    },

    '@text-end': (r: Rule) => {
      let v = 1 === r.n.text ? r : r.prev
      r.node = v.node =
        (1 === r.n.text ? '' : r.prev.node) + (!trim ? r.o0.src : '')
    },

    '@text-space': (r: Rule) => {
      if (strict) {
        let v = 1 === r.n.text ? r : r.prev
        r.node = v.node =
          (1 === r.n.text ? '' : r.prev.node) + (!trim ? r.o0.src : '')
      }
    },


    // === Condition refs ===

    '@not-record-empty': () => !record_empty,


    // === FuncRef for dynamic rule names ===

    '@record-close-next': () => record_empty ? 'record' : 'newline',

    '@text-space-push': () => strict ? '' : 'val',
  }


  // Usually [#TX, #ST, #NR, #VL]
  let VAL = tn.tokenSet.VAL

  let { LN, CA, SP, ZZ } = tn.token

  // Parse embedded grammar definition with a jsonic-grammar Tabnas instance.
  const grammarDef = new Tabnas().use(jsonic).parse(grammarText)
  grammarDef.ref = refs
  tn.grammar(grammarDef)


  // Rules list, elem, val are modified in code rather than the grammar file,
  // because in non-strict mode the default jsonic alternatives must be preserved
  // to support embedded JSON values like [1,2] and {x:1}.

  tn.rule('list', (rs: RuleSpec) => {
    return rs
      .open([
        // If not ignoring empty fields, don't consume LN used to close empty record.
        { s: [LN], b: 1 },
      ])
      // Unconditional fallback to push elem — the default jsonic list rule gates
      // its elem push on prev.u.implist which the record rule does not set.
      .open([{ p: 'elem' }], { append: true })
      .close([
        // LN ends record
        { s: [LN], b: 1 },

        { s: [ZZ] },
      ])
  })

  tn.rule('elem', (rs: RuleSpec) => {
    return rs
      .open(
        [
          // An empty element
          {
            s: [CA],
            b: 1,
            a: (r: Rule) => {
              r.node.push(options.field.empty)
              r.u.done = true
            },
          },
        ],
      )

      .close(
        [
          // An empty element at the end of the line
          {
            s: [CA, [LN, ZZ]],
            b: 1,
            a: (r: Rule) => r.node.push(options.field.empty),
          },

          // LN ends record
          { s: [LN], b: 1 },
        ],
      )
  })

  tn.rule('val', (rs: RuleSpec) => {
    return rs.open(
      [
        // Handle text and space concatentation
        { s: [VAL, SP], b: 2, p: 'text' },
        { s: [SP], b: 1, p: 'text' },

        // LN ends record
        { s: [LN], b: 1 },
      ],
    )
  })

  // Close is called on final rule - set parent val node
  tn.rule('text', (rs: RuleSpec) => {
    rs.bc((r: Rule) => {
      r.parent.node = undefined === r.child.node ? r.node : r.child.node
    })
  })
}

// Custom Markdown String matcher.
// Handles "a""b" -> "a"b" quoting wierdness.
// This is a reduced copy of the standard Jsonic string matcher.
function buildMarkdownStringMatcher(mdopts: MarkdownOptions) {
  return function makeMarkdownStringMatcher(cfg: Config, _opts: TabnasOptions) {
    return function markdownStringMatcher(lex: Lex): Token | undefined {
      let quoteMap: any = { [mdopts.string.quote]: true }

      let { pnt, src } = lex
      let { sI, rI, cI } = pnt
      let srclen = src.length

      if (quoteMap[src[sI]]) {
        const q = src[sI] // Quote character
        const qI = sI
        const qrI = rI
        ++sI
        ++cI

        let s: string[] = []

        for (sI; sI < srclen; sI++) {
          cI++
          let c = src[sI]

          // Quote char.
          if (q === c) {
            sI++
            cI++

            if (q === src[sI]) {
              s.push(q)
            } else {
              break // String finished.
            }
          }

          // Body part of string.
          else {
            let bI = sI

            let qc = q.charCodeAt(0)
            let cc = src.charCodeAt(sI)

            while (sI < srclen && 32 <= cc && qc !== cc) {
              cc = src.charCodeAt(++sI)
              cI++
            }
            cI--

            if (cfg.line.chars[src[sI]]) {
              if (cfg.line.rowChars[src[sI]]) {
                pnt.rI = ++rI
              }

              cI = 1
              s.push(src.substring(bI, sI + 1))
            } else if (cc < 32) {
              pnt.sI = sI
              pnt.cI = cI
              return lex.bad('unprintable', sI, sI + 1)
            } else {
              s.push(src.substring(bI, sI))
              sI--
            }
          }
        }

        if (src[sI - 1] !== q || pnt.sI === sI - 1) {
          pnt.rI = qrI
          return lex.bad('unterminated_string', qI, sI)
        }

        const tkn = lex.token(
          '#ST',
          s.join(''),
          src.substring(pnt.sI, sI),
          pnt,
        )

        pnt.sI = sI
        pnt.rI = rI
        pnt.cI = cI
        return tkn
      }
    }
  }
}

// Default option values.
Markdown.defaults = {
  trim: null,
  comment: null,
  number: null,
  value: null,
  header: true,
  object: true,
  stream: null,
  strict: true,
  field: {
    separation: null,
    nonameprefix: 'field~',
    empty: '',
    names: undefined,
    exact: false,
  },
  record: {
    separators: null,
    empty: false,
  },
  string: {
    quote: '"',
    markdown: null,
  },
} as MarkdownOptions

export { Markdown, buildMarkdownStringMatcher }

export type { MarkdownOptions }
