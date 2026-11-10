// Copyright (c) 2026 Richard Rodger, MIT License

package tabnasmarkdown

// engineblock.go — the engine-facing block driver: a custom lexer matcher
// that cuts the source one physical line at a time, runs the shared block
// algorithm over each line, and hands the engine the blocks that line opened
// and closed as tokens; and the rule actions that build the AST from those
// tokens. Mirrors ts/src/engine-block.ts.
//
// The grammar's rules open and close every block (markdown.go): a
// container's rule opens on the token that opens it (`#BQ` for a block
// quote, `#LS` for a list, ...) and closes on `#EN`, and a leaf's rule opens
// and closes on its one token (`#HR`, `#CD`, `#DF`, ...). A block's inline
// content is parsed when the block closes, since the block bounds it, and its
// inline nodes are tokens and rules too, so the rule passes carry the whole
// tree, a block at a time, and the value the rules build is that tree.
//
// The block decisions themselves are the shared blockParser's, the same code
// the engine-free path runs (the anti-drift rule, dx-report §42). This driver
// turns what it decided into tokens in the order it decided it
// (blockParser.events), and nothing here recognizes any Markdown of its own.
//
// Each line is one `#LB` token, cut through the lexer's window, so a grammar
// that declares lex.window is read a window at a time. The tokens of the
// blocks the line closed and opened come first, of no width, where the line
// starts, one each time the lexer asks, and then the line itself: the tokens
// are in source order, and the last line's tokens also close every block
// still open, since no token follows the last line but the end.

import (
	"strings"
	"unicode/utf8"

	parser "github.com/tabnas/parser/go"
)

const (
	// lineToken is one physical line, read: the blocks it made precede it.
	lineToken = "#LB"
	// endToken is a container's end: its value is the fields that follow
	// its children.
	endToken = "#EN"
)

// nodeRule is a node of the AST, its rule and the token its rule opens on.
// A container's token carries its head (the fields before its children,
// type first), and its #EN its tail; a leaf's token carries the whole node.
// The rule is named for the node's type. The GFM nodes are the gfm group's.
type nodeRule struct {
	rule      string
	token     string
	container bool
	gfm       bool
	desc      string
}

var nodeRules = []nodeRule{
	{"blockquote", "#BQ", true, false, "block quote"},
	{"list", "#LS", true, false, "list"},
	{"listItem", "#LI", true, false, "list item"},
	{"paragraph", "#PA", true, false, "paragraph"},
	{"heading", "#HD", true, false, "ATX or setext heading"},
	{"table", "#TB", true, true, "GFM table"},
	{"tableRow", "#TR", true, true, "table row"},
	{"tableCell", "#TC", true, true, "table cell"},
	{"emphasis", "#EM", true, false, "emphasis"},
	{"strong", "#SG", true, false, "strong emphasis"},
	{"delete", "#DL", true, true, "strikethrough"},
	{"link", "#LK", true, false, "link"},
	{"linkReference", "#LR", true, false, "link reference"},
	{"thematicBreak", "#HR", false, false, "thematic break"},
	{"code", "#CD", false, false, "fenced or indented code"},
	{"html", "#HT", false, false, "HTML block or raw HTML"},
	{"definition", "#DF", false, false, "link reference definition"},
	{"text", "#TT", false, false, "text"},
	{"inlineCode", "#IC", false, false, "code span"},
	{"break", "#BR", false, false, "hard line break"},
	{"image", "#IM", false, false, "image"},
	{"imageReference", "#IR", false, false, "image reference"},
}

// tokenOfType is the token a node of each AST type opens with.
var tokenOfType = func() map[string]string {
	m := make(map[string]string, len(nodeRules))
	for _, n := range nodeRules {
		m[n.rule] = n.token
	}
	return m
}()

// mdASTFields is astFieldOrder as the host that walks the value reads it:
// transduce's ParserSource takes the order of a grammar's plain maps from
// ctx.Meta["fields"], so a document read whole is written in the order the
// canonical runtime's objects have, as the transducer part writes it.
var mdASTFields = func() []any {
	fields := make([]any, len(astFieldOrder))
	for i, k := range astFieldOrder {
		fields[i] = k
	}
	return fields
}()

// made is a token the matcher hands out: its name and its value.
type made struct {
	name string
	val  any
}

// mdParseState is the per-parse state carried on ctx.U["md"], seeded by the
// parse.prepare hook.
type mdParseState struct {
	bp   *blockParser
	meta map[string]any
	// keepTree is the caller's request for the native tree back.
	keepTree bool
	// inlineTn and inline are the inline phase's engine instance, built per
	// parse (a plugin instance may serve concurrent Parse calls from many
	// goroutines, and the engine's concurrency guarantee covers
	// construction, not shared parsing), and the inline parse a block's AST
	// is made with: references kept.
	inlineTn *parser.Tabnas
	inline   *inlineParser
	events   []BlockEvent
	// made are the tokens the line at the cursor made, handed out one a
	// call before the line's own token (next the one due), and read says
	// whether that line has been read: its blocks are decided, and its #LB
	// is what is left.
	made []made
	next int
	read bool
}

// line is one physical line at the cursor.
type line struct {
	text       string
	src        string
	len        int
	terminated bool
	last       bool
}

// cutLine reads one physical line at the cursor through the lexer's window:
// up to (not including) the next `\n`, `\r` or `\r\n`, with §2.3's NUL
// replacement. len covers the terminator, last says whether the source ends
// after it (which is why a final line ending does not make a trailing blank
// line). The window is filled past the line's end, so a token never ends at
// the window's end, where the lexer would cut it again over a wider one, and
// a line longer than the window is read on in doubling steps. A line is read
// twice, once to decide its blocks and once for its own token, and gives the
// same both times. Lengths are bytes; every byte compared is ASCII.
func cutLine(lex *parser.Lex) (line, bool) {
	w := lex.Window()
	if 0 == len(w) {
		if !lex.Fill(1) {
			return line{}, false
		}
		w = lex.Window()
	}

	i := 0
	for {
		for i < len(w) && '\n' != w[i] && '\r' != w[i] {
			i++
		}
		if i < len(w) {
			break
		}
		had := len(w)
		lex.Fill(2*had + 1)
		w = lex.Window()
		if len(w) == had {
			break
		}
	}

	next := i
	terminated := false
	if i < len(w) {
		terminated = true
		if '\r' == w[i] {
			if i+1 >= len(w) && lex.Fill(i+2) {
				w = lex.Window()
			}
			if i+1 < len(w) && '\n' == w[i+1] {
				next = i + 2
			} else {
				next = i + 1
			}
		} else {
			next = i + 1
		}
	}

	last := !lex.Fill(next + 1)
	if !last {
		w = lex.Window()
	}

	text := w[:i]
	if 0 <= strings.IndexByte(text, 0) {
		text = strings.ReplaceAll(text, "\x00", "�")
	}
	return line{text: text, src: w[:next], len: next, terminated: terminated, last: last}, true
}

// makeMdBlockMatcher builds the mdBlock matcher. Each invocation returns one
// token. At a line not yet read, it reads the line, feeds it to the shared
// blockParser (and, for the last line, closes every block still open), and
// gathers the tokens of the blocks the line closed and opened; those are
// handed out first, of no width, where the line starts, and then the line
// itself as a #LB token. The matcher owns the engine's point: SI advances
// past the terminator with the #LB, RI counts rows, and CI resets per line
// (the block algorithm computes spec §2.2 tab-expanded columns itself).
// Nothing reads the source but cutLine, through the window.
func makeMdBlockMatcher(opts Options, tins map[string]parser.Tin) parser.MakeLexMatcher {
	return func(_ *parser.LexConfig, _ *parser.Options) parser.LexMatcher {
		return func(lex *parser.Lex, _ *parser.Rule) *parser.Token {
			if lex.Ctx == nil {
				return nil
			}
			st, ok := lex.Ctx.U["md"].(*mdParseState)
			if !ok {
				return nil
			}

			if st.next < len(st.made) {
				m := st.made[st.next]
				st.next++
				return lex.Token(m.name, tins[m.name], m.val, "")
			}

			ln, ok := cutLine(lex)
			if !ok {
				return nil
			}

			if !st.read {
				st.bp.incorporateLine(ln.text)
				if ln.last {
					st.bp.finish()
				}
				var out []made
				for _, ev := range st.events {
					out = eventTokens(st, opts, ev, out)
				}
				st.events = st.events[:0]
				if 0 < len(out) {
					st.made = out
					st.next = 1
					st.read = true
					return lex.Token(out[0].name, tins[out[0].name], out[0].val, "")
				}
			}

			st.made = nil
			st.next = 0
			st.read = false

			pnt := lex.Cursor()
			tkn := lex.Token(lineToken, tins[lineToken], nil, ln.src)

			// Row/column bookkeeping is observability data (token positions,
			// traces): a consumed terminator starts a new row; an
			// unterminated final line just widens the current one, by
			// CHARACTERS, as the canonical runtime counts its UTF-16 units
			// (they differ only past U+FFFF, the recorded divergence).
			pnt.SI += ln.len
			if ln.terminated {
				pnt.RI++
				pnt.CI = 1
			} else {
				pnt.CI += utf8.RuneCountInString(ln.text)
			}

			return tkn
		}
	}
}

// ordered is an AST node's members as a token's value: in astFieldOrder,
// the canonical runtime's member order, which a reader of the rule passes
// writes them in, the children left out.
func ordered(node map[string]any) *parser.OrderedMap {
	om := parser.NewOrderedMap()
	for _, k := range orderedKeys(node) {
		if "children" != k {
			om.Set(k, node[k])
		}
	}
	return om
}

// eventTokens appends the tokens one block event makes.
func eventTokens(st *mdParseState, opts Options, ev BlockEvent, out []made) []made {
	switch ev.Kind {
	case EventOpen:
		head := blockHead(ev.Node)
		return append(out, made{tokenOfType[head["type"].(string)], ordered(head)})

	case EventRow:
		out = append(out, made{"#TR", ordered(blockHead(ev.Node))})
		for cell := ev.Node.FirstChild; cell != nil; cell = cell.Next {
			out = inlineBlockTokens(st, opts, cell, out)
		}
		return append(out, made{endToken, ordered(blockTail(ev.Node))})

	case EventDefinition:
		return append(out, made{"#DF", ordered(definitionNode(ev.Def))})

	case EventClose:
		node := ev.Node
		switch node.Type {
		case NodeBlockQuote, NodeList, NodeItem, NodeTable:
			return append(out, made{endToken, ordered(blockTail(node))})

		case NodeParagraph, NodeHeading:
			// A paragraph that held nothing but definitions is gone: its
			// definitions were written as they were read.
			if node.Parent == nil {
				return out
			}
			return inlineBlockTokens(st, opts, node, out)

		case NodeThematicBreak, NodeCodeBlock, NodeHTMLBlock:
			leaf := leafBlock(node)
			out = append(out, made{tokenOfType[leaf["type"].(string)], ordered(leaf)})
			if !st.keepTree {
				release(node)
			}
			return out
		}
	}
	return out
}

// inlineBlockTokens appends a block whose content is inline nodes (a
// paragraph, a heading, a table cell), parsed now: its head, its inline
// nodes, its end. The block's raw text stays on the native node when the
// caller wants the native tree back, which is parsed again, references
// resolved, once the document ends.
func inlineBlockTokens(st *mdParseState, opts Options, node *MdNode, out []made) []made {
	head := blockHead(node)
	target := node
	if st.keepTree {
		target = NewNode(node.Type)
		target.SourcePos = node.SourcePos
		target.StringContent = append([]byte(nil), node.StringContent...)
	}
	parseBlockEngine(st.inlineTn, st.inline, target)
	out = append(out, made{tokenOfType[head["type"].(string)], ordered(head)})
	out = inlineTokens(inlineChildren(target, opts), out)
	out = append(out, made{endToken, ordered(blockTail(node))})
	if !st.keepTree {
		release(node)
	}
	return out
}

// inlineTokens appends the tokens of a run of inline nodes, in document
// order: a container's head, its children's, its end; a leaf whole.
// Iterative, as the canonical runtime's is.
func inlineTokens(nodes []any, out []made) []made {
	type frame struct {
		list []any
		i    int
	}
	stack := []frame{{list: nodes}}
	for 0 < len(stack) {
		top := &stack[len(stack)-1]
		if top.i >= len(top.list) {
			stack = stack[:len(stack)-1]
			if 0 < len(stack) {
				out = append(out, made{endToken, parser.NewOrderedMap()})
			}
			continue
		}
		node, _ := top.list[top.i].(map[string]any)
		top.i++
		token := tokenOfType[node["type"].(string)]
		out = append(out, made{token, ordered(node)})
		if children, ok := node["children"].([]any); ok {
			stack = append(stack, frame{list: children})
		}
	}
	return out
}

// release drops what a written block holds that nothing reads again: its
// raw text, its literal and its inline nodes. The block itself stays in the
// native tree, whose later lines still read its flags.
func release(node *MdNode) {
	node.StringContent = nil
	node.Literal = ""
	node.FirstChild = nil
	node.LastChild = nil
}

// mdPrepare is the parse.prepare hook: fresh block-parser state for every
// parse, with the parse's meta captured for the keepTree handshake, and the
// AST's member order declared for a host that walks the value.
func mdPrepare(opts Options) func(ctx *parser.Context) {
	return func(ctx *parser.Context) {
		// A parse from a reader with no meta has none to write to.
		if ctx.Meta == nil {
			ctx.Meta = map[string]any{}
		}
		if ctx.U == nil {
			ctx.U = map[string]any{}
		}
		ctx.Meta["fields"] = mdASTFields
		keepTree := false
		if md, ok := ctx.Meta["md"].(map[string]any); ok {
			keepTree, _ = md["keepTree"].(bool)
		}
		st := &mdParseState{
			bp:       newBlockParser(opts),
			meta:     ctx.Meta,
			keepTree: keepTree,
			inlineTn: makeInlineTn(opts),
			inline:   &inlineParser{refmap: RefMap{}, options: opts, keepReferences: true},
		}
		st.bp.events = &st.events
		ctx.U["md"] = st
	}
}

// mdBegin is the markdown rule's open action: the document's children
// gather here.
func mdBegin(r *parser.Rule, _ *parser.Context) {
	children := []any{}
	r.Node = &children
}

// mdFinish is the markdown rule's close action on #ZZ: the document. When
// the caller passed meta["md"]["keepTree"], the native tree, with its
// references resolved, is handed back on the meta map: the engine-path
// conformance harness renders it.
func mdFinish(opts Options) parser.AltAction {
	return func(r *parser.Rule, ctx *parser.Context) {
		if st, ok := ctx.U["md"].(*mdParseState); ok && st.keepTree {
			parseInlinesEngine(st.inlineTn, st.bp.doc, st.bp.refmap, opts)
			if md, ok := st.meta["md"].(map[string]any); ok {
				md["tree"] = st.bp.doc
			}
		}
		children, _ := r.Node.(*[]any)
		list := []any{}
		if children != nil {
			list = *children
		}
		r.Node = map[string]any{"type": "document", "children": list}
	}
}

// mdOpen is a container node's open action: its children gather in a new
// list.
func mdOpen(r *parser.Rule, _ *parser.Context) {
	u := r.EnsureU()
	u["into"] = r.Node
	u["head"] = r.O0.Val
	children := []any{}
	r.Node = &children
}

// mdClose is a container node's close action on #EN: its node is its head,
// its children and its tail, added to its parent's children.
func mdClose(r *parser.Rule, _ *parser.Context) {
	u := r.EnsureU()
	node := map[string]any{}
	if head, ok := u["head"].(*parser.OrderedMap); ok {
		for _, k := range head.Keys {
			node[k] = head.Vals[k]
		}
	}
	children := []any{}
	if list, ok := r.Node.(*[]any); ok {
		children = *list
	}
	node["children"] = children
	if tail, ok := r.C0.Val.(*parser.OrderedMap); ok {
		for _, k := range tail.Keys {
			node[k] = tail.Vals[k]
		}
	}
	if into, ok := u["into"].(*[]any); ok {
		*into = append(*into, node)
	}
}

// mdLeaf is a leaf node's open action: the token's value is the node.
func mdLeaf(r *parser.Rule, _ *parser.Context) {
	node := map[string]any{}
	if om, ok := r.O0.Val.(*parser.OrderedMap); ok {
		for _, k := range om.Keys {
			node[k] = om.Vals[k]
		}
	}
	if list, ok := r.Node.(*[]any); ok {
		*list = append(*list, node)
	}
}
