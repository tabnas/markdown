/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Projection from the native CommonMark tree to the map-based JSON AST that
// this package has always returned. Port of ts/src/ast.ts — the two must agree
// node for node, since test/spec/*.tsv is the parity contract between them.
//
// Every children slice is allocated non-nil so that an empty container
// marshals as `[]` and not `null`. The previous Go port left them nil, which
// was a silent TS/Go output difference on every empty document, blockquote,
// list item and link label.

import (
	"strings"
	"unicode"
)

// astFieldOrder is a topological ordering of every member in the public
// AST. A node only contains the fields for its own type, so one list gives
// the canonical TypeScript insertion order for every node: a heading is
// type/depth/children, a code block type/lang/meta/value, a reference
// type/identifier/label/referenceType and then children or alt, a list
// type/ordered/start/children/spread and a list item
// type/children/spread/checked (the fields known only once the children are
// come after them). The Go AST uses plain maps, which cannot carry that order
// themselves: the engine path declares it to a host that walks the value
// (engineblock.go), and orders the members of each token's value by it.
var astFieldOrder = []string{
	"type", "align", "depth", "lang", "meta", "value", "identifier", "label",
	"referenceType", "url", "title", "alt", "ordered", "start", "children",
	"spread", "checked",
}

// astFieldRank is each field's place in astFieldOrder.
var astFieldRank = func() map[string]int {
	rank := make(map[string]int, len(astFieldOrder))
	for i, k := range astFieldOrder {
		rank[k] = i
	}
	return rank
}()

// orderedKeys is a node's keys in astFieldOrder.
func orderedKeys(node map[string]any) []string {
	keys := make([]string, 0, len(node))
	for _, k := range astFieldOrder {
		if _, ok := node[k]; ok {
			keys = append(keys, k)
		}
	}
	return keys
}

// labelIdentifier is a label's identifier, as mdast's: normalized as
// CommonMark matches labels (normalizeReference), in lower case, as the
// canonical runtime's toLowerCase writes it. Two labels a definition and a
// reference would match on have the same identifier.
func labelIdentifier(rawLabel string) string {
	return jsToLower(normalizeReference(rawLabel))
}

// jsToLower is JavaScript's String.prototype.toLowerCase over the upper-case
// text normalizeReference makes: the simple per-rune mapping, but for a
// capital sigma, which is final (ς) at the end of a word and σ elsewhere
// (Unicode's Final_Sigma), and U+0130, whose lower case is two runes.
func jsToLower(s string) string {
	if isASCII(s) {
		return strings.ToLower(s)
	}
	runes := []rune(s)
	var b strings.Builder
	b.Grow(len(s))
	for i, r := range runes {
		switch r {
		case 0x03A3:
			if finalSigma(runes, i) {
				b.WriteRune(0x03C2)
			} else {
				b.WriteRune(0x03C3)
			}
		case 0x0130:
			b.WriteString("i̇")
		default:
			b.WriteRune(unicode.ToLower(r))
		}
	}
	return b.String()
}

// finalSigma is Unicode's Final_Sigma condition at runes[i]: a cased letter
// before it and none after it, case-ignorable characters skipped either way.
func finalSigma(runes []rune, i int) bool {
	before := false
	for j := i - 1; 0 <= j; j-- {
		if caseIgnorable(runes[j]) {
			continue
		}
		before = cased(runes[j])
		break
	}
	if !before {
		return false
	}
	for j := i + 1; j < len(runes); j++ {
		if caseIgnorable(runes[j]) {
			continue
		}
		return !cased(runes[j])
	}
	return true
}

func cased(r rune) bool {
	return unicode.IsUpper(r) || unicode.IsLower(r) || unicode.IsTitle(r) ||
		unicode.Is(unicode.Other_Lowercase, r) || unicode.Is(unicode.Other_Uppercase, r)
}

func caseIgnorable(r rune) bool {
	switch r {
	case '\'', '.', ':', 0x00B7, 0x0387, 0x05F4, 0x2018, 0x2019, 0x2024, 0x2027,
		0xFE13, 0xFE52, 0xFE55, 0xFF07, 0xFF0E, 0xFF1A:
		return true
	}
	return unicode.In(r, unicode.Mn, unicode.Me, unicode.Cf, unicode.Lm, unicode.Sk)
}

// collectAltText flattens an image's inline children to plain text for `alt`.
func collectAltText(node *MdNode) string {
	var b strings.Builder
	for child := node.FirstChild; child != nil; child = child.Next {
		switch child.Type {
		case NodeText, NodeCode, NodeHTMLInline:
			b.WriteString(child.Literal)
		case NodeSoftbreak, NodeLinebreak:
			b.WriteByte('\n')
		default:
			b.WriteString(collectAltText(child))
		}
	}
	return b.String()
}

// itemIsSpread implements mdast semantics: an item is spread when two of its
// children are separated by a blank line, not merely when it has more than
// one child. `- a\n  - b` holds a paragraph and a nested list with no blank
// line between them and is not spread; `- a\n\n  - b` is.
//
// Derived from SourcePos rather than tracked separately: consecutive children
// whose line ranges are not adjacent must have had a blank line between them.
// The children are the AST's, definitions among them, each by the lines it
// spans.
func itemIsSpread(item *MdNode) bool {
	spread := false
	prevEnd := -1
	step := func(start, end int) {
		if 0 <= prevEnd && start > prevEnd+1 {
			spread = true
		}
		prevEnd = end
	}
	forEachBlockChild(item,
		func(def Definition) { step(def.Start, def.End) },
		func(child *MdNode) { step(child.SourcePos[0][0], child.SourcePos[1][0]) })
	return spread
}

// forEachBlockChild walks a container's children in the AST's order: each
// child, and each definition where it stands among them (see DefinitionSite).
func forEachBlockChild(container *MdNode, onDefinition func(Definition), onChild func(*MdNode)) {
	defs := container.Definitions
	d := 0
	for d < len(defs) && defs[d].After == nil {
		onDefinition(defs[d].Def)
		d++
	}
	for c := container.FirstChild; c != nil; c = c.Next {
		onChild(c)
		for d < len(defs) && defs[d].After == c {
			onDefinition(defs[d].Def)
			d++
		}
	}
}

// definitionNode is a definition's node.
func definitionNode(def Definition) map[string]any {
	return map[string]any{
		"type":       "definition",
		"identifier": labelIdentifier(def.Label),
		"label":      unescapeString(def.Label),
		"url":        def.Destination,
		"title":      nullableString(def.Title, def.HasTitle && def.Title != ""),
	}
}

func inlineChildren(node *MdNode, opts Options) []any {
	out := make([]any, 0, 4)

	// Adjacent text is merged into a single node, and the merge has to accrete
	// rather than concatenate: Go strings are immutable, so `s = s + more`
	// copies the whole prefix every time — O(n²) over one long run, which a
	// paragraph of soft-broken lines is exactly. (TypeScript's `+=` builds a
	// rope and is already linear, which is why only this side needs a builder.)
	//
	// `acc` holds the run in progress and `accNode` the node it will be written
	// to; anything that is not text ends the run, so every append that is not
	// pushText goes through `push`, which flushes first.
	var acc strings.Builder
	var accNode map[string]any

	flush := func() {
		if accNode != nil {
			accNode["value"] = acc.String()
			acc.Reset()
			accNode = nil
		}
	}

	pushText := func(value string) {
		if value == "" {
			return
		}
		if accNode == nil {
			accNode = map[string]any{"type": "text"}
			out = append(out, accNode)
		}
		acc.WriteString(value)
	}

	push := func(v any) {
		flush()
		out = append(out, v)
	}

	for child := node.FirstChild; child != nil; child = child.Next {
		switch child.Type {
		case NodeText:
			pushText(child.Literal)

		case NodeSoftbreak:
			// Documented behaviour: a soft break reads as a space in the AST.
			if opts.Breaks {
				push(map[string]any{"type": "break"})
			} else {
				pushText(" ")
			}

		case NodeLinebreak:
			push(map[string]any{"type": "break"})

		case NodeCode:
			push(map[string]any{"type": "inlineCode", "value": child.Literal})

		case NodeHTMLInline:
			push(map[string]any{"type": "html", "value": child.Literal})

		case NodeEmph:
			push(map[string]any{"type": "emphasis", "children": inlineChildren(child, opts)})

		case NodeStrong:
			push(map[string]any{"type": "strong", "children": inlineChildren(child, opts)})

		case NodeDel:
			push(map[string]any{"type": "delete", "children": inlineChildren(child, opts)})

		case NodeLink:
			push(map[string]any{
				"type":     "link",
				"url":      child.Destination,
				"title":    nullableString(child.Title, child.HasTitle),
				"children": inlineChildren(child, opts),
			})

		case NodeImage:
			push(map[string]any{
				"type":  "image",
				"url":   child.Destination,
				"title": nullableString(child.Title, child.HasTitle),
				"alt":   collectAltText(child),
			})

		case NodeLinkReference:
			push(map[string]any{
				"type":          "linkReference",
				"identifier":    labelIdentifier(child.Label),
				"label":         unescapeString(child.Label),
				"referenceType": child.ReferenceType,
				"children":      inlineChildren(child, opts),
			})

		case NodeImageReference:
			push(map[string]any{
				"type":          "imageReference",
				"identifier":    labelIdentifier(child.Label),
				"label":         unescapeString(child.Label),
				"referenceType": child.ReferenceType,
				"alt":           collectAltText(child),
			})
		}
	}

	flush()
	return out
}

// nullableString yields a value that marshals to JSON null when absent, so
// `title` matches the TypeScript `string | null`.
func nullableString(s string, present bool) any {
	if !present {
		return nil
	}
	return s
}

// nullableBool is nullableString for `listItem.checked`, which mdast defines
// as `boolean | null` — null for an item that is not a GFM task list item, and
// therefore null everywhere when GFM is off.
func nullableBool(b bool, present bool) any {
	if !present {
		return nil
	}
	return b
}

func blockChildren(node *MdNode, opts Options) []any {
	out := make([]any, 0, 4)
	forEachBlockChild(node,
		func(def Definition) { out = append(out, definitionNode(def)) },
		func(child *MdNode) {
			if b := toBlock(child, opts); b != nil {
				out = append(out, b)
			}
		})
	return out
}

// blockHead is the fields of a container block's node known when it opens:
// what precedes its children. Its fields known only when it closes are
// blockTail's. Mirrors ts/src/ast.ts blockHead.
func blockHead(node *MdNode) map[string]any {
	switch node.Type {
	case NodeBlockQuote:
		return map[string]any{"type": "blockquote"}
	case NodeList:
		data := node.ListData
		ordered := data != nil && data.Type == ListOrdered
		var start any
		if ordered {
			start = 1
			if data != nil {
				start = data.Start
			}
		}
		return map[string]any{"type": "list", "ordered": ordered, "start": start}
	case NodeItem:
		return map[string]any{"type": "listItem"}
	case NodeParagraph:
		return map[string]any{"type": "paragraph"}
	case NodeHeading:
		return map[string]any{"type": "heading", "depth": node.Level}
	case NodeTable:
		// []any with nil entries, not []TableAlign: a column with no colon is
		// JSON null in the canonical runtime, and a table with no columns is
		// `[]` rather than `null`.
		align := make([]any, 0, len(node.TableAlign))
		for _, a := range node.TableAlign {
			if AlignNone == a {
				align = append(align, nil)
			} else {
				align = append(align, string(a))
			}
		}
		return map[string]any{"type": "table", "align": align}
	case NodeTableRow:
		return map[string]any{"type": "tableRow"}
	case NodeTableCell:
		return map[string]any{"type": "tableCell"}
	}
	return map[string]any{}
}

// blockTail is the fields of a container block's node that follow its
// children.
func blockTail(node *MdNode) map[string]any {
	switch node.Type {
	case NodeList:
		tight := true
		if node.ListData != nil {
			tight = node.ListData.Tight
		}
		return map[string]any{"spread": !tight}
	case NodeItem:
		return map[string]any{
			"spread":  itemIsSpread(node),
			"checked": nullableBool(node.Checked, node.HasChecked),
		}
	}
	return map[string]any{}
}

// leafBlock is a leaf block's node: a code block, an HTML block or a
// thematic break.
func leafBlock(node *MdNode) map[string]any {
	switch node.Type {
	case NodeThematicBreak:
		return map[string]any{"type": "thematicBreak"}

	case NodeCodeBlock:
		// jsTrim/jsSpaceIndex, not strings.TrimSpace and an ASCII-only class:
		// the canonical runtime splits on `.trim()` and `.search(/\s/)`, whose
		// character set includes NBSP and U+FEFF but excludes U+0085. The block
		// phase already trimmed the info string, but unescapeString runs after
		// that trim, so `&#160;` and friends put whitespace back.
		info := jsTrim(node.Info)
		var lang, meta any
		if info != "" {
			sp, width := jsSpaceIndex(info)
			if sp < 0 {
				lang = info
			} else {
				lang = info[:sp]
				// Step a whole rune. The TypeScript's `sp + 1` advances one
				// UTF-16 unit, which is the same thing there because every
				// character in the class is BMP.
				if rest := jsTrim(info[sp+width:]); rest != "" {
					meta = rest
				}
			}
		}
		// The native tree keeps the trailing newline because the spec's HTML
		// for <pre><code> includes it; mdast's code.value does not. Strip
		// exactly one, not all, so a block genuinely ending in a blank line
		// keeps it.
		value := node.Literal
		if strings.HasSuffix(value, "\n") {
			value = value[:len(value)-1]
		}
		return map[string]any{"type": "code", "lang": lang, "meta": meta, "value": value}

	case NodeHTMLBlock:
		return map[string]any{"type": "html", "value": node.Literal}
	}
	return nil
}

// containerNode is a container node of the AST: its head, its children,
// then its tail.
func containerNode(node *MdNode, children []any) map[string]any {
	out := blockHead(node)
	out["children"] = children
	for k, v := range blockTail(node) {
		out[k] = v
	}
	return out
}

func toBlock(node *MdNode, opts Options) map[string]any {
	switch node.Type {
	case NodeParagraph, NodeHeading:
		return containerNode(node, inlineChildren(node, opts))

	case NodeThematicBreak, NodeCodeBlock, NodeHTMLBlock:
		return leafBlock(node)

	case NodeBlockQuote, NodeItem:
		// An item is only reached here if it is somehow detached from its
		// list; a list builds its own items below.
		return containerNode(node, blockChildren(node, opts))

	case NodeTable:
		// A table's children are rows and cells rather than blocks, so
		// blockChildren never descends into one; the nesting is a fixed three
		// levels deep and cannot be driven past the stack by input.
		rows := make([]any, 0, 4)
		for row := node.FirstChild; row != nil; row = row.Next {
			if row.Type != NodeTableRow {
				continue
			}
			cells := make([]any, 0, 4)
			for cell := row.FirstChild; cell != nil; cell = cell.Next {
				if cell.Type != NodeTableCell {
					continue
				}
				cells = append(cells, containerNode(cell, inlineChildren(cell, opts)))
			}
			rows = append(rows, containerNode(row, cells))
		}
		return containerNode(node, rows)

	case NodeList:
		items := make([]any, 0, 4)
		for child := node.FirstChild; child != nil; child = child.Next {
			if child.Type == NodeItem {
				items = append(items, containerNode(child, blockChildren(child, opts)))
			}
		}
		return containerNode(node, items)
	}

	return nil
}

// ToAST projects a parsed native tree into the public JSON AST.
func ToAST(doc *MdNode, opts Options) map[string]any {
	return map[string]any{"type": "document", "children": blockChildren(doc, opts)}
}

// --- resolving references -----------------------------------------------------

// resource is what ResolveReferences reads a reference's destination from.
type resource struct {
	url   any
	title any
}

// ResolveReferences returns the tree with its references resolved, as
// CommonMark reads a document: each linkReference and imageReference whose
// identifier a definition has becomes the link or the image that definition
// gives (the first definition of an identifier wins), one no definition has
// becomes its source text again (`[`, its text, `]`, then `[label]` for a
// full reference and `[]` for a collapsed one, as mdast-util-to-hast reverts
// one), and the definitions leave the tree. Adjacent text is merged, as the
// parse merges it. A new tree; the one given is not changed. Mirrors
// ts/src/ast.ts resolveReferences.
//
// The parse reads a bracketed label as a reference whether or not the
// document defines it, so where one with no definition decided how the text
// around it nests (emphasis that would have crossed it, a link whose text
// holds it), the result is the tree that reading gives, not the one
// CommonMark gives: the loss the manifest declares. Every other document
// resolves to the tree CommonMark reads.
func ResolveReferences(doc map[string]any) map[string]any {
	defs := map[string]resource{}
	collectDefinitions(doc, defs)
	return resolveNode(doc, defs)
}

// collectDefinitions gathers every definition in the tree, the first of each
// identifier kept.
func collectDefinitions(node map[string]any, defs map[string]resource) {
	if "definition" == node["type"] {
		id, _ := node["identifier"].(string)
		if _, seen := defs[id]; !seen {
			defs[id] = resource{url: node["url"], title: node["title"]}
		}
		return
	}
	children, _ := node["children"].([]any)
	for _, c := range children {
		if m, ok := c.(map[string]any); ok {
			collectDefinitions(m, defs)
		}
	}
}

// referenceSuffix is what follows a reference's text in its source.
func referenceSuffix(node map[string]any) string {
	switch node["referenceType"] {
	case ReferenceFull:
		label, _ := node["label"].(string)
		return "[" + label + "]"
	case ReferenceCollapsed:
		return "[]"
	}
	return ""
}

// resolveNode is a node with its references resolved: a copy, its children
// resolved in turn.
func resolveNode(node map[string]any, defs map[string]resource) map[string]any {
	out := make(map[string]any, len(node))
	for k, v := range node {
		out[k] = v
	}
	if children, ok := node["children"].([]any); ok {
		out["children"] = resolveChildren(children, defs)
	}
	return out
}

// resolveChildren is a run of children with their references resolved.
func resolveChildren(children []any, defs map[string]resource) []any {
	out := make([]any, 0, len(children))
	push := func(n any) {
		if m, ok := n.(map[string]any); ok && "text" == m["type"] && 0 < len(out) {
			if last, ok := out[len(out)-1].(map[string]any); ok && "text" == last["type"] {
				a, _ := last["value"].(string)
				b, _ := m["value"].(string)
				out[len(out)-1] = map[string]any{"type": "text", "value": a + b}
				return
			}
		}
		out = append(out, n)
	}
	text := func(s string) { push(map[string]any{"type": "text", "value": s}) }

	for _, c := range children {
		m, ok := c.(map[string]any)
		if !ok {
			push(c)
			continue
		}
		switch m["type"] {
		case "definition":
			continue
		case "imageReference":
			id, _ := m["identifier"].(string)
			alt, _ := m["alt"].(string)
			if res, found := defs[id]; found {
				push(map[string]any{"type": "image", "url": res.url, "title": res.title, "alt": alt})
			} else {
				text("![" + alt + "]" + referenceSuffix(m))
			}
		case "linkReference":
			id, _ := m["identifier"].(string)
			kids, _ := m["children"].([]any)
			resolved := resolveChildren(kids, defs)
			if res, found := defs[id]; found {
				push(map[string]any{"type": "link", "url": res.url, "title": res.title, "children": resolved})
			} else {
				text("[")
				for _, k := range resolved {
					push(k)
				}
				text("]" + referenceSuffix(m))
			}
		default:
			push(resolveNode(m, defs))
		}
	}
	return out
}
