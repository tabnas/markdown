/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Projection from the native CommonMark tree to the mdast-adjacent JSON
//! AST this package has always returned. Port of `ts/src/ast.ts` and
//! `go/ast.go`; the three must agree node for node, since
//! `test/spec/*.tsv` is the parity contract between them.
//!
//! The native tree is what the HTML renderer walks, and it carries things
//! the public AST has no room for (source positions, the block / inline
//! distinction for raw HTML, container open / closed state). This module is
//! the one place that decides what survives the crossing. Two deliberate
//! choices:
//!
//! - Soft breaks collapse into a single space inside the surrounding text
//!   run, which is the long-standing documented behaviour of this package
//!   (`breaks: true` promotes them to `break` nodes instead). The native
//!   tree keeps them as real `softbreak` nodes, so an HTML render is
//!   unaffected.
//! - `spread` follows mdast semantics: a list is spread when it is loose,
//!   an item when it holds blank-line-separated blocks.
//!
//! The result is a [`tabnas::Value`], which is what the plugin's parse
//! returns, so the engine path and the engine-free path produce one type.
//! Every children array is a real (possibly empty) array, never `null`.
//!
//! Both projections are iterative rather than recursive, because nesting
//! depth is chosen by the input: `"> ".repeat(20000)` is 40 KB of Markdown
//! the block parser and the renderer both handle, and `"*".repeat(5000)`
//! around a word nests emphasis 2500 deep.

use indexmap::IndexMap;
use tabnas::Value;

use crate::common::{js_space_index, js_trim};
use crate::node::{ListType, NodeId, NodeType, Tree};
use crate::options::Options;

fn object(entries: Vec<(&'static str, Value)>) -> Value {
    let mut map = IndexMap::with_capacity(entries.len());
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::object(map)
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

fn nullable_string(s: &Option<String>) -> Value {
    match s {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

/// Flatten an image's inline children to plain text for `alt`. Iterative
/// over an explicit stack, for the reason the module doc gives.
fn collect_alt_text(tree: &Tree, node: NodeId) -> String {
    let mut out = String::new();
    let mut stack: Vec<NodeId> = Vec::new();
    let mut c = tree.get(node).last_child;
    while let Some(id) = c {
        stack.push(id);
        c = tree.get(id).prev;
    }
    while let Some(id) = stack.pop() {
        let child = tree.get(id);
        match child.node_type {
            NodeType::Text | NodeType::Code | NodeType::HtmlInline => out.push_str(&child.literal),
            NodeType::Softbreak | NodeType::Linebreak => out.push('\n'),
            _ => {
                let mut c = child.last_child;
                while let Some(g) = c {
                    stack.push(g);
                    c = tree.get(g).prev;
                }
            }
        }
    }
    out
}

/// mdast semantics: an item is spread when two of its children are
/// separated by a *blank line*, not merely when it has more than one
/// child. `- a\n  - b` holds a paragraph and a nested list with no blank
/// line between them and is not spread; `- a\n\n  - b` is.
///
/// Derived from the block parser's sourcepos rather than tracked
/// separately: consecutive children whose line ranges are not adjacent
/// must have had a blank line between them.
fn item_is_spread(tree: &Tree, item: NodeId) -> bool {
    let mut child = tree.get(item).first_child;
    while let Some(c) = child {
        let Some(next) = tree.get(c).next else {
            break;
        };
        let end_line = tree.get(c).sourcepos[1][0];
        let next_start_line = tree.get(next).sourcepos[0][0];
        if next_start_line > end_line + 1 {
            return true;
        }
        child = Some(next);
    }
    false
}

/// One container's inline children under construction.
struct InlineFrame {
    /// The container node, and what it becomes once its children are done
    /// (`None` for the block whose children are being collected).
    node: Option<NodeId>,
    /// The next child to visit.
    cursor: Option<NodeId>,
    out: Vec<Value>,
    /// The text run in progress: adjacent text is merged into one node,
    /// and the merge accretes here rather than concatenating strings.
    acc: String,
    acc_open: bool,
}

impl InlineFrame {
    fn flush(&mut self) {
        if self.acc_open {
            self.out.push(object(vec![
                ("type", string("text")),
                ("value", Value::String(std::mem::take(&mut self.acc))),
            ]));
            self.acc_open = false;
        }
    }

    fn push_text(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        self.acc_open = true;
        self.acc.push_str(value);
    }

    fn push(&mut self, value: Value) {
        self.flush();
        self.out.push(value);
    }
}

/// Project a block's inline children.
fn inline_children(tree: &Tree, node: NodeId, opts: &Options) -> Vec<Value> {
    let mut stack = vec![InlineFrame {
        node: None,
        cursor: tree.get(node).first_child,
        out: Vec::new(),
        acc: String::new(),
        acc_open: false,
    }];

    loop {
        let frame = stack.last_mut().expect("the block's frame is never popped");
        let Some(child_id) = frame.cursor else {
            // This container's children are done.
            frame.flush();
            let done = stack.pop().expect("a frame to pop");
            let Some(container) = done.node else {
                return done.out;
            };
            let parent = stack.last_mut().expect("a container has a parent frame");
            let container_node = tree.get(container);
            let value = match container_node.node_type {
                NodeType::Emph => object(vec![
                    ("type", string("emphasis")),
                    ("children", Value::array(done.out)),
                ]),
                NodeType::Strong => object(vec![
                    ("type", string("strong")),
                    ("children", Value::array(done.out)),
                ]),
                NodeType::Del => object(vec![
                    ("type", string("delete")),
                    ("children", Value::array(done.out)),
                ]),
                _ => object(vec![
                    ("type", string("link")),
                    ("url", string(&container_node.destination)),
                    ("title", nullable_string(&container_node.title)),
                    ("children", Value::array(done.out)),
                ]),
            };
            parent.push(value);
            continue;
        };

        let child = tree.get(child_id);
        frame.cursor = child.next;

        match child.node_type {
            NodeType::Text => frame.push_text(&child.literal),

            NodeType::Softbreak => {
                // Documented behaviour: a soft break reads as a space in the
                // AST.
                if opts.breaks {
                    frame.push(object(vec![("type", string("break"))]));
                } else {
                    frame.push_text(" ");
                }
            }

            NodeType::Linebreak => frame.push(object(vec![("type", string("break"))])),

            NodeType::Code => frame.push(object(vec![
                ("type", string("inlineCode")),
                ("value", string(&child.literal)),
            ])),

            NodeType::HtmlInline => frame.push(object(vec![
                ("type", string("html")),
                ("value", string(&child.literal)),
            ])),

            NodeType::Emph | NodeType::Strong | NodeType::Del | NodeType::Link => {
                frame.flush();
                stack.push(InlineFrame {
                    node: Some(child_id),
                    cursor: child.first_child,
                    out: Vec::new(),
                    acc: String::new(),
                    acc_open: false,
                });
            }

            NodeType::Image => frame.push(object(vec![
                ("type", string("image")),
                ("url", string(&child.destination)),
                ("title", nullable_string(&child.title)),
                ("alt", Value::String(collect_alt_text(tree, child_id))),
            ])),

            // A block node cannot appear here in a well-formed tree;
            // skipping is safer than emitting a node the published types do
            // not allow.
            _ => {}
        }
    }
}

/// Containers whose children are blocks rather than inlines.
fn is_block_container(t: NodeType) -> bool {
    matches!(
        t,
        NodeType::Document | NodeType::BlockQuote | NodeType::List | NodeType::Item
    )
}

/// A projected block, or `None` for a node the AST has no shape for.
fn build_block(
    tree: &Tree,
    node: NodeId,
    opts: &Options,
    built: &mut [Option<Value>],
) -> Option<Value> {
    let n = tree.get(node);
    match n.node_type {
        NodeType::Paragraph => Some(object(vec![
            ("type", string("paragraph")),
            ("children", Value::array(inline_children(tree, node, opts))),
        ])),

        NodeType::Heading => Some(object(vec![
            ("type", string("heading")),
            ("depth", Value::Number(n.level as f64)),
            ("children", Value::array(inline_children(tree, node, opts))),
        ])),

        NodeType::ThematicBreak => Some(object(vec![("type", string("thematicBreak"))])),

        NodeType::BlockQuote => Some(object(vec![
            ("type", string("blockquote")),
            ("children", Value::array(built_children(tree, node, built))),
        ])),

        NodeType::CodeBlock => {
            // `js_trim` / `js_space_index`, not `str::trim` and an
            // ASCII-only class: the canonical runtime splits on `.trim()`
            // and `.search(/\s/)`, whose character set includes NBSP and
            // U+FEFF but excludes U+0085. The block phase already trimmed
            // the info string, but `unescape_string` runs after that trim,
            // so `&#160;` and friends put whitespace back.
            let info = js_trim(n.info.as_deref().unwrap_or(""));
            let (lang, meta) = if info.is_empty() {
                (Value::Null, Value::Null)
            } else {
                match js_space_index(info) {
                    None => (string(info), Value::Null),
                    Some((sp, width)) => {
                        let rest = js_trim(&info[sp + width..]);
                        (
                            string(&info[..sp]),
                            if rest.is_empty() {
                                Value::Null
                            } else {
                                string(rest)
                            },
                        )
                    }
                }
            };
            // The native tree keeps the trailing newline because the spec's
            // HTML for `<pre><code>` includes it; mdast's `code.value` does
            // not. Strip exactly one, not all, so a block genuinely ending
            // in a blank line keeps it.
            let value = n.literal.strip_suffix('\n').unwrap_or(&n.literal);
            Some(object(vec![
                ("type", string("code")),
                ("lang", lang),
                ("meta", meta),
                ("value", string(value)),
            ]))
        }

        NodeType::HtmlBlock => Some(object(vec![
            ("type", string("html")),
            ("value", string(&n.literal)),
        ])),

        NodeType::Table => {
            // A table's children are rows and cells rather than blocks, so
            // the block walk never descends into one; the nesting is a
            // fixed three levels deep and cannot be driven past the stack
            // by input.
            let mut rows = Vec::new();
            let mut row = n.first_child;
            while let Some(r) = row {
                let row_node = tree.get(r);
                row = row_node.next;
                if row_node.node_type != NodeType::TableRow {
                    continue;
                }
                let mut cells = Vec::new();
                let mut cell = row_node.first_child;
                while let Some(c) = cell {
                    let cell_node = tree.get(c);
                    cell = cell_node.next;
                    if cell_node.node_type != NodeType::TableCell {
                        continue;
                    }
                    cells.push(object(vec![
                        ("type", string("tableCell")),
                        ("children", Value::array(inline_children(tree, c, opts))),
                    ]));
                }
                rows.push(object(vec![
                    ("type", string("tableRow")),
                    ("children", Value::array(cells)),
                ]));
            }
            // A column with no colon is JSON null, and a table with no
            // columns is `[]`.
            let align: Vec<Value> = n
                .table_align
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|a| match a {
                    Some(a) => string(a.as_str()),
                    None => Value::Null,
                })
                .collect();
            Some(object(vec![
                ("type", string("table")),
                ("align", Value::array(align)),
                ("children", Value::array(rows)),
            ]))
        }

        NodeType::List => {
            let data = n.list_data.as_ref();
            let ordered = data.is_some_and(|d| d.list_type == ListType::Ordered);
            // The items were built before the list (children precede
            // their parent in the build order) and each already took its
            // own children out of `built`; take the items the same way.
            let items = built_children(tree, node, built);
            let start = if ordered {
                Value::Number(data.map_or(1, |d| d.start) as f64)
            } else {
                Value::Null
            };
            let tight = data.is_none_or(|d| d.tight);
            Some(object(vec![
                ("type", string("list")),
                ("ordered", Value::Bool(ordered)),
                ("start", start),
                ("spread", Value::Bool(!tight)),
                ("children", Value::array(items)),
            ]))
        }

        NodeType::Item => Some(list_item(tree, node, built)),

        _ => None,
    }
}

fn list_item(tree: &Tree, item: NodeId, built: &mut [Option<Value>]) -> Value {
    let checked = match tree.get(item).checked {
        Some(b) => Value::Bool(b),
        None => Value::Null,
    };
    object(vec![
        ("type", string("listItem")),
        ("spread", Value::Bool(item_is_spread(tree, item))),
        ("checked", checked),
        ("children", Value::array(built_children(tree, item, built))),
    ])
}

/// The already-built projections of `node`'s children, in source order,
/// taken out of `built`.
fn built_children(tree: &Tree, node: NodeId, built: &mut [Option<Value>]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut c = tree.get(node).first_child;
    while let Some(id) = c {
        if let Some(value) = built[id].take() {
            out.push(value);
        }
        c = tree.get(id).next;
    }
    out
}

/// Project a container's block children.
///
/// Two passes. The first walks the block tree with an explicit stack,
/// recording nodes in an order where every parent precedes its
/// descendants. Building in reverse then guarantees a node's children are
/// already built when it is reached, so `build_block` can look them up
/// instead of recursing.
fn block_children(tree: &Tree, node: NodeId, opts: &Options) -> Vec<Value> {
    let mut order: Vec<NodeId> = Vec::new();
    let mut stack: Vec<NodeId> = Vec::new();

    let mut c = tree.get(node).last_child;
    while let Some(id) = c {
        stack.push(id);
        c = tree.get(id).prev;
    }
    while let Some(n) = stack.pop() {
        order.push(n);
        if is_block_container(tree.get(n).node_type) {
            let mut c = tree.get(n).last_child;
            while let Some(id) = c {
                stack.push(id);
                c = tree.get(id).prev;
            }
        }
    }

    let mut built: Vec<Option<Value>> = vec![None; tree.len()];
    for &n in order.iter().rev() {
        let block = build_block(tree, n, opts, &mut built);
        built[n] = block;
    }

    built_children(tree, node, &mut built)
}

/// Project a parsed native tree into the public JSON AST.
pub fn to_ast(tree: &Tree, opts: &Options) -> Value {
    object(vec![
        ("type", string("document")),
        (
            "children",
            Value::array(block_children(tree, tree.root(), opts)),
        ),
    ])
}
