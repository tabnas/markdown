const { Jsonic } = require('@jsonic/jsonic-next')
const { Debug } = require('@jsonic/jsonic-next/debug')
const { Csv } = require('..')

const tlog = []

// const c0 = Jsonic.make()
//       .use(Debug,{trace:true})
//       .use(Csv,{comment:true,object:false,header:false})

// const u0 = Jsonic.make()
//       // .use(Debug,{trace:true})
//       .use(Csv,{
//         strict:false,
//       })

const csv = Jsonic.make()
  .use(Debug, { trace: true })
  .use(Csv, {
    // line: {empty:true},
    // header: false,
    // object: false,
    // trim: true,
    // value: true,
    // comment: true,
    // record: { empty: true }
  })
  // .sub({lex:(t)=>console.log(t)})
  .sub({ lex: (t) => tlog.push(t) })

// console.log(csv.options.tokenSet)
// console.log(csv.internal().config.lex.match)

// console.log(csv(`a,b
// 1,2,`,{xlog:-1}))

console.log(
  csv(
    `a
,1`,
    { xlog: -1 },
  ),
)

// console.log(csv(`a,b
// 1, 2
// 11 ,{22
// 3 3, "a"
// `,{xlog:-1}))

// console.log(csv(`a,b
// 1,2
// 3,"x""y"
// 4,5
// `,{xlog:-1}))

// console.log(csv(`a,b
// 1, 2 3
// 4,  5  6
// 7,	8		9
// 10, 11 12 13
// `,{xlog:-1}))

// const u0 = Jsonic.make()
//       .use(Debug,{trace:true})
//       .use(Csv, {strict:false})

// console.dir(u0(`a,b
// 1 , 2
// `),{depth:null})

// console.dir(u0(`a,b,c
// true,[1,2],{x:{y:"q\\"w"}}
// null,'Q\\r\\nA',1e2
// `),{depth:null})

// console.log(c0(`a,b,c
// 1 , 2 , 3
//  11 ,  22   , 33
// 4\t,\t5\t,\t6
// \t44\t,\t\t55\t\t\t,\t6\t
// `))

// console.log(c0(`a,b,c,d,e,f
// 1 ,2 , 3 ,4 5 , 6 7,8 9 0
// `))

// console.log(c0(`a,b
// "x"y,z`))

// console.log(u0(`a
//  b `))

// console.log(c0(`
// 1`))

// console.log(c0(''))

// console.log(c0(`#foo
// #bar
// 1,2
// #a
// #b

// 3,4

// #c

// `))

// console.log(csv(`a,b
// A,B
// #X

// AA,BB`))

// console.log(csv(`
// #X
// #XX
// a,b
// #Y
// #YY
// A,B
// #Z
// #ZZ
// `))

// console.log(csv('\n'))

// console.log(csv('a,b\nA,"""B"'))

// console.log(csv('true'))

// console.log(csv('\na\n'))

// console.log(tlog)
