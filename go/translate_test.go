package tabnasmarkdown

import (
	"encoding/json"
	"os"
	"testing"
)

func TestTranslationParts(t *testing.T) {
	parts := Translate()
	if parts == nil {
		t.Fatal("Translate returned nil")
	}
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Manifest != string(manifest) {
		t.Fatal("embedded manifest differs from tabnas.plugin.json")
	}
	if parts.Lift == nil || parts.Lift.Entry != "markdown-lift" {
		t.Fatalf("lift entry is %#v", parts.Lift)
	}
	lift, err := os.ReadFile("../alchemy/lift.alc")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Lift.Source != string(lift) {
		t.Fatal("embedded lift differs from alchemy/lift.alc")
	}
	if parts.Render == nil || parts.Render.Entry != "markdown-render" {
		t.Fatalf("render entry is %#v", parts.Render)
	}
	render, err := os.ReadFile("../alchemy/render.alc")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Render.Source != string(render) {
		t.Fatal("embedded render differs from alchemy/render.alc")
	}
}

// An embed takes a plain tree into a format's own schema. Markdown's
// events carry an mdast tree, but its render writes from records, which
// any tree's rows give, so its manifest names no embed and the package
// carries none; a manifest that named one would be held to its file here,
// as the lift and the render are above.
func TestTranslationEmbed(t *testing.T) {
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	var spec struct {
		Translate struct {
			Embed *string `json:"embed"`
		} `json:"translate"`
	}
	if err := json.Unmarshal(manifest, &spec); err != nil {
		t.Fatal(err)
	}
	parts := Translate()
	if spec.Translate.Embed == nil {
		if parts.Embed != nil {
			t.Fatalf("the manifest names no embed, and Translate carries %#v", parts.Embed)
		}
		return
	}
	if parts.Embed == nil || parts.Embed.Entry != "markdown-embed" {
		t.Fatalf("embed entry is %#v", parts.Embed)
	}
	embed, err := os.ReadFile("../" + *spec.Translate.Embed)
	if err != nil {
		t.Fatal(err)
	}
	if parts.Embed.Source != string(embed) {
		t.Fatalf("embedded embed differs from %s", *spec.Translate.Embed)
	}
}
