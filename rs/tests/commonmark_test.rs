// CommonMark 0.31.2 conformance, over the vendored 652-example suite in
// test/commonmark/spec.json: the same corpus ts/test/commonmark.test.ts
// and go/commonmark_test.go run, so the three runtimes are held to one
// standard rather than to each other.
//
// The suite is pure CommonMark, so GFM must be off: strikethrough changes
// the expected output for several examples.
//
// Run just this file's summary with:
//
//	cargo test --test commonmark_test -- --nocapture

mod common;

use std::collections::BTreeMap;

use common::{load_spec_cases, CM_OPTS};
use tabnas_markdown::{parse_document, to_html, Options};

#[derive(Default)]
struct Tally {
    pass: usize,
    total: usize,
}

#[test]
fn commonmark_spec() {
    let cases = load_spec_cases();

    let mut by_section: BTreeMap<String, Tally> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut failures = Vec::new();

    for c in &cases {
        if !by_section.contains_key(&c.section) {
            order.push(c.section.clone());
        }
        let tally = by_section.entry(c.section.clone()).or_default();
        tally.total += 1;

        let actual = to_html(&c.markdown, &CM_OPTS);
        if actual == c.html {
            tally.pass += 1;
            continue;
        }
        failures.push(format!(
            "example {} [{}]\n  markdown: {:?}\n  expected: {:?}\n  actual:   {:?}",
            c.example, c.section, c.markdown, c.html, actual
        ));
    }

    let mut total = 0;
    let mut passed = 0;
    order.sort_by(|a, b| {
        let ta = &by_section[a];
        let tb = &by_section[b];
        let ra = ta.pass as f64 / ta.total as f64;
        let rb = tb.pass as f64 / tb.total as f64;
        ra.partial_cmp(&rb).expect("ratios are finite")
    });
    for name in &order {
        let tally = &by_section[name];
        total += tally.total;
        passed += tally.pass;
        let mark = if tally.pass == tally.total {
            "OK "
        } else {
            "   "
        };
        println!("  {mark} {name:<40} {:>3}/{:<3}", tally.pass, tally.total);
    }
    println!(
        "  TOTAL {passed}/{total}  {:.2}%",
        100.0 * passed as f64 / total as f64
    );

    assert!(
        failures.is_empty(),
        "{} of {} examples failed:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
    assert_eq!(passed, 652, "the full suite must pass");
}

/// The combinations the corpus never exercises: it only ever runs
/// `{gfm: false, breaks: false}`. Every other combination must at least
/// parse and render without panicking.
#[test]
fn commonmark_option_matrix() {
    let cases = load_spec_cases();
    for gfm in [true, false] {
        for breaks in [true, false] {
            let opts = Options { gfm, breaks };
            for c in &cases {
                let outcome = std::panic::catch_unwind(|| {
                    let _ = to_html(&c.markdown, &opts);
                    let _ = parse_document(&c.markdown, &opts);
                });
                assert!(
                    outcome.is_ok(),
                    "gfm={gfm}/breaks={breaks}: example {} panicked",
                    c.example
                );
            }
        }
    }
}
