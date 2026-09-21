// Pins for the engine inline driver's structural contract. Mirrors
// ts/test/engine-inline.test.ts and go/engineinline_test.go, per the
// runtime-alignment rule: the invariant is enforced independently in each
// runtime, so a Rust-only change cannot erode it while the parity suites
// stay green.

use tabnas_markdown::engine_inline::{make_inline_tn, INLINE_TOKENS};
use tabnas_markdown::Options;

/// The inline matchers apply their one-token effects at lex time, because
/// the engine's rule loop reads lookahead tokens before earlier alts'
/// actions run. That is only sound while no alt asks the lexer to run
/// further ahead than one token: an alt with deeper lookahead would have
/// the `]` matcher consult a bracket stack whose earlier effects had not
/// been applied yet. Pin `s.len() <= 1` so the invariant cannot erode
/// silently.
#[test]
fn inline_rule_lookahead() {
    let tn = make_inline_tn(&Options::default());
    let inline = tn
        .rule_specs()
        .into_iter()
        .find(|rs| rs.name == "inline")
        .expect("inline rule not found");
    for alt in &inline.open {
        assert!(
            alt.s.len() <= 1,
            "inline open alt lookahead must be <= 1, got {}",
            alt.s.len()
        );
    }
    for alt in &inline.close {
        assert!(
            alt.s.len() <= 1,
            "inline close alt lookahead must be <= 1, got {}",
            alt.s.len()
        );
    }
}

#[test]
fn inline_token_alphabet() {
    let want = [
        "#IBK", "#IES", "#ICS", "#IDL", "#IOB", "#IBG", "#ICB", "#IAL", "#IHT", "#IEN", "#ITX",
        "#ILI",
    ];
    assert_eq!(INLINE_TOKENS, want, "inline token alphabet drifted");
}

/// The option reshapes the registered matcher set itself: the base
/// dialect's lexer simply has no tilde matcher.
#[test]
fn tilde_matcher_exists_only_with_gfm() {
    let with = make_inline_tn(&Options::default());
    assert!(with.options.lex.matchers.contains_key("inlTilde"));
    let without = make_inline_tn(&Options::COMMONMARK);
    assert!(!without.options.lex.matchers.contains_key("inlTilde"));

    // Registration order is precedence, so the set is sorted by order.
    let orders: Vec<f64> = with
        .options
        .lex
        .matchers
        .values()
        .map(|m| m.order)
        .collect();
    assert!(
        orders.windows(2).all(|w| w[0] < w[1]),
        "matchers are registered in ascending order: {orders:?}"
    );
    assert_eq!(with.options.lex.matchers.len(), 13);
    assert_eq!(without.options.lex.matchers.len(), 12);
}
