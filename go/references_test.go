// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// references_test.go — references stay unresolved in the AST, as mdast keeps
// them (admin ADR-33, ruling 6), and ResolveReferences gives the tree back
// with them resolved, as CommonMark reads a document: CommonMark's tree for
// every example of both corpora but the five registered, where a label with
// no definition decided how the text around it nests. Mirrors
// ts/test/references.test.ts; the definitions and references themselves are
// pinned in every runtime by test/spec/reference.tsv.

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

// resolvedOtherwise are the CommonMark examples where resolving the tree's
// references does not give the tree CommonMark reads, each with why: the
// declared loss. An example that comes to resolve as CommonMark reads it
// fails here, and is then deleted.
var resolvedOtherwise = map[int]string{
	512: "the inner [bar] reads as a reference, so the outer link text holds a link and is no link",
	523: "the [bar* baz] reads as a reference, so the emphasis cannot cross into it",
	528: "the inner [bar] reads as a reference, so the outer link text holds a link and is no link",
	569: "[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]",
	571: "[foo][bar] reads as a full reference with no definition, not [foo] and then [bar][baz]",
}

// commonmarkReading is CommonMark's reading of a document as an AST,
// without its definitions.
func commonmarkReading(src string, opts Options) map[string]any {
	var strip func(node map[string]any) map[string]any
	strip = func(node map[string]any) map[string]any {
		children, ok := node["children"].([]any)
		if !ok {
			return node
		}
		out := map[string]any{}
		for k, v := range node {
			out[k] = v
		}
		kept := []any{}
		for _, c := range children {
			m := c.(map[string]any)
			if "definition" != m["type"] {
				kept = append(kept, strip(m))
			}
		}
		out["children"] = kept
		return out
	}
	return strip(ToAST(Parse(src, opts), opts))
}

func asJSON(t *testing.T, v any) string {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func TestReferencesResolveToTheirLinks(t *testing.T) {
	doc := ParseDocument("[a] ![b][a] [c][]\n\n[a]: /1 \"t\"\n[A]: /2\n[c]: /3\n", DefaultOptions)
	want := `{"children":[{"children":[{"children":[{"type":"text","value":"a"}],"title":"t","type":"link","url":"/1"},` +
		`{"type":"text","value":" "},{"alt":"b","title":"t","type":"image","url":"/1"},{"type":"text","value":" "},` +
		`{"children":[{"type":"text","value":"c"}],"title":null,"type":"link","url":"/3"}],"type":"paragraph"}],"type":"document"}`
	if got := asJSON(t, ResolveReferences(doc)); got != want {
		t.Fatalf("got  %s\nwant %s", got, want)
	}
}

func TestReferencesWithNoDefinitionReadAsTheirSource(t *testing.T) {
	doc := ParseDocument("x [a] [b][] [c *d*][e] ![f][g] y\n", DefaultOptions)
	want := `{"children":[{"children":[{"type":"text","value":"x [a] [b][] [c "},` +
		`{"children":[{"type":"text","value":"d"}],"type":"emphasis"},{"type":"text","value":"][e] ![f][g] y"}],` +
		`"type":"paragraph"}],"type":"document"}`
	if got := asJSON(t, ResolveReferences(doc)); got != want {
		t.Fatalf("got  %s\nwant %s", got, want)
	}
}

func TestReferencesResolvingLeavesTheTreeAsItWas(t *testing.T) {
	doc := ParseDocument("[a]\n\n[a]: /u\n", DefaultOptions)
	before := asJSON(t, doc)
	ResolveReferences(doc)
	if after := asJSON(t, doc); after != before {
		t.Fatalf("resolving changed the tree:\n%s\n%s", before, after)
	}
}

func TestReferencesResolveAsCommonMarkReadsButTheRegistered(t *testing.T) {
	for _, corpus := range []struct {
		name  string
		cases []specCase
		opts  Options
	}{
		{"commonmark", loadSpecCases(t), Options{GFM: false}},
		{"gfm", loadGFMCases(t), DefaultOptions},
	} {
		otherwise := map[int]string{}
		for _, c := range corpus.cases {
			resolved := asJSON(t, ResolveReferences(ParseDocument(c.Markdown, corpus.opts)))
			reading := asJSON(t, commonmarkReading(c.Markdown, corpus.opts))
			if resolved != reading {
				otherwise[c.Example] = strings.TrimSpace(c.Markdown)
			}
		}
		registered := map[int]string{}
		if "commonmark" == corpus.name {
			registered = resolvedOtherwise
		}
		gotKeys, wantKeys := []int{}, []int{}
		for k := range otherwise {
			gotKeys = append(gotKeys, k)
		}
		for k := range registered {
			wantKeys = append(wantKeys, k)
		}
		if len(gotKeys) != len(wantKeys) {
			t.Fatalf("%s: %d examples resolve otherwise (%v), %d are registered", corpus.name, len(gotKeys), otherwise, len(wantKeys))
		}
		for k := range registered {
			if _, ok := otherwise[k]; !ok {
				t.Fatalf("%s example %d resolves as CommonMark reads it now: delete its entry", corpus.name, k)
			}
		}
	}
}

func TestReferencesIdentifierIsTheLowerCasedKey(t *testing.T) {
	for _, c := range []struct{ label, want string }{
		{"Foo  Bar", "foo bar"},
		{"a\\*b", "a\\*b"},
		{"ẞ", "ss"},
		{"ΟΔΟΣ", "οδος"},
		{"ΟΔΟΣ ΚΑΙ", "οδος και"},
		{"ΣΑ", "σα"},
	} {
		got := labelIdentifier(c.label)
		if !reflect.DeepEqual(got, c.want) {
			t.Errorf("labelIdentifier(%q) = %q, want %q", c.label, got, c.want)
		}
	}
}
