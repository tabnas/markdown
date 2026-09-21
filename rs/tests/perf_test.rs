// Same-run ratios, never wall-clock budgets: both sides scale together on
// a slow CI box, so the checks are machine-independent. Mirrors
// go/perf_test.go.

use std::time::Instant;

use tabnas::Tabnas;
use tabnas_markdown::make;

/// A fresh engine instance with the Markdown plugin installed. Building
/// the engine and installing the grammar is the expensive part; a parse
/// of a tiny document is comparatively cheap.
fn make_markdown_parser() -> Tabnas {
    make()
}

/// Guards against a performance regression in how the Markdown plugin is
/// consumed. The right (fast) pattern is to build the instance ONCE and
/// reuse it; the wrong (slow) pattern rebuilds the engine and installs
/// the grammar on every parse, which is dominated by construction and is
/// many times slower.
///
/// This compares N parses that rebuild the instance each call against N
/// parses reusing ONE instance, on the SAME machine in the SAME run.
/// Reuse must stay close to linear relative to the rebuild path (here
/// reuse is far faster), so the rebuild-per-parse anti-pattern would blow
/// the ratio. There is deliberately NO wall-clock budget.
#[test]
fn parse_reuses_instance() {
    const SRC: &str = "# Title\n\npara with *em* and `code`\n\n- item one\n- item two";
    const N: usize = 300;

    // Warm both paths so the comparison is steady-state.
    for _ in 0..20 {
        let _ = make_markdown_parser().parse(SRC);
    }
    let reused = make_markdown_parser();
    for _ in 0..20 {
        let _ = reused.parse(SRC);
    }

    // Rebuild-per-parse: pays construction on every iteration.
    let t0 = Instant::now();
    for _ in 0..N {
        make_markdown_parser().parse(SRC).expect("rebuild parse");
    }
    let rebuild = t0.elapsed();

    // Reuse one instance: only pays parsing per iteration.
    let t1 = Instant::now();
    for _ in 0..N {
        reused.parse(SRC).expect("reuse parse");
    }
    let reuse = t1.elapsed();

    // Reusing one instance must be no slower than rebuilding it every
    // parse. Allow 4x slack for scheduling noise around an otherwise
    // lopsided win (reuse should be much faster).
    assert!(
        reuse <= rebuild * 4,
        "reusing one Markdown instance is not faster than rebuilding it per parse: \
         {N} reuse parses took {reuse:?} vs {rebuild:?} rebuilding each time \
         (ratio {:.1}x, limit 4x). Build the instance once and reuse it.",
        reuse.as_secs_f64() / rebuild.as_secs_f64()
    );
    println!(
        "rebuild-per-parse={rebuild:?}  reuse-one={reuse:?}  rebuild/reuse={:.2}x",
        rebuild.as_secs_f64() / reuse.as_secs_f64()
    );
}

/// Pins the engine block driver's linearity: the plugin path lexes one
/// #LB token per line and feeds it to the shared `incorporate_line`, so
/// doubling the document must roughly double the parse time. A regression
/// to per-token re-parsing blows this ratio immediately. Linear is ~2x,
/// quadratic is ~4x on the doubling alone and far worse in practice.
#[test]
fn engine_block_linearity() {
    let tn = make_markdown_parser();

    let mk = |n: usize| "para *em* `c` line\n\n- item [x] here\n> quote | cell\n\n".repeat(n);
    let (small, large) = (mk(400), mk(800));

    // Warm both sizes so the comparison is steady-state.
    for _ in 0..3 {
        tn.parse(&small).expect("parses");
        tn.parse(&large).expect("parses");
    }

    const ITERS: usize = 5;
    let t0 = Instant::now();
    for _ in 0..ITERS {
        tn.parse(&small).expect("parses");
    }
    let d_small = t0.elapsed();

    let t1 = Instant::now();
    for _ in 0..ITERS {
        tn.parse(&large).expect("parses");
    }
    let d_large = t1.elapsed();

    let ratio = d_large.as_secs_f64() / d_small.as_secs_f64();
    assert!(
        ratio <= 6.0,
        "engine block path is not linear: doubling the document made the parse {ratio:.1}x slower (limit 6x)"
    );
    println!("small={d_small:?} large={d_large:?} large/small={ratio:.2}x");
}
