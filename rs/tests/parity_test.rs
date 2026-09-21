// Cross-runtime conformance, driven by the shared `test/spec/*.tsv`
// fixtures at the repo root (see ../../test/AGENTS.md).
//
// The fixture loader, the escape codec, the ERROR:<code> contract and the
// row loop all come from tabnas-support, whose TypeScript and Go halves
// ts/test/parity.test.ts and go/parity_test.go use to run the SAME files,
// so no two of the three implementations can drift without one of them
// going red, and neither can the loaders.
//
// What is left here is only what is specific to markdown: how to build
// the parser for a row's options, and how to flatten a result for
// comparison.

use std::path::Path;

use tabnas_markdown::{make_with, Options};
use tabnas_support::{find_spec_dir, Failure, Runner, Value};

/// Every fixture in the spec directory. `find_spec_dir` walks up from the
/// crate directory, and `dir` discovers the files by listing, so adding a
/// .tsv runs it in all three runtimes without touching any runner.
#[test]
fn spec() {
    let dir = find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("test/spec is found above rs/");

    Runner::new_with_row(|input, row| {
        // A fresh parser per row: the `opts` column is per-case, and
        // plugin options must not leak from one row into the next.
        let raw = row.named("opts");
        let opts = if raw.trim().is_empty() {
            Options::default()
        } else {
            let bag: serde_json::Value = serde_json::from_str(raw)
                .map_err(|error| Failure::message(format!("opts column: {error}")))?;
            Options::resolve_json(&bag)
        };

        let parser = make_with(&opts);
        let value = parser
            .parse(input)
            .map_err(|error| Failure::new(error.code.clone()).with_message(error.to_string()))?;

        // Flatten through JSON so the parser's own containers and numeric
        // types compare against the fixture's decoded shape.
        Ok(Value::from(value.to_json()))
    })
    .dir(dir);
}
