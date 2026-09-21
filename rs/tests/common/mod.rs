// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use tabnas_markdown::Options;

/// The repository root: the parent of `rs/`.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// One example of a vendored HTML corpus.
#[derive(Debug, Clone)]
pub struct SpecCase {
    pub markdown: String,
    pub html: String,
    pub example: u64,
    pub section: String,
}

fn load_cases(rel: &str, want: usize, what: &str) -> Vec<SpecCase> {
    let path = repo_root().join(rel);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
    let cases: Vec<SpecCase> = json
        .as_array()
        .unwrap_or_else(|| panic!("{}: not a JSON array", path.display()))
        .iter()
        .map(|c| SpecCase {
            markdown: c["markdown"].as_str().expect("markdown").to_string(),
            html: c["html"].as_str().expect("html").to_string(),
            example: c["example"].as_u64().expect("example"),
            section: c["section"].as_str().expect("section").to_string(),
        })
        .collect();
    assert_eq!(
        cases.len(),
        want,
        "expected {what}, got {} examples",
        cases.len()
    );
    cases
}

/// The vendored CommonMark 0.31.2 suite: exactly 652 examples, or the
/// grader fails.
pub fn load_spec_cases() -> Vec<SpecCase> {
    load_cases("test/commonmark/spec.json", 652, "the full 0.31.2 suite")
}

/// The vendored GFM extension corpus: exactly 24 examples, or the grader
/// fails.
pub fn load_gfm_cases() -> Vec<SpecCase> {
    load_cases(
        "test/gfm/spec.json",
        24,
        "the vendored 24-example extension corpus",
    )
}

/// What these extensions are gated on.
pub const GFM_OPTS: Options = Options {
    gfm: true,
    breaks: false,
};

/// The CommonMark baseline the whole 652-example suite runs with.
pub const CM_OPTS: Options = Options::COMMONMARK;

/// Render a parse result as plain JSON, with integral numbers as
/// integers. `tabnas::Value` holds every number as an `f64`, so `depth: 1`
/// would otherwise serialize as `1.0` and never equal a fixture's `1`.
/// This is the Rust spelling of the Go `jsonFlatten`.
pub fn to_json(value: &tabnas::Value) -> serde_json::Value {
    normalize_numbers(value.to_json())
}

/// Integral floats as integers, throughout.
pub fn normalize_numbers(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 => {
                serde_json::Value::from(f as i64)
            }
            _ => serde_json::Value::Number(n),
        },
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(normalize_numbers).collect())
        }
        serde_json::Value::Object(entries) => serde_json::Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, normalize_numbers(value)))
                .collect(),
        ),
        other => other,
    }
}

/// The fastest of `n` runs, which sheds scheduler noise far better than
/// a single sample or a mean.
pub fn best_of(n: usize, mut f: impl FnMut() -> Duration) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..n {
        let d = f();
        if d < best {
            best = d;
        }
    }
    best
}

// --- seeded pseudo-random documents -----------------------------------------

/// The fragment pool, byte-identical to FRAGMENTS in
/// ts/test/differential.test.ts and differentialFragments in
/// go/differential_test.go; a change here must land there in the same
/// commit.
pub const FRAGMENTS: [&str; 33] = [
    "# H1\n",
    "para text with *emph* and _under_\n",
    "- item one\n",
    "- [x] task\n",
    "1. ordered\n",
    "> quoted line\n",
    "> a | b\n> --- | ---\n",
    "```\ncode\n```\n",
    "a | b\n--- | ---\nc | d\n",
    "[l](u \"t`t\")\n",
    "[a](b`c) `d`\n",
    "[not a `link](/foo`)\n",
    "`code` span\n",
    "hard  \nbreak\n",
    "soft \nbreak\n",
    "[ref]: /url \"title\"\n",
    "use [ref] and [missing] here\n",
    "~~del~~ and ~one~ tilde\n",
    "<div>\nhtml block\n</div>\n",
    "inline <em>html</em> tag\n",
    "escaped \\* star \\| pipe\n",
    "&amp; entity &#65; and &nosuch;\n",
    "bare www.example.com literal\n",
    "scheme https://x.example/z?q=1 literal\n",
    "mail a@b.co literal\n",
    "***\n",
    "Setext\n===\n",
    "Setext two\n---\n",
    "    indented code\n",
    "> [a](b`c) `d`\n",
    "![img](/i.png \"alt`tick\")\n",
    "tab\there\n",
    "\n",
];

/// xorshift32, identical to nextRand in the other two runtimes.
pub fn next_rand(mut state: u32) -> u32 {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state
}

/// The deterministic document for a seed; the other two runtimes build
/// the same one, so a reproduction case can be named by its seed in any
/// of them.
pub fn fuzz_doc(seed: u32) -> String {
    let mut state = seed.wrapping_mul(2_654_435_761);
    if state == 0 {
        state = 1;
    }
    state = next_rand(state);
    let count = 3 + (state % 12) as usize;
    let mut doc = String::new();
    for _ in 0..count {
        state = next_rand(state);
        doc.push_str(FRAGMENTS[(state as usize) % FRAGMENTS.len()]);
    }
    doc
}
