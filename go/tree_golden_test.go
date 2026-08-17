// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// tree_golden_test.go — golden native-tree snapshots, shared with the
// TypeScript runtime.
//
// The `test/spec/*.tsv` fixtures compare public ASTs, and the projection
// drops SourcePos — so a positional divergence between the runtimes is
// invisible to them. This suite pins the NATIVE tree instead: every fixture
// input is parsed with ParseTree and serialized to the canonical JSON shape
// both runtimes implement (see serializeTree here and in
// ts/test/tree-golden.test.ts), then compared against
// `test/spec/tree/<name>.json`. The goldens are generated from the canonical
// TypeScript:
//
//	cd ts && npm run build && MD_TREE_GOLDEN=write npm test
//
// so this suite failing means the Go native tree — sourcepos included — has
// drifted from the TypeScript one.

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	support "github.com/tabnas/support/go"
)

// treeLiteralTypes lists the node types whose Literal is meaningful. Kept as
// an explicit list because Go cannot distinguish an absent literal from an
// empty one — the serializers include the field for exactly these types.
var treeLiteralTypes = map[NodeType]bool{
	NodeText:       true,
	NodeCode:       true,
	NodeHTMLInline: true,
	NodeCodeBlock:  true,
	NodeHTMLBlock:  true,
}

// serializeTree produces the canonical cross-runtime serialization. It must
// stay field-for-field identical to serializeTree in
// ts/test/tree-golden.test.ts.
func serializeTree(n *MdNode) map[string]any {
	out := map[string]any{"type": string(n.Type), "sourcepos": n.SourcePos}

	if treeLiteralTypes[n.Type] {
		out["literal"] = n.Literal
	}
	if n.Type == NodeHeading {
		out["level"] = n.Level
	}
	if n.Type == NodeLink || n.Type == NodeImage {
		out["destination"] = n.Destination
		if n.HasTitle {
			out["title"] = n.Title
		}
	}
	if n.Type == NodeCodeBlock {
		out["isFenced"] = n.IsFenced
		if n.IsFenced {
			out["info"] = n.Info
			out["fenceChar"] = string(n.FenceChar)
			out["fenceLength"] = n.FenceLength
			out["fenceOffset"] = n.FenceOffset
		}
	}
	if n.ListData != nil {
		out["listData"] = map[string]any{
			"type":         string(n.ListData.Type),
			"tight":        n.ListData.Tight,
			"start":        n.ListData.Start,
			"delimiter":    n.ListData.Delimiter,
			"bulletChar":   n.ListData.BulletChar,
			"padding":      n.ListData.Padding,
			"markerOffset": n.ListData.MarkerOffset,
		}
	}
	if n.TableAlign != nil {
		arr := make([]any, len(n.TableAlign))
		for i, a := range n.TableAlign {
			if a == AlignNone {
				arr[i] = nil
			} else {
				arr[i] = string(a)
			}
		}
		out["tableAlign"] = arr
	}
	if n.IsHeaderRow {
		out["isHeaderRow"] = true
	}
	if n.HasChecked {
		out["checked"] = n.Checked
	}
	if n.Type == NodeDocument {
		out["gfm"] = n.GFM
	}

	var children []any
	for c := n.FirstChild; c != nil; c = c.Next {
		children = append(children, serializeTree(c))
	}
	if len(children) > 0 {
		out["children"] = children
	}

	return out
}

type treeGoldenCase struct {
	Input string `json:"input"`
	Opts  string `json:"opts"`
	Tree  any    `json:"tree"`
}

func TestTreeGoldens(t *testing.T) {
	dir, err := support.FindSpecDir("")
	if err != nil {
		t.Fatal(err)
	}
	files, err := support.LoadSpecDir(dir, nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(files) == 0 {
		t.Fatal("no spec fixtures found")
	}

	for _, spec := range files {
		goldenPath := filepath.Join(dir, "tree", strings.TrimSuffix(spec.Name, ".tsv")+".json")
		raw, err := os.ReadFile(goldenPath)
		if err != nil {
			t.Fatalf("tree golden missing for %s: %v (regenerate: cd ts && npm run build && MD_TREE_GOLDEN=write npm test)", spec.Name, err)
		}

		var golden []treeGoldenCase
		if err := json.Unmarshal(raw, &golden); err != nil {
			t.Fatalf("%s: %v", goldenPath, err)
		}
		if len(golden) != len(spec.Rows) {
			t.Fatalf("%s: %d golden cases for %d fixture rows — regenerate the goldens",
				spec.Name, len(golden), len(spec.Rows))
		}

		for i, g := range golden {
			opts := map[string]any{}
			if trimmed := strings.TrimSpace(g.Opts); trimmed != "" {
				if err := json.Unmarshal([]byte(trimmed), &opts); err != nil {
					t.Fatalf("%s case %d opts: %v", spec.Name, i, err)
				}
			}

			tree := ParseTree(support.Unescape(g.Input), ResolveOptions(opts))
			got := jsonFlatten(serializeTree(tree))
			if !reflect.DeepEqual(got, g.Tree) {
				gotJSON, _ := json.Marshal(got)
				wantJSON, _ := json.Marshal(g.Tree)
				t.Errorf("%s %s: native tree diverges from golden\n got: %s\nwant: %s",
					spec.Name, spec.Rows[i].Where(), gotJSON, wantJSON)
			}
		}
	}
}
