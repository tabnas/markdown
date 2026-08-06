/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

import (
	"regexp"
	"strings"

	jsonic "github.com/tabnas/jsonic/go"
)

const Version = "0.4.1"

// --- BEGIN EMBEDDED markdown-grammar.jsonic ---
const grammarText = `
# Markdown prose grammar — CommonMark/GFM subset
# See dx-report.md section 2 and ts/src/markdown.ts header.
# This grammar is intentionally trivial: the prose parser is implemented
# in JS inside the markdown rule bo action (parseDocument). The
# block structure therefore shows as a single markdown -> block fan-out
# in the railroad diagram, which is honest about where the complexity
# lives. The file is kept so ts/embed-grammar.js continues to embed a
# grammar verbatim into both runtimes (see AGENTS.md).
{
  rule: markdown: open: [
    { s: '#ZZ' }
  ]
  rule: markdown: close: [
    { s: '#ZZ' }
  ]
}
`

// --- END EMBEDDED markdown-grammar.jsonic ---

// Defaults mirrors TS Markdown.defaults.
var Defaults = map[string]any{
	"gfm":    true,
	"breaks": false,
}

// Markdown is the tabnas plugin for CommonMark/GFM prose. It expects a
// jsonic (or bare) engine; the test harness uses jsonic.Make.
// The block scanner reads ctx.Src() directly; lexers for string/comment/
// number/value are disabled so backticks and `#` headings are not
// mis-lexed before parseDocument runs.
func Markdown(j *jsonic.Jsonic, options map[string]any) error {
	if j.Decoration("markdown-init") != nil {
		return nil
	}
	j.Decorate("markdown-init", true)

	gfm := true
	if v, ok := options["gfm"]; ok {
		if b, ok := v.(bool); ok {
			gfm = b
		}
	}
	breaks := false
	if v, ok := options["breaks"]; ok {
		if b, ok := v.(bool); ok {
			breaks = b
		}
	}
	opts := mdOptions{gfm: gfm, breaks: breaks}

	// Lex: disable string/comment/number/value that would turn Markdown
	// syntax into bad tokens before the block scanner runs.
	j.SetOptions(jsonic.Options{
		String:  &jsonic.StringOptions{Lex: boolPtr(false)},
		Comment: &jsonic.CommentOptions{Lex: boolPtr(false)},
		Number:  &jsonic.NumberOptions{Lex: boolPtr(false)},
		Value:   &jsonic.ValueOptions{Lex: boolPtr(false)},
		Lex:     &jsonic.LexOptions{EmptyResult: map[string]any{"type": "document", "children": []any{}}},
		Rule:    &jsonic.RuleOptions{Start: "markdown"},
	})

	// Rule markdown: bo builds the document from ctx.Src, open/close
	// consume the token stream via #AA wildcard so trailing-content check passes.
	_ = grammarText

	j.Rule("markdown", func(rs *jsonic.RuleSpec, _ *jsonic.Parser) {
		rs.Clear()
		rs.AddBO(func(r *jsonic.Rule, ctx *jsonic.Context) {
			src := ctx.Src
			if strings.Trim(src, "\r\n\t ") == "" {
				r.Node = map[string]any{"type": "document", "children": []any{}}
				return
			}
			r.Node = parseDocument(src, opts)
		})
		rs.AddOpen(&jsonic.AltSpec{S: [][]jsonic.Tin{{jsonic.TinAA}}, R: "markdown"}, &jsonic.AltSpec{})
		rs.AddClose(&jsonic.AltSpec{S: [][]jsonic.Tin{{jsonic.TinZZ}}}, &jsonic.AltSpec{})
	})

	return nil
}

// ---------------------------------------------------------------------------
// Options and regexes

type mdOptions struct {
	gfm    bool
	breaks bool
}

var (
	reATXHeading       = regexp.MustCompile(`^ {0,3}(#{1,6})[ \t]+(.+?)(?:[ \t]+#+)?[ \t]*$`)
	reSetextUnderline  = regexp.MustCompile(`^ {0,3}(=+|-+)[ \t]*$`)
	reFencedStart      = regexp.MustCompile(`^ {0,3}(` + "`{3,}|~{3,})[ \t]*.*$")
	reIndentedCodeLine = regexp.MustCompile(`^( {4}|\t)`)
	reBlockquoteLine   = regexp.MustCompile(`^ {0,3}>\s?`)
	reUnorderedItem    = regexp.MustCompile(`^ {0,3}([*+-])[ \t]+(.*)$`)
	reOrderedItem      = regexp.MustCompile(`^ {0,3}(\d+)([.)])[ \t]+(.*)$`)
	reHtmlBlock        = regexp.MustCompile(`^ {0,3}<`)
	reHtmlTag          = regexp.MustCompile(`^<\/?[a-zA-Z][\w-]*(?:\s[^>]*)?>`)
)

// ---------------------------------------------------------------------------
// Inline helpers

func parseLinkDestination(inside string) (string, *string) {
	s := strings.TrimSpace(inside)
	if s == "" {
		return "", nil
	}
	if strings.HasPrefix(s, "<") {
		closeIdx := strings.Index(s[1:], ">")
		if closeIdx != -1 {
			url := s[1 : 1+closeIdx]
			rest := strings.TrimSpace(s[1+closeIdx+1:])
			if rest == "" {
				return url, nil
			}
			if (strings.HasPrefix(rest, "\"") && strings.HasSuffix(rest, "\"") && len(rest) > 1) ||
				(strings.HasPrefix(rest, "'") && strings.HasSuffix(rest, "'") && len(rest) > 1) ||
				(strings.HasPrefix(rest, "(") && strings.HasSuffix(rest, ")") && len(rest) > 1) {
				title := rest[1 : len(rest)-1]
				return url, &title
			}
			return s, nil
		}
	}
	// Unbracketed
	i := 0
	for i < len(s) && s[i] != ' ' && s[i] != '\t' {
		i++
	}
	url := s[:i]
	rest := strings.TrimSpace(s[i:])
	if rest == "" {
		return url, nil
	}
	if (strings.HasPrefix(rest, "\"") && strings.HasSuffix(rest, "\"") && len(rest) > 1) ||
		(strings.HasPrefix(rest, "'") && strings.HasSuffix(rest, "'") && len(rest) > 1) ||
		(strings.HasPrefix(rest, "(") && strings.HasSuffix(rest, ")") && len(rest) > 1) {
		title := rest[1 : len(rest)-1]
		return url, &title
	}
	return s, nil
}

func findClosingBracket(text string, start int) int {
	depth := 0
	for i := start; i < len(text); i++ {
		c := text[i]
		if c == '\\' && i+1 < len(text) {
			i++
			continue
		}
		if c == '[' {
			depth++
		} else if c == ']' {
			if depth == 0 {
				return i
			}
			depth--
		}
	}
	return -1
}

func findClosingParen(text string, start int) int {
	depth := 0
	for i := start; i < len(text); i++ {
		c := text[i]
		if c == '\\' && i+1 < len(text) {
			i++
			continue
		}
		if c == '(' {
			depth++
		} else if c == ')' {
			if depth == 0 {
				return i
			}
			depth--
		}
	}
	return -1
}

func parseInline(text string, opts mdOptions) []any {
	var nodes []any
	i := 0
	buf := ""

	flush := func() {
		if buf != "" {
			nodes = append(nodes, map[string]any{"type": "text", "value": buf})
			buf = ""
		}
	}

	for i < len(text) {
		ch := text[i]

		// Escapes
		if ch == '\\' && i+1 < len(text) {
			nxt := text[i+1]
			if strings.ContainsRune("\\`*_[]{}()#+-.!|>~", rune(nxt)) {
				buf += string(nxt)
				i += 2
				continue
			}
		}

		// Inline code
		if ch == '`' {
			j := i
			for j < len(text) && text[j] == '`' {
				j++
			}
			ticks := j - i
			fence := strings.Repeat("`", ticks)
			closeIdx := strings.Index(text[j:], fence)
			if closeIdx != -1 {
				raw := text[j : j+closeIdx]
				code := strings.Trim(strings.ReplaceAll(raw, "\n", " "), " ")
				// also collapse \r\n ?
				code = strings.ReplaceAll(code, "\r", "")
				flush()
				nodes = append(nodes, map[string]any{"type": "inlineCode", "value": code})
				i = j + closeIdx + ticks
				continue
			}
		}

		// Image ![alt](url)
		if strings.HasPrefix(text[i:], "![") {
			closeAlt := findClosingBracket(text, i+2)
			if closeAlt != -1 && closeAlt+1 < len(text) && text[closeAlt+1] == '(' {
				closeParen := findClosingParen(text, closeAlt+2)
				if closeParen != -1 {
					alt := text[i+2 : closeAlt]
					inside := text[closeAlt+2 : closeParen]
					url, title := parseLinkDestination(inside)
					if url != "" {
						flush()
						m := map[string]any{"type": "image", "url": url, "alt": alt}
						if title != nil {
							m["title"] = *title
						} else {
							m["title"] = nil
						}
						nodes = append(nodes, m)
						i = closeParen + 1
						continue
					}
				}
			}
		}

		// Autolink <https://...>
		if ch == '<' {
			closeIdx := strings.Index(text[i:], ">")
			if closeIdx != -1 {
				inner := text[i+1 : i+closeIdx]
				if strings.HasPrefix(inner, "https://") || strings.HasPrefix(inner, "http://") {
					if !strings.Contains(inner, " ") {
						flush()
						nodes = append(nodes, map[string]any{
							"type": "link", "url": inner, "title": nil,
							"children": []any{map[string]any{"type": "text", "value": inner}},
						})
						i += closeIdx + 1
						continue
					}
				}
				if strings.Contains(inner, "@") && !strings.Contains(inner, " ") && strings.Contains(inner, ".") {
					flush()
					nodes = append(nodes, map[string]any{
						"type": "link", "url": "mailto:" + inner, "title": nil,
						"children": []any{map[string]any{"type": "text", "value": inner}},
					})
					i += closeIdx + 1
					continue
				}
			}
		}

		// Link [label](url)
		if ch == '[' {
			closeBracket := findClosingBracket(text, i+1)
			if closeBracket != -1 {
				label := text[i+1 : closeBracket]
				if closeBracket+1 < len(text) && text[closeBracket+1] == '(' {
					closeParen := findClosingParen(text, closeBracket+2)
					if closeParen != -1 {
						inside := text[closeBracket+2 : closeParen]
						url, title := parseLinkDestination(inside)
						if url != "" {
							flush()
							m := map[string]any{
								"type": "link", "url": url,
								"children": parseInline(label, opts),
							}
							if title != nil {
								m["title"] = *title
							} else {
								m["title"] = nil
							}
							nodes = append(nodes, m)
							i = closeParen + 1
							continue
						}
					}
				}
			}
		}

		// Strong ** or __
		if strings.HasPrefix(text[i:], "**") || strings.HasPrefix(text[i:], "__") {
			delim := text[i : i+2]
			closeIdx := strings.Index(text[i+2:], delim)
			if closeIdx != -1 && closeIdx > 0 {
				inner := text[i+2 : i+2+closeIdx]
				if strings.TrimSpace(inner) != "" {
					flush()
					nodes = append(nodes, map[string]any{"type": "strong", "children": parseInline(inner, opts)})
					i += 2 + closeIdx + 2
					continue
				}
			}
		}

		// Strikethrough ~~ (gfm)
		if opts.gfm && strings.HasPrefix(text[i:], "~~") {
			closeIdx := strings.Index(text[i+2:], "~~")
			if closeIdx != -1 && closeIdx > 0 {
				inner := text[i+2 : i+2+closeIdx]
				if strings.TrimSpace(inner) != "" {
					flush()
					nodes = append(nodes, map[string]any{"type": "delete", "children": parseInline(inner, opts)})
					i += 2 + closeIdx + 2
					continue
				}
			}
		}

		// Emphasis * or _
		if ch == '*' || ch == '_' {
			if ch == '_' {
				prevIsAlnum := i > 0 && isWordChar(text[i-1])
				nextIsAlnum := i+1 < len(text) && isWordChar(text[i+1])
				if prevIsAlnum && nextIsAlnum {
					buf += string(ch)
					i++
					continue
				}
			}
			delim := string(ch)
			closeIdx := -1
			for k := i + 1; k < len(text); k++ {
				if text[k] == '\\' && k+1 < len(text) {
					k++
					continue
				}
				if string(text[k]) == delim {
					if ch == '_' {
						prevIsAlnum := k > 0 && isWordChar(text[k-1])
						nextIsAlnum := k+1 < len(text) && isWordChar(text[k+1])
						if prevIsAlnum && nextIsAlnum {
							continue
						}
					}
					if k+1 < len(text) && string(text[k+1]) == delim {
						continue
					}
					if k > 0 && string(text[k-1]) == delim && k >= 2 && strings.HasPrefix(text[k-1:], delim+delim) {
						continue
					}
					closeIdx = k
					break
				}
			}
			if closeIdx != -1 && closeIdx > i+1 {
				inner := text[i+1 : closeIdx]
				if strings.TrimSpace(inner) != "" {
					flush()
					nodes = append(nodes, map[string]any{"type": "emphasis", "children": parseInline(inner, opts)})
					i = closeIdx + 1
					continue
				}
			}
		}

		// Hard/soft break
		if ch == '\n' {
			prevTwoSpaces := i >= 2 && text[i-1] == ' ' && text[i-2] == ' '
			if prevTwoSpaces {
				if strings.HasSuffix(buf, "  ") {
					buf = buf[:len(buf)-2]
				} else if strings.HasSuffix(buf, " ") {
					buf = buf[:len(buf)-1]
				}
				flush()
				nodes = append(nodes, map[string]any{"type": "break"})
			} else if opts.breaks {
				flush()
				nodes = append(nodes, map[string]any{"type": "break"})
			} else {
				buf += " "
			}
			i++
			continue
		}

		buf += string(ch)
		i++
	}

	flush()
	return nodes
}

func isWordChar(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_'
}

// ---------------------------------------------------------------------------
// Block helpers

func isBlank(line string) bool {
	return strings.Trim(line, " \t") == ""
}

func isATXHeading(line string) []string {
	m := reATXHeading.FindStringSubmatch(line)
	return m
}

func isSetextUnderline(line string) []string {
	m := reSetextUnderline.FindStringSubmatch(line)
	return m
}

func isThematicBreak(line string) bool {
	trimmed := strings.Trim(line, " \t")
	if len(trimmed) < 3 {
		return false
	}
	// Must be only *, -, _ with optional spaces
	c := rune(trimmed[0])
	if c != '*' && c != '-' && c != '_' {
		return false
	}
	for _, ch := range trimmed {
		if ch != c && ch != ' ' && ch != '\t' {
			return false
		}
	}
	// Must have at least 3 of the marker char
	count := 0
	for _, ch := range trimmed {
		if rune(ch) == c {
			count++
		}
	}
	return count >= 3
}

func isFencedCodeStart(line string) []string {
	m := reFencedStart.FindStringSubmatch(line)
	return m
}

func isIndentedCodeLine(line string) bool {
	return reIndentedCodeLine.MatchString(line)
}

func isBlockquoteLine(line string) bool {
	return reBlockquoteLine.MatchString(line)
}

func isUnorderedItem(line string) []string {
	m := reUnorderedItem.FindStringSubmatch(line)
	return m
}

func isOrderedItem(line string) []string {
	m := reOrderedItem.FindStringSubmatch(line)
	return m
}

func isHtmlBlockLine(line string) bool {
	t := strings.TrimLeft(line, " \t")
	if !strings.HasPrefix(t, "<") {
		return false
	}
	if strings.HasPrefix(t, "<!--") || strings.HasPrefix(t, "<![CDATA[") || strings.HasPrefix(strings.ToUpper(t), "<!DOCTYPE") || strings.HasPrefix(t, "<?") {
		return true
	}
	// Match tag-like <div> / </a> / <br> etc., not autolink <https://...>
	if reHtmlTag.MatchString(t) {
		return true
	}
	return false
}

// ---------------------------------------------------------------------------
// Block parsing

func parseDocument(src string, opts mdOptions) map[string]any {
	lines := strings.Split(strings.ReplaceAll(src, "\r\n", "\n"), "\n")
	nodes, _ := parseBlocks(lines, 0, opts, 0)
	return map[string]any{"type": "document", "children": nodes}
}

func parseBlocks(lines []string, start int, opts mdOptions, baseIndent int) ([]any, int) {
	var nodes []any
	i := start
	for i < len(lines) {
		line := lines[i]
		if isBlank(line) {
			i++
			continue
		}

		// Fenced code
		if m := isFencedCodeStart(line); m != nil {
			fence := m[1]
			fenceChar := string(fence[0])
			// Extract info string after fence
			idx := strings.Index(line, fence)
			info := strings.TrimSpace(line[idx+len(fence):])
			lang := ""
			meta := ""
			if info != "" {
				parts := strings.Fields(info)
				if len(parts) > 0 {
					lang = parts[0]
					if len(parts) > 1 {
						meta = strings.Join(parts[1:], " ")
					}
				}
			}
			var langPtr interface{}
			if lang != "" {
				langPtr = lang
			}
			var metaPtr interface{}
			if meta != "" {
				metaPtr = meta
			}
			// Closing fence
			closingRe := regexp.MustCompile(`^ {0,3}` + regexp.QuoteMeta(fenceChar) + `{` + strings.Repeat(fenceChar, len(fence)) + `,}[ \t]*$`)
			// Simpler: build dynamically
			closingRe = regexp.MustCompile(`^ {0,3}` + regexp.QuoteMeta(string(fenceChar)) + `{` + itoa(len(fence)) + `,}[ \t]*$`)
			var content []string
			i++
			for i < len(lines) && !closingRe.MatchString(lines[i]) {
				content = append(content, lines[i])
				i++
			}
			if i < len(lines) {
				i++
			}
			nodes = append(nodes, map[string]any{
				"type": "code", "lang": langPtr, "meta": metaPtr, "value": strings.Join(content, "\n"),
			})
			continue
		}

		// Indented code
		if isIndentedCodeLine(line) && isUnorderedItem(line) == nil && isOrderedItem(line) == nil {
			var content []string
			for i < len(lines) && (isIndentedCodeLine(lines[i]) || isBlank(lines[i])) {
				if isBlank(lines[i]) {
					// Keep blank only if next non-blank is indented
					j := i + 1
					for j < len(lines) && isBlank(lines[j]) {
						j++
					}
					if j < len(lines) && isIndentedCodeLine(lines[j]) {
						content = append(content, "")
						i++
					} else {
						break
					}
				} else {
					raw := lines[i]
					if strings.HasPrefix(raw, "\t") {
						content = append(content, raw[1:])
					} else if len(raw) >= 4 {
						content = append(content, raw[4:])
					} else {
						content = append(content, strings.TrimLeft(raw, " \t"))
					}
					i++
				}
			}
			nodes = append(nodes, map[string]any{"type": "code", "lang": nil, "meta": nil, "value": strings.Join(content, "\n")})
			continue
		}

		// ATX heading
		if m := isATXHeading(line); m != nil {
			depth := len(m[1])
			content := strings.TrimSpace(m[2])
			// Remove trailing hashes already handled by regex, but trim again
			content = strings.TrimSpace(content)
			nodes = append(nodes, map[string]any{
				"type": "heading", "depth": depth, "children": parseInline(content, opts),
			})
			i++
			continue
		}

		// Thematic break (after setext check via paragraph path)
		if isThematicBreak(line) {
			nodes = append(nodes, map[string]any{"type": "thematicBreak"})
			i++
			continue
		}

		// Blockquote
		if isBlockquoteLine(line) {
			var bqLines []string
			for i < len(lines) && (isBlockquoteLine(lines[i]) || isBlank(lines[i])) {
				if isBlank(lines[i]) {
					nxt := ""
					if i+1 < len(lines) {
						nxt = lines[i+1]
					}
					if isBlockquoteLine(nxt) {
						bqLines = append(bqLines, "")
						i++
						continue
					}
					bqLines = append(bqLines, "")
					i++
					j := i
					for j < len(lines) && isBlank(lines[j]) {
						j++
					}
					if j < len(lines) && !isBlockquoteLine(lines[j]) && !isBlank(lines[j]) {
						break
					}
					continue
				}
				re := regexp.MustCompile(`^ {0,3}>[ \t]?(.*)$`)
				m := re.FindStringSubmatch(lines[i])
				if m != nil {
					bqLines = append(bqLines, m[1])
				} else {
					bqLines = append(bqLines, "")
				}
				i++
			}
			for len(bqLines) > 0 && bqLines[len(bqLines)-1] == "" {
				bqLines = bqLines[:len(bqLines)-1]
			}
			inner, _ := parseBlocks(bqLines, 0, opts, 0)
			nodes = append(nodes, map[string]any{"type": "blockquote", "children": inner})
			continue
		}

		// HTML block
		if isHtmlBlockLine(line) {
			var htmlLines []string
			for i < len(lines) && !isBlank(lines[i]) {
				htmlLines = append(htmlLines, lines[i])
				i++
				if i < len(lines) && isBlank(lines[i]) {
					break
				}
				nxt := ""
				if i < len(lines) {
					nxt = lines[i]
				}
				if isATXHeading(nxt) != nil || isFencedCodeStart(nxt) != nil || isThematicBreak(nxt) || isBlockquoteLine(nxt) || isUnorderedItem(nxt) != nil || isOrderedItem(nxt) != nil {
					break
				}
			}
			nodes = append(nodes, map[string]any{"type": "html", "value": strings.Join(htmlLines, "\n")})
			continue
		}

		// Lists
		ul := isUnorderedItem(line)
		ol := isOrderedItem(line)
		if ul != nil || ol != nil {
			ordered := ol != nil
			var startNum interface{}
			if ordered {
				n, _ := stringsParseInt(ol[1])
				startNum = n
			}
			var items []any
			spread := false
			for i < len(lines) {
				cur := lines[i]
				if isBlank(cur) {
					i++
					continue
				}
				ulM := isUnorderedItem(cur)
				olM := isOrderedItem(cur)
				var markerMatch []string
				if ordered {
					markerMatch = olM
				} else {
					markerMatch = ulM
				}
				if markerMatch == nil {
					if len(items) > 0 && (strings.HasPrefix(cur, "  ") || strings.HasPrefix(cur, "\t")) && isATXHeading(cur) == nil && isFencedCodeStart(cur) == nil && !isThematicBreak(cur) && !isBlockquoteLine(cur) && !isHtmlBlockLine(cur) {
						// continuation - will be handled inside item
					} else {
						break
					}
					break
				}
				// Parse one item
				reMarker := regexp.MustCompile(`^ {0,3}(?:[*+-]|\d+[.)])[ \t]+`)
				mLen := len(reMarker.FindString(cur))
				var firstContent string
				if ordered {
					firstContent = olM[3]
				} else {
					firstContent = ulM[2]
				}
				itemLines := []string{firstContent}
				i++
				blankRun := 0
				for i < len(lines) {
					nxt := lines[i]
					if isBlank(nxt) {
						peek := ""
						if i+1 < len(lines) {
							peek = lines[i+1]
						}
						if isUnorderedItem(peek) != nil || isOrderedItem(peek) != nil {
							break
						}
						itemLines = append(itemLines, "")
						blankRun++
						i++
						continue
					}
					if (isUnorderedItem(nxt) != nil || isOrderedItem(nxt) != nil) && !strings.HasPrefix(nxt, "    ") {
						break
					}
					if strings.HasPrefix(nxt, "  ") || strings.HasPrefix(nxt, "\t") {
						stripped := nxt
						if strings.HasPrefix(stripped, "\t") {
							stripped = stripped[1:]
						} else {
							lead := 0
							for lead < len(stripped) && stripped[lead] == ' ' {
								lead++
							}
							if lead > 4 {
								lead = 4
							}
							stripped = stripped[lead:]
						}
						itemLines = append(itemLines, stripped)
						i++
						blankRun = 0
						continue
					}
					if isATXHeading(nxt) != nil || isFencedCodeStart(nxt) != nil || isThematicBreak(nxt) || isBlockquoteLine(nxt) || isHtmlBlockLine(nxt) {
						break
					}
					if blankRun > 0 {
						break
					}
					itemLines = append(itemLines, nxt)
					i++
				}
				itemHadBlank := false
				for _, l := range itemLines {
					if l == "" {
						itemHadBlank = true
						break
					}
				}
				if itemHadBlank {
					spread = true
				}
				for len(itemLines) > 0 && itemLines[len(itemLines)-1] == "" {
					itemLines = itemLines[:len(itemLines)-1]
				}
				src := strings.Join(itemLines, "\n")
				var itemBlocks []any
				if strings.TrimSpace(src) != "" {
					itemBlocks, _ = parseBlocks(strings.Split(strings.ReplaceAll(src, "\r\n", "\n"), "\n"), 0, opts, 0)
				}
				items = append(items, map[string]any{"type": "listItem", "spread": itemHadBlank, "children": itemBlocks})
				_ = mLen
			}
			nodes = append(nodes, map[string]any{"type": "list", "ordered": ordered, "start": startNum, "spread": spread, "children": items})
			continue
		}

		// Paragraph — collect until blank or block start
		{
			var paraLines []string
			for i < len(lines) && !isBlank(lines[i]) {
				nxt := lines[i]
				if len(paraLines) > 0 {
					isSetext := isSetextUnderline(nxt) != nil
					isThematicButNotSetext := isThematicBreak(nxt) && !isSetext
					if isATXHeading(nxt) != nil || isFencedCodeStart(nxt) != nil || isThematicButNotSetext || isBlockquoteLine(nxt) || isUnorderedItem(nxt) != nil || isOrderedItem(nxt) != nil || isHtmlBlockLine(nxt) {
						break
					}
					if isIndentedCodeLine(nxt) {
						break
					}
				}
				paraLines = append(paraLines, nxt)
				i++
			}
			if len(paraLines) > 0 {
				last := paraLines[len(paraLines)-1]
				if m := isSetextUnderline(last); m != nil && len(paraLines) >= 2 {
					depth := 2
					if m[1][0] == '=' {
						depth = 1
					}
					hdText := strings.TrimSpace(strings.Join(paraLines[:len(paraLines)-1], "\n"))
					nodes = append(nodes, map[string]any{"type": "heading", "depth": depth, "children": parseInline(hdText, opts)})
					continue
				}
			}
			paraText := strings.TrimRight(strings.Join(paraLines, "\n"), " \t")
			if paraText != "" {
				nodes = append(nodes, map[string]any{"type": "paragraph", "children": parseInline(paraText, opts)})
			}
			continue
		}
	}
	return nodes, i
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	s := ""
	for n > 0 {
		s = string(rune('0'+n%10)) + s
		n /= 10
	}
	return s
}

func stringsParseInt(s string) (int, bool) {
	n := 0
	for _, ch := range s {
		if ch < '0' || ch > '9' {
			return 0, false
		}
		n = n*10 + int(ch-'0')
	}
	return n, true
}

func boolPtr(b bool) *bool { return &b }

// BuildMarkdownStringMatcher is retained for the export test (TS: buildMarkdownStringMatcher).
// It implements RFC-4180 double-quote escaping ("a""b" → a"b) on top of the
// default string matcher, matching the old CSV behaviour.
func BuildMarkdownStringMatcher(stringOpts map[string]any) jsonic.MakeLexMatcher {
	// This is a minimal port of the TS buildMarkdownStringMatcher: it
	// returns a matcher that handles ""-escaped quotes inside a quoted
	// field. The implementation delegates to the standard JSON matcher
	// for other cases but ensures "" is collapsed to ".
	return func(cfg *jsonic.LexConfig, _ *jsonic.Options) jsonic.LexMatcher {
		return func(lex *jsonic.Lex, _ *jsonic.Rule) *jsonic.Token {
			q := "\""
			if v, ok := stringOpts["quote"].(string); ok && v != "" {
				q = v
			}
			qMap := map[byte]bool{q[0]: true}
			pnt := lex.Cursor()
			src := lex.Src
			sI := pnt.SI
			if sI >= len(src) || !qMap[src[sI]] {
				return nil
			}
			qR := pnt.RI
			sI++
			pnt.CI++
			var buf []byte
			for sI < len(src) {
				pnt.CI++
				c := src[sI]
				if c == q[0] {
					sI++
					pnt.CI++
					if sI < len(src) && src[sI] == q[0] {
						buf = append(buf, q[0])
					} else {
						break
					}
				} else {
					bI := sI
					qc := q[0]
					cc := src[sI]
					for sI < len(src) && cc >= 32 && cc != qc {
						sI++
						if sI < len(src) {
							cc = src[sI]
						}
						pnt.CI++
					}
					pnt.CI--
					if sI < len(src) && src[sI] < 32 {
						return lex.Bad("unprintable")
					}
					buf = append(buf, src[bI:sI]...)
					sI--
				}
				sI++
			}
			if sI == 0 || src[sI-1] != q[0] {
				pnt.RI = qR
				return lex.Bad("unterminated_string")
			}
			tkn := lex.Token("#ST", jsonic.TinST, string(buf), src[pnt.SI:sI])
			pnt.SI = sI
			return tkn
		}
	}
}
