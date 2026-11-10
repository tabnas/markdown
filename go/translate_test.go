package tabnasmarkdown

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"testing"
)

func TestTranslationParts(t *testing.T) {
	inCheckout(t)
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
// events carry its own mdast-adjacent tree, but its render writes from
// records, which any tree's rows give, so its manifest names no embed and
// the package carries none; a manifest that named one would be held to its
// file here, as the lift and the render are above.
func TestTranslationEmbed(t *testing.T) {
	inCheckout(t)
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

// TestTranslationTransduce holds the transducer part the manifest names to
// the one the package carries, and the manifest to giving no `whole`
// sentence beside it: a document streams through the part, a block at a
// time.
func TestTranslationTransduce(t *testing.T) {
	inCheckout(t)
	var spec struct {
		Translate struct {
			Transduce string  `json:"transduce"`
			Whole     *string `json:"whole"`
		} `json:"translate"`
	}
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(manifest, &spec); err != nil {
		t.Fatal(err)
	}
	if spec.Translate.Transduce != "alchemy/transduce.alc" || spec.Translate.Whole != nil {
		t.Fatalf("translate names transduce %q and whole %v", spec.Translate.Transduce, spec.Translate.Whole)
	}
	parts := Translate()
	if parts.Transduce == nil || parts.Transduce.Entry != "markdown-transduce" {
		t.Fatalf("transduce entry is %#v", parts.Transduce)
	}
	transduce, err := os.ReadFile("../" + spec.Translate.Transduce)
	if err != nil {
		t.Fatal(err)
	}
	if parts.Transduce.Source != string(transduce) {
		t.Fatalf("embedded transducer differs from %s: run npm run embed in ts", spec.Translate.Transduce)
	}
}

// Every call hands back parts of its own: a caller that changes what it
// was given, the parts or a part they point to, changes nothing the next
// caller reads, and callers on several goroutines share nothing to race on.
func TestTranslateReturnsACopy(t *testing.T) {
	first := Translate()
	want := *Translate().Render
	first.Manifest = ""
	if first.Render != nil {
		first.Render.Entry = ""
		first.Render.Source = ""
	}
	first.Lift, first.Embed, first.Render = nil, nil, nil
	second := Translate()
	if second.Manifest == "" {
		t.Fatal("a change to one call's manifest reached the next call")
	}
	if second.Render == nil || *second.Render != want {
		t.Fatalf("a change to one call's render reached the next call: %#v", second.Render)
	}
}

// inCheckout skips a test that holds the embedded copies to the
// repository's own files when it runs where those files are not, as from
// the module cache, whose zip holds the go/ module alone. In a checkout,
// a missing file still fails the test that reads it.
func inCheckout(t *testing.T) {
	t.Helper()
	if _, err := os.Stat("../ts/package.json"); errors.Is(err, fs.ErrNotExist) {
		t.Skip("not in a checkout of the repository: the module cache holds the go/ module alone")
	}
}
