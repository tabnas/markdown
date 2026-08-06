package tabnasmarkdown

import (
	"encoding/json"
	"reflect"
	"testing"

	parser "github.com/tabnas/parser/go"
)

func mdParse(src string, opts ...map[string]any) (map[string]any, error) {
	j := parser.Make()
	var o map[string]any
	if len(opts) > 0 {
		o = opts[0]
	}
	if err := j.UseDefaults(Markdown, Defaults, o); err != nil {
		return nil, err
	}
	res, err := j.Parse(src)
	if err != nil {
		return nil, err
	}
	if res == nil {
		return map[string]any{"type": "document", "children": []any{}}, nil
	}
	if m, ok := res.(map[string]any); ok {
		return m, nil
	}
	return nil, nil
}

func jsonRoundMD(v any) any {
	raw, _ := json.Marshal(v)
	var out any
	json.Unmarshal(raw, &out)
	return out
}

func TestEmpty(t *testing.T) {
	res, _ := mdParse("")
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, map[string]any{"type": "document", "children": []any{}}) {
		t.Fatalf("empty: got %#v", got)
	}
	res, _ = mdParse("\n")
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, map[string]any{"type": "document", "children": []any{}}) {
		t.Fatalf("blank: got %#v", got)
	}
}

func TestATXHeading(t *testing.T) {
	res, _ := mdParse("# Hello")
	want := map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "heading", "depth": float64(1), "children": []any{map[string]any{"type": "text", "value": "Hello"}}},
	}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("ATX: got %#v want %#v", got, want)
	}
	res, _ = mdParse("## Heading 2")
	want = map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "heading", "depth": float64(2), "children": []any{map[string]any{"type": "text", "value": "Heading 2"}}},
	}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("ATX2: got %#v", got)
	}
}

func TestSetextHeading(t *testing.T) {
	res, _ := mdParse("Foo\n===")
	want := map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "heading", "depth": float64(1), "children": []any{map[string]any{"type": "text", "value": "Foo"}}},
	}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("setext ===: got %#v want %#v", got, want)
	}
	res, _ = mdParse("Bar\n---")
	want = map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "heading", "depth": float64(2), "children": []any{map[string]any{"type": "text", "value": "Bar"}}},
	}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("setext ---: got %#v", got)
	}
}

func TestThematicBreak(t *testing.T) {
	res, _ := mdParse("---")
	want := map[string]any{"type": "document", "children": []any{map[string]any{"type": "thematicBreak"}}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("thematic: got %#v", got)
	}
}

func TestParagraph(t *testing.T) {
	res, _ := mdParse("Hello world")
	want := map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "paragraph", "children": []any{map[string]any{"type": "text", "value": "Hello world"}}},
	}}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("para: got %#v", got)
	}
	res, _ = mdParse("Hello\n\nWorld")
	// two paragraphs
	if m, ok := jsonRoundMD(res).(map[string]any); ok {
		if ch, ok := m["children"].([]any); ok && len(ch) != 2 {
			t.Fatalf("two paras: got %d children", len(ch))
		}
	}
}

func TestFencedCode(t *testing.T) {
	res, _ := mdParse("```\ncode here\n```")
	want := map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "code", "lang": nil, "meta": nil, "value": "code here"}},
	}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("fenced: got %#v want %#v", got, want)
	}
	res, _ = mdParse("```js\nconsole.log(\"hi\")\n```")
	want = map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "code", "lang": "js", "meta": nil, "value": "console.log(\"hi\")"}},
	}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("fenced js: got %#v", got)
	}
}

func TestIndentedCode(t *testing.T) {
	res, _ := mdParse("    indented\n    second")
	want := map[string]any{"type": "document", "children": []any{
		map[string]any{"type": "code", "lang": nil, "meta": nil, "value": "indented\nsecond"}},
	}
	if got := jsonRoundMD(res); !reflect.DeepEqual(got, want) {
		t.Fatalf("indented: got %#v", got)
	}
}

func TestBlockquote(t *testing.T) {
	res, _ := mdParse("> hello\n> world")
	got := jsonRoundMD(res).(map[string]any)
	ch := got["children"].([]any)[0].(map[string]any)
	if ch["type"] != "blockquote" {
		t.Fatalf("blockquote type got %v", ch["type"])
	}
}

func TestLists(t *testing.T) {
	res, _ := mdParse("- a\n- b\n- c")
	got := jsonRoundMD(res).(map[string]any)
	list := got["children"].([]any)[0].(map[string]any)
	if list["ordered"] != false {
		t.Fatalf("unordered: ordered should be false")
	}
	if children, ok := list["children"].([]any); !ok || len(children) != 3 {
		t.Fatalf("unordered: expected 3 items, got %#v", list["children"])
	}
	res, _ = mdParse("1. a\n2. b\n3. c")
	got = jsonRoundMD(res).(map[string]any)
	list = got["children"].([]any)[0].(map[string]any)
	if list["ordered"] != true {
		t.Fatalf("ordered: should be true")
	}
	if list["start"] != float64(1) {
		t.Fatalf("ordered start: got %v", list["start"])
	}
}

func TestInline(t *testing.T) {
	res, _ := mdParse("Hello *world*")
	got := jsonRoundMD(res).(map[string]any)
	para := got["children"].([]any)[0].(map[string]any)
	children := para["children"].([]any)
	if len(children) != 2 || children[1].(map[string]any)["type"] != "emphasis" {
		t.Fatalf("emphasis: got %#v", children)
	}
	res, _ = mdParse("Hello **world**")
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	children = para["children"].([]any)
	if children[1].(map[string]any)["type"] != "strong" {
		t.Fatalf("strong: got %#v", children)
	}
	res, _ = mdParse("`code`")
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	children = para["children"].([]any)
	if children[0].(map[string]any)["type"] != "inlineCode" {
		t.Fatalf("inlineCode: got %#v", children)
	}
	res, _ = mdParse("foo_bar_baz")
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	children = para["children"].([]any)
	if children[0].(map[string]any)["value"] != "foo_bar_baz" {
		t.Fatalf("underscore word: got %#v", children)
	}
}

func TestLinks(t *testing.T) {
	res, _ := mdParse("[link](https://example.com)")
	got := jsonRoundMD(res).(map[string]any)
	para := got["children"].([]any)[0].(map[string]any)
	link := para["children"].([]any)[0].(map[string]any)
	if link["type"] != "link" || link["url"] != "https://example.com" {
		t.Fatalf("link: got %#v", link)
	}
	res, _ = mdParse("![alt](https://example.com/img.png)")
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	img := para["children"].([]any)[0].(map[string]any)
	if img["type"] != "image" {
		t.Fatalf("image: got %#v", img)
	}
	res, _ = mdParse("<https://example.com>")
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	link = para["children"].([]any)[0].(map[string]any)
	if link["type"] != "link" {
		t.Fatalf("autolink: got %#v", link)
	}
}

func TestStrikethrough(t *testing.T) {
	res, _ := mdParse("~~delete~~")
	got := jsonRoundMD(res).(map[string]any)
	para := got["children"].([]any)[0].(map[string]any)
	del := para["children"].([]any)[0].(map[string]any)
	if del["type"] != "delete" {
		t.Fatalf("delete gfm: got %#v", del)
	}
	res, _ = mdParse("~~delete~~", map[string]any{"gfm": false})
	got = jsonRoundMD(res).(map[string]any)
	para = got["children"].([]any)[0].(map[string]any)
	txt := para["children"].([]any)[0].(map[string]any)
	if txt["type"] != "text" {
		t.Fatalf("delete non-gfm: expected text, got %#v", txt)
	}
}

func TestBareAndCompat(t *testing.T) {
	// Bare engine: the package depends on @tabnas/parser only.
	j := parser.Make()
	if err := j.Use(Markdown, nil); err != nil {
		t.Fatalf("Use Markdown: %v", err)
	}
	res, err := j.Parse("# hi")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if m, ok := res.(map[string]any); !ok || m["type"] != "document" {
		t.Fatalf("bare: got %#v", res)
	}
}
