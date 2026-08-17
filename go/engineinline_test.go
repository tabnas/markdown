// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasmarkdown

// engineinline_test.go — pins for the engine inline driver's structural
// contract. Mirrors ts/test/engine-inline.test.ts, per the runtime-alignment
// rule: the invariant is enforced independently in each runtime, so a
// Go-only change cannot erode it while the parity suites stay green.

import (
	"reflect"
	"testing"
)

func TestInlineRuleLookahead(t *testing.T) {
	// The inline matchers apply their one-token effects at lex time, because
	// the engine's rule loop reads lookahead tokens before earlier alts'
	// actions run. That is only sound while no alt asks the lexer to run
	// further ahead than one token — an alt with deeper lookahead would have
	// the `]` matcher consult a bracket stack whose earlier effects had not
	// been applied yet. Pin sN <= 1 so the invariant cannot erode silently.
	j := makeInlineTn(ResolveOptions(nil))

	found := false
	for _, rs := range j.Rules() {
		if rs.Name != "inline" {
			continue
		}
		found = true
		for _, alt := range rs.OpenAlts() {
			if len(alt.S) > 1 {
				t.Errorf("inline open alt lookahead must be <= 1, got %d", len(alt.S))
			}
		}
		for _, alt := range rs.CloseAlts() {
			if len(alt.S) > 1 {
				t.Errorf("inline close alt lookahead must be <= 1, got %d", len(alt.S))
			}
		}
	}
	if !found {
		t.Fatal("inline rule not found")
	}
}

func TestInlineTokenAlphabet(t *testing.T) {
	want := []string{
		"#IBK", "#IES", "#ICS", "#IDL", "#IOB", "#IBG",
		"#ICB", "#IAL", "#IHT", "#IEN", "#ITX", "#ILI",
	}
	if !reflect.DeepEqual(inlineTokenNames, want) {
		t.Errorf("inline token alphabet drifted: %v", inlineTokenNames)
	}
}
