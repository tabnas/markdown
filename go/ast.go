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

import "strings"

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
func itemIsSpread(item *MdNode) bool {
	for child := item.FirstChild; child != nil && child.Next != nil; child = child.Next {
		endLine := child.SourcePos[1][0]
		nextStartLine := child.Next.SourcePos[0][0]
		if nextStartLine > endLine+1 {
			return true
		}
	}
	return false
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
	for child := node.FirstChild; child != nil; child = child.Next {
		if b := toBlock(child, opts); b != nil {
			out = append(out, b)
		}
	}
	return out
}

func toBlock(node *MdNode, opts Options) map[string]any {
	switch node.Type {
	case NodeParagraph:
		return map[string]any{"type": "paragraph", "children": inlineChildren(node, opts)}

	case NodeHeading:
		return map[string]any{"type": "heading", "depth": node.Level, "children": inlineChildren(node, opts)}

	case NodeThematicBreak:
		return map[string]any{"type": "thematicBreak"}

	case NodeBlockQuote:
		return map[string]any{"type": "blockquote", "children": blockChildren(node, opts)}

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
				cells = append(cells, map[string]any{
					"type":     "tableCell",
					"children": inlineChildren(cell, opts),
				})
			}
			rows = append(rows, map[string]any{"type": "tableRow", "children": cells})
		}
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
		return map[string]any{"type": "table", "align": align, "children": rows}

	case NodeList:
		data := node.ListData
		ordered := data != nil && data.Type == ListOrdered
		items := make([]any, 0, 4)
		for child := node.FirstChild; child != nil; child = child.Next {
			if child.Type != NodeItem {
				continue
			}
			items = append(items, map[string]any{
				"type":     "listItem",
				"spread":   itemIsSpread(child),
				"checked":  nullableBool(child.Checked, child.HasChecked),
				"children": blockChildren(child, opts),
			})
		}
		var start any
		if ordered {
			start = 1
			if data != nil {
				start = data.Start
			}
		}
		tight := true
		if data != nil {
			tight = data.Tight
		}
		return map[string]any{
			"type":     "list",
			"ordered":  ordered,
			"start":    start,
			"spread":   !tight,
			"children": items,
		}

	case NodeItem:
		// Only reachable if an item is somehow detached from its list.
		return map[string]any{
			"type":     "listItem",
			"spread":   itemIsSpread(node),
			"checked":  nullableBool(node.Checked, node.HasChecked),
			"children": blockChildren(node, opts),
		}
	}

	return nil
}

// ToAST projects a parsed native tree into the public JSON AST.
func ToAST(doc *MdNode, opts Options) map[string]any {
	return map[string]any{"type": "document", "children": blockChildren(doc, opts)}
}
