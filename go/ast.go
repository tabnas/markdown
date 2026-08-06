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

	pushText := func(value string) {
		if value == "" {
			return
		}
		if n := len(out); n > 0 {
			if last, ok := out[n-1].(map[string]any); ok && last["type"] == "text" {
				last["value"] = last["value"].(string) + value
				return
			}
		}
		out = append(out, map[string]any{"type": "text", "value": value})
	}

	for child := node.FirstChild; child != nil; child = child.Next {
		switch child.Type {
		case NodeText:
			pushText(child.Literal)

		case NodeSoftbreak:
			// Documented behaviour: a soft break reads as a space in the AST.
			if opts.Breaks {
				out = append(out, map[string]any{"type": "break"})
			} else {
				pushText(" ")
			}

		case NodeLinebreak:
			out = append(out, map[string]any{"type": "break"})

		case NodeCode:
			out = append(out, map[string]any{"type": "inlineCode", "value": child.Literal})

		case NodeHTMLInline:
			out = append(out, map[string]any{"type": "html", "value": child.Literal})

		case NodeEmph:
			out = append(out, map[string]any{"type": "emphasis", "children": inlineChildren(child, opts)})

		case NodeStrong:
			out = append(out, map[string]any{"type": "strong", "children": inlineChildren(child, opts)})

		case NodeDel:
			out = append(out, map[string]any{"type": "delete", "children": inlineChildren(child, opts)})

		case NodeLink:
			out = append(out, map[string]any{
				"type":     "link",
				"url":      child.Destination,
				"title":    nullableString(child.Title, child.HasTitle),
				"children": inlineChildren(child, opts),
			})

		case NodeImage:
			out = append(out, map[string]any{
				"type":  "image",
				"url":   child.Destination,
				"title": nullableString(child.Title, child.HasTitle),
				"alt":   collectAltText(child),
			})
		}
	}

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
		info := strings.TrimSpace(node.Info)
		var lang, meta any
		if info != "" {
			if sp := strings.IndexAny(info, " \t\n\v\f\r"); sp < 0 {
				lang = info
			} else {
				lang = info[:sp]
				if rest := strings.TrimSpace(info[sp+1:]); rest != "" {
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
			"children": blockChildren(node, opts),
		}
	}

	return nil
}

// ToAST projects a parsed native tree into the public JSON AST.
func ToAST(doc *MdNode, opts Options) map[string]any {
	return map[string]any{"type": "document", "children": blockChildren(doc, opts)}
}
