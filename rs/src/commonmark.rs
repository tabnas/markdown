/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Engine-free entry point for the CommonMark parser. Port of
//! `ts/src/commonmark.ts` and `go/commonmark.go`.
//!
//! Nothing reachable from here imports the tabnas engine. That keeps the
//! parser testable (and the conformance suite runnable) without the engine
//! present, exactly as in the other two runtimes. The plugin wiring lives
//! in `lib.rs`, `engine_block.rs` and `engine_inline.rs`, which import
//! this; never the other way round.

use crate::block::parse_blocks;
use crate::inline::{parse_inlines, parse_inlines_keeping_references};
use crate::node::Tree;
use crate::options::Options;

/// The full two-phase parse (spec Appendix A): block structure first, then
/// inlines over the accumulated string content of each leaf, using the
/// link reference map the block phase collected.
pub fn parse(input: &str, options: Options) -> Tree {
    let (mut tree, refmap) = parse_blocks(input, options);
    parse_inlines(&mut tree, refmap, options);
    tree
}

/// [`parse`] with the inline phase reading every bracketed label a
/// definition could name as a reference, left unresolved, and the link
/// reference map not consulted: the tree the AST is projected from (see
/// `ast.rs`). [`parse`] resolves references as CommonMark says, which is
/// the tree the HTML renderer needs.
pub fn parse_keeping_references(input: &str, options: Options) -> Tree {
    let (mut tree, _) = parse_blocks(input, options);
    parse_inlines_keeping_references(&mut tree, options);
    tree
}
