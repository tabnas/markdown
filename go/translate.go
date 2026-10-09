package tabnasmarkdown

import _ "embed"

// TranslationPart is one optional alchemy source and the entry point a host calls.
type TranslationPart struct {
	Entry  string
	Source string
}

// TranslationParts is the package-local structural translation interface.
type TranslationParts struct {
	Manifest string
	Lift     *TranslationPart
	Embed    *TranslationPart
	Render   *TranslationPart
}

//go:embed translate/manifest.json
var translationManifest string

//go:embed translate/lift.alc
var translationLift string

//go:embed translate/render.alc
var translationRender string

var translationParts = TranslationParts{
	Manifest: translationManifest,
	Lift:     &TranslationPart{Entry: "markdown-lift", Source: translationLift},
	Render:   &TranslationPart{Entry: "markdown-render", Source: translationRender},
}

// Translate returns Markdown's immutable translation parts.
func Translate() *TranslationParts { return &translationParts }
