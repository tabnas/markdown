/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Types shared by both parse phases. Port of `ts/src/options.ts` and
//! `go/options.go`.

use std::collections::HashMap;

/// The parse options. [`Default`] is this package's defaults (`gfm` on,
/// `breaks` off), the same pair as `Markdown.defaults` in TypeScript and
/// `Defaults` in Go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Enable the GFM extensions. Default true.
    ///
    /// Five are implemented, and this one flag gates all of them together:
    /// tables, strikethrough (`~~x~~`), task list items (`- [x] foo`),
    /// autolink literals (bare `www.` / `http://` / `https://` / `ftp://` /
    /// `a@b.co`) and the disallowed-raw-HTML filter. Footnotes are not
    /// implemented.
    ///
    /// The first four are parse-time; the raw-HTML filter is applied by the
    /// renderer, which is why `html.rs` reads this option too. With `gfm`
    /// off the output is plain CommonMark, byte for byte.
    pub gfm: bool,
    /// Render soft line breaks as hard breaks. Default false.
    pub breaks: bool,
}

impl Default for Options {
    fn default() -> Self {
        DEFAULT_OPTIONS
    }
}

/// The package defaults: `gfm` on, `breaks` off.
pub const DEFAULT_OPTIONS: Options = Options {
    gfm: true,
    breaks: false,
};

impl Options {
    /// Plain CommonMark: every extension off. What the conformance corpus
    /// runs with.
    pub const COMMONMARK: Options = Options {
        gfm: false,
        breaks: false,
    };

    /// Apply the defaults to a plugin option bag. Unknown keys are
    /// ignored; a key present with a non-boolean value keeps the default.
    /// This is the Rust spelling of `resolveOptions` / `ResolveOptions`.
    pub fn resolve(bag: &tabnas::Value) -> Options {
        let mut out = DEFAULT_OPTIONS;
        let tabnas::Value::Object(map) = bag else {
            return out;
        };
        if let Some(tabnas::Value::Bool(gfm)) = map.get("gfm") {
            out.gfm = *gfm;
        }
        if let Some(tabnas::Value::Bool(breaks)) = map.get("breaks") {
            out.breaks = *breaks;
        }
        out
    }

    /// The same resolution over a JSON bag, which is how the shared
    /// fixtures and the golden files spell a row's options.
    pub fn resolve_json(bag: &serde_json::Value) -> Options {
        let mut out = DEFAULT_OPTIONS;
        if let Some(gfm) = bag.get("gfm").and_then(serde_json::Value::as_bool) {
            out.gfm = gfm;
        }
        if let Some(breaks) = bag.get("breaks").and_then(serde_json::Value::as_bool) {
            out.breaks = breaks;
        }
        out
    }

    /// The options as a plugin option bag, for `Tabnas::use_plugin`.
    pub fn to_value(self) -> tabnas::Value {
        let mut map = indexmap::IndexMap::new();
        map.insert("gfm".to_string(), tabnas::Value::Bool(self.gfm));
        map.insert("breaks".to_string(), tabnas::Value::Bool(self.breaks));
        tabnas::Value::object(map)
    }
}

/// A resolved link reference definition (section 4.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefDef {
    pub destination: String,
    /// `None` when the definition carried no title.
    pub title: Option<String>,
}

/// Keys are labels put through `normalize_reference`.
pub type RefMap = HashMap<String, RefDef>;
