/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Types shared by both parse phases. Port of ts/src/options.ts.

// Options controls the parse. Zero value is pure CommonMark with GFM off;
// use ResolveOptions to apply this package's defaults instead.
type Options struct {
	// GFM enables the GFM extensions. Default true.
	//
	// Four are implemented, and this one flag gates all of them together:
	// strikethrough (~~x~~), task list items (- [x] foo), autolink literals
	// (bare www. / http:// / https:// / ftp:// / a@b.co) and the
	// disallowed-raw-HTML filter. Tables and footnotes are not implemented.
	//
	// The first three are parse-time; the raw-HTML filter is applied by the
	// renderer, which is why html.go reads this option too. With GFM off the
	// output is plain CommonMark, byte for byte.
	GFM bool
	// Breaks renders soft line breaks as hard breaks. Default false.
	Breaks bool
}

// DefaultOptions mirrors Defaults and ts/src/options.ts.
var DefaultOptions = Options{GFM: true, Breaks: false}

// ResolveOptions applies defaults to a plugin option map. Unknown keys are
// ignored; a key present with a non-bool value keeps the default.
func ResolveOptions(opts map[string]any) Options {
	out := DefaultOptions
	if opts == nil {
		return out
	}
	if v, ok := opts["gfm"].(bool); ok {
		out.GFM = v
	}
	if v, ok := opts["breaks"].(bool); ok {
		out.Breaks = v
	}
	return out
}

// RefDef is a resolved link reference definition (§4.7).
type RefDef struct {
	Destination string
	Title       string
	HasTitle    bool
}

// RefMap keys are labels put through normalizeReference.
type RefMap map[string]RefDef
