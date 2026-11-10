// A parse from a reader (`Tabnas::parse_reader`, admin ADR-30).
//
// `make()` declares `lex.window`: the one matcher cuts a line through the
// lexer's window and fills it past the line's end, and nothing else reads
// the source, so a reader is read a window at a time. Every row of the
// shared fixtures and every document of both conformance corpora parses
// the same from a reader, handed over a few bytes at a time and under
// windows whose margin and block are a few characters
// (`parse_reader_with_sizes`, which makes the matcher meet the window's
// edge in every line), as from its text: the same value, member order
// included. So do long documents whose lines, code blocks and tables are
// longer than the window's margin, and a long document from a reader
// parses in time linear in its length. ts/test/reader.test.ts and
// go/reader_test.go hold the other runtimes to the same.

mod common;

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use tabnas::{Tabnas, WindowSizes};
use tabnas_markdown::Options;
use tabnas_support::{find_spec_dir, load_spec_dir, SpecOptions};

/// The pieces a reader hands its bytes over in.
const PIECES: [usize; 3] = [1, 3, 64];

/// Windows whose margin and block are a few characters.
const WINDOWS: [(usize, usize); 3] = [(1, 1), (3, 2), (64, 64)];

/// A reader that hands its bytes over at most `size` at a time.
struct Pieces<'a> {
    bytes: &'a [u8],
    at: usize,
    size: usize,
}

impl Read for Pieces<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.size.min(buf.len()).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

fn pieces(bytes: &[u8], size: usize) -> Pieces<'_> {
    Pieces { bytes, at: 0, size }
}

/// A parse's value as JSON text, members in their order, or its failure.
fn outcome(parsed: Result<tabnas::Value, tabnas::TabnasError>) -> String {
    match parsed {
        Ok(value) => format!("ok {value}"),
        Err(e) => format!("err {} at {} {}:{}", e.code, e.pos, e.row, e.col),
    }
}

/// Every way this test reads `bytes` from a reader, each with its outcome:
/// in every size of pieces under the window's own sizes, and under every
/// small window.
fn readings(parser: &Tabnas, bytes: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for size in PIECES {
        out.push((
            format!("pieces of {size}"),
            outcome(parser.parse_reader(pieces(bytes, size))),
        ));
        for (margin, block) in WINDOWS {
            let sizes = WindowSizes {
                margin,
                block,
                chunk: size,
            };
            out.push((
                format!("pieces of {size}, margin {margin}, block {block}"),
                outcome(parser.parse_reader_with_sizes(pieces(bytes, size), sizes)),
            ));
        }
    }
    out
}

#[test]
fn make_reads_a_reader_a_window_at_a_time() {
    assert!(tabnas_markdown::make().options.lex.window);
    assert!(
        tabnas_markdown::make_with(&Options::COMMONMARK)
            .options
            .lex
            .window
    );
    // The plugin leaves the window to the parser that installs it.
    let mut tn = Tabnas::new();
    tn.use_plugin(tabnas_markdown::plugin(), None)
        .expect("the plugin installs");
    assert!(!tn.options.lex.window);
}

#[test]
fn every_row_parses_the_same_from_a_reader() {
    let dir = find_spec_dir(Some(Path::new(env!("CARGO_MANIFEST_DIR"))))
        .expect("test/spec is found above rs/");
    let files = load_spec_dir(&dir, &SpecOptions::default()).expect("the fixtures load");
    let mut compared = 0;
    for file in &files {
        for row in &file.rows {
            let opts = row.named("opts");
            let options = if opts.trim().is_empty() {
                Options::default()
            } else {
                Options::resolve_json(&serde_json::from_str(opts).expect("opts are JSON"))
            };
            let parser = tabnas_markdown::make_with(&options);
            let input = row.unesc_named("input");
            let whole = outcome(parser.parse(&input));
            for (how, read) in readings(&parser, input.as_bytes()) {
                assert_eq!(read, whole, "{} {} in {how}", file.file, row.location());
                compared += 1;
            }
        }
    }
    assert!(compared > 1000, "only {compared} parses compared");
}

#[test]
fn every_corpus_document_parses_the_same_from_a_reader() {
    for (name, cases, options) in [
        ("commonmark", common::load_spec_cases(), Options::COMMONMARK),
        ("gfm", common::load_gfm_cases(), Options::default()),
    ] {
        let parser = tabnas_markdown::make_with(&options);
        let total = cases.len();
        for (i, c) in cases.iter().enumerate() {
            let whole = outcome(parser.parse(&c.markdown));
            for (how, read) in readings(&parser, c.markdown.as_bytes()) {
                assert_eq!(read, whole, "{name} example {} in {how}", c.example);
            }
            if (i + 1) % 200 == 0 || i + 1 == total {
                println!(
                    "reader {name}: {} of {total} ({}%)",
                    i + 1,
                    (i + 1) * 100 / total
                );
            }
        }
    }
}

#[test]
fn line_endings_of_every_kind_at_the_window_edge() {
    // A `\r\n` cut between two reads, a lone `\r`, a final line with and
    // without its ending, and characters past ASCII on either side.
    let parser = tabnas_markdown::make();
    for doc in [
        "a\r\nb\r\n\r\nc",
        "a\rb\r\rc\r",
        "# é\r\n> ñ\r- €\n  𝄞\r\n",
        "\r\n\r\n",
        "\r",
        "x",
        "| a | b |\r\n| - | - |\r\n| 1 | 2 |\r",
    ] {
        let whole = outcome(parser.parse(doc));
        for (how, read) in readings(&parser, doc.as_bytes()) {
            assert_eq!(read, whole, "{doc:?} in {how}");
        }
    }
}

#[test]
fn long_lines_code_blocks_and_tables_parse_the_same_from_a_reader() {
    // Each construct longer than the window's 64 KiB margin, so the window
    // grows to hold a line, and a code block and a table of many lines.
    let parser = tabnas_markdown::make();
    let long = format!("{}*x* [r]", "ab é€𝄞 cd ".repeat(9000));
    let doc = format!(
        "# {long}\n\n{long}\n{long}\n\n```\n{code}```\n\n| a | b |\n| - | - |\n{rows}\n> {long}\n> - {long}\n\n[r]: /u\n",
        code = format!("{long}\n").repeat(3),
        rows = format!("| {long} | c |\n").repeat(3),
    );
    let whole = outcome(parser.parse(&doc));
    assert!(whole.starts_with("ok "));
    for size in [4096, 64 * 1024 + 7] {
        assert_eq!(
            outcome(parser.parse_reader(pieces(doc.as_bytes(), size))),
            whole,
            "pieces of {size}"
        );
    }
    let sizes = WindowSizes {
        margin: 16,
        block: 16,
        chunk: 7,
    };
    assert_eq!(
        outcome(parser.parse_reader_with_sizes(pieces(doc.as_bytes(), 5), sizes)),
        whole,
        "a small window"
    );
}

#[test]
fn a_long_document_from_a_reader_parses_in_time_linear_in_its_length() {
    let unit = "# Heading\n\nSome *text* and a [ref] with `code`.\n\n\
                - a\n- b\n  > quoted\n\n\
                | x | y |\n| - | - |\n| 1 | 2 |\n\n\
                ```js\nlet x = 1\n```\n\n[ref]: /u \"t\"\n\n";
    let parser = tabnas_markdown::make();
    let time = |n: usize| -> Duration {
        let doc = unit.repeat(n);
        let start = Instant::now();
        parser
            .parse_reader(pieces(doc.as_bytes(), 4096))
            .expect("parses");
        start.elapsed()
    };
    time(10); // warm
    let small = time(100);
    let large = time(1000);
    println!("reader: 100 units {small:?}, 1000 units {large:?}");
    // Ten times the document, about ten times the time; well under the
    // hundred a quadratic parse would take.
    assert!(
        large < small * 30,
        "100 units {small:?}, 1000 units {large:?}"
    );
}
