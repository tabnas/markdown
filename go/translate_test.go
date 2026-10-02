package tabnasmarkdown

import (
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
