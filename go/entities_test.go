/* Copyright (c) 2025-2026 Richard Rodger and other contributors, MIT License */

package tabnasmarkdown

// Named character references (§6.2), checked against the canonical table.
//
// Go decodes named references with the standard library's HTML5 table rather
// than carrying its own copy; ts/src/entities.ts is the canonical list (2125
// semicolon-terminated entries, generated from the WHATWG entities.json). The
// stdlib table is not the same set and its matching rules are not the same
// rules, so this reads the TypeScript table and asserts the two agree on every
// entry. Two defects it pins, both of which made Go and TypeScript disagree on
// ordinary prose:
//
//   - html.UnescapeString honours the legacy semicolon-less aliases, and
//     honours them as a *prefix* of a longer name, so `&ampa;` came back as
//     `&a;` and `&nbspa;` as " a;". §6.2 admits only the exact
//     semicolon-terminated names, so both are literal text.
//   - `&nGt;` and `&nLt;` are in the WHATWG table but not the stdlib's;
//     missingEntities supplies them.

import (
	"encoding/json"
	"os"
	"strconv"
	"strings"
	"testing"
)

// canonicalEntities reads the generated TypeScript table — the one source of
// truth for which names §6.2 admits and what they decode to.
func canonicalEntities(t *testing.T) map[string]string {
	t.Helper()
	raw, err := os.ReadFile("../ts/src/entities.ts")
	if err != nil {
		t.Fatalf("read entities.ts: %v", err)
	}
	src := string(raw)

	open := strings.Index(src, "JSON.parse(")
	if open < 0 {
		t.Fatal("entities.ts: no JSON.parse( — has the generator changed shape?")
	}
	start := strings.Index(src[open:], `"`)
	if start < 0 {
		t.Fatal("entities.ts: no string literal after JSON.parse(")
	}
	start += open
	end := strings.LastIndex(src, `"`)
	if end <= start {
		t.Fatal("entities.ts: unterminated string literal")
	}

	inner, err := strconv.Unquote(src[start : end+1])
	if err != nil {
		t.Fatalf("entities.ts: unquote: %v", err)
	}
	var table map[string]string
	if err := json.Unmarshal([]byte(inner), &table); err != nil {
		t.Fatalf("entities.ts: parse: %v", err)
	}
	if len(table) != 2125 {
		t.Fatalf("entities.ts: expected 2125 names, got %d", len(table))
	}
	return table
}

// TestEntityTableMatchesTypeScript checks every canonical name decodes to the
// canonical value, and that no near-miss decodes at all.
func TestEntityTableMatchesTypeScript(t *testing.T) {
	table := canonicalEntities(t)

	for name, want := range table {
		ref := "&" + name + ";"
		if got := decodeEntity(ref); got != want {
			t.Errorf("decodeEntity(%q) = %q, want %q", ref, got, want)
		}

		// One more name character makes it a different name. Some of those are
		// themselves real names (`sup` -> `sup1`, `le` -> `leq`), so the
		// expectation is "decodes iff the table has it" — the exact rule §6.2
		// states, and the one html.UnescapeString breaks by matching a legacy
		// alias prefix and leaving the tail behind.
		for _, extra := range []string{"q", "Z", "1"} {
			longer := name + extra
			if len(longer) > 32 {
				continue // beyond entityPattern's name limit
			}
			near := "&" + longer + ";"
			want := near
			if v, ok := table[longer]; ok {
				want = v
			}
			if got := decodeEntity(near); got != want {
				t.Errorf("decodeEntity(%q) = %q, want %q", near, got, want)
			}
		}
	}
}

// TestEntityRendersLikeTypeScript is the same contract one level up, through
// the public renderer, for the shapes that actually regressed.
func TestEntityLegacyAliasNotMatchedAsPrefix(t *testing.T) {
	cases := []struct{ in, want string }{
		{"&ampa;\n", "<p>&amp;ampa;</p>\n"},
		{"&ampA;\n", "<p>&amp;ampA;</p>\n"},
		{"&nbspa;\n", "<p>&amp;nbspa;</p>\n"},
		{"&ltx;\n", "<p>&amp;ltx;</p>\n"},
		{"&gtx;\n", "<p>&amp;gtx;</p>\n"},
		{"&quotx;\n", "<p>&amp;quotx;</p>\n"},
		{"&copyx;\n", "<p>&amp;copyx;</p>\n"},
		{"&AMPa;\n", "<p>&amp;AMPa;</p>\n"},
		// The genuine references still resolve.
		{"&amp;\n", "<p>&amp;</p>\n"},
		{"&nbsp;\n", "<p> </p>\n"},
		{"&semi;\n", "<p>;</p>\n"},
		{"&nGt;\n", "<p>≫⃒</p>\n"},
		{"&nLt;\n", "<p>≪⃒</p>\n"},
		// A name with no semicolon is not a reference either.
		{"&amp\n", "<p>&amp;amp</p>\n"},
	}
	for _, c := range cases {
		if got := ToHTML(c.in, Options{}); got != c.want {
			t.Errorf("ToHTML(%q) = %q, want %q", c.in, got, c.want)
		}
	}
}
