// The differential gate: the plugin path must agree with the engine-free
// path.
//
// `parse_document(src, opts)` (engine-free) and `make_with(opts).parse(src)`
// (plugin path) are required to produce the same AST for every input,
// compared after JSON flattening. Today the two paths share every line of
// code, so this suite is trivially green; that is the point. It exists so
// the engine-substrate stages (dx-report §42) cannot land a divergence
// silently: the conformance corpora are blind to several legal-input
// behaviors (see test/spec/mixed.tsv), so "all suites green" is necessary
// but not sufficient once the plugin path stops sharing the drivers.
//
// The seeded document generator is identical to the one in
// ts/test/differential.test.ts and go/differential_test.go: same
// fragments, same xorshift32, same documents, so a reproduction case can
// be named by its seed in any runtime.

mod common;

use std::path::Path;

use common::{fuzz_doc, load_gfm_cases, load_spec_cases, to_json};
use tabnas::Tabnas;
use tabnas_markdown::{make_with, parse_document, Options};
use tabnas_support::{find_spec_dir, load_spec_dir, SpecOptions};

const OPTION_SETS: [Options; 4] = [
    Options {
        gfm: true,
        breaks: false,
    },
    Options {
        gfm: false,
        breaks: false,
    },
    Options {
        gfm: true,
        breaks: true,
    },
    Options {
        gfm: false,
        breaks: true,
    },
];

/// One engine instance per option set, reused across every parse: the
/// supported pattern (instance construction is what perf_test.rs pins as
/// the dominant cost).
fn rigs() -> Vec<Tabnas> {
    OPTION_SETS.iter().map(make_with).collect()
}

fn check_differential(rigs: &[Tabnas], src: &str, where_: &str, failures: &mut Vec<String>) {
    for (rig, opts) in rigs.iter().zip(OPTION_SETS.iter()) {
        let direct = parse_document(src, opts);
        let plugin = match rig.parse(src) {
            Ok(value) => value,
            Err(error) => {
                failures.push(format!(
                    "{where_} opts={opts:?}: plugin path error: {error}"
                ));
                continue;
            }
        };
        let plugin = to_json(&plugin);
        let direct = to_json(&direct);
        if plugin != direct {
            failures.push(format!(
                "{where_} opts={opts:?}: plugin path diverges from engine-free path\n plugin: {plugin}\n direct: {direct}"
            ));
        }
    }
}

fn assert_clean(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} divergences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn differential_commonmark() {
    let rigs = rigs();
    let mut failures = Vec::new();
    for c in load_spec_cases() {
        check_differential(
            &rigs,
            &c.markdown,
            &format!("commonmark example {}", c.example),
            &mut failures,
        );
    }
    assert_clean(failures);
}

#[test]
fn differential_gfm() {
    let rigs = rigs();
    let mut failures = Vec::new();
    for c in load_gfm_cases() {
        check_differential(
            &rigs,
            &c.markdown,
            &format!("gfm example {}", c.example),
            &mut failures,
        );
    }
    assert_clean(failures);
}

/// The engine short-circuits "" through `lex.empty_result` before the
/// rule loop, a path no corpus example or fixture reaches.
#[test]
fn differential_empty() {
    let mut failures = Vec::new();
    check_differential(&rigs(), "", "empty input", &mut failures);
    assert_clean(failures);
}

#[test]
fn differential_fixtures() {
    let dir = find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("test/spec is found above rs/");
    let files = load_spec_dir(&dir, &SpecOptions::default()).expect("the fixtures load");
    assert!(!files.is_empty(), "no spec fixtures found");
    let rigs = rigs();
    let mut failures = Vec::new();
    for spec in &files {
        for row in &spec.rows {
            check_differential(
                &rigs,
                &row.unesc_named("input"),
                &format!("{} {}", spec.file, row.location()),
                &mut failures,
            );
        }
    }
    assert_clean(failures);
}

const FUZZ_DOCS: u32 = 200;

#[test]
fn differential_fuzz() {
    let rigs = rigs();
    let mut failures = Vec::new();
    for seed in 1..=FUZZ_DOCS {
        check_differential(
            &rigs,
            &fuzz_doc(seed),
            &format!("fuzz seed {seed}"),
            &mut failures,
        );
    }
    assert_clean(failures);
}

/// The generator itself is part of the cross-runtime contract: the same
/// seed must name the same document in every runtime.
#[test]
fn fuzz_generator_is_stable() {
    assert_eq!(common::next_rand(1), 270_369);
    let doc = fuzz_doc(1);
    assert!(!doc.is_empty());
    assert_eq!(doc, fuzz_doc(1));
    assert_ne!(fuzz_doc(1), fuzz_doc(2));
}
