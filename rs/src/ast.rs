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
//! The tree keeps references unresolved, as mdast does: a bracketed label
//! is a `linkReference` or an `imageReference` naming it, whether or not the
//! document defines it, and each link reference definition is a
//! `definition` node where it stands. A block's tree then depends on
//! nothing outside the block, so it is complete as the block closes (the
//! engine path writes it then; see `engine_block.rs`).
//! [`resolve_references`] gives back the tree with the references
//! resolved, for a caller that wants links. A list's and a list item's
//! `spread`, and an item's `checked`, follow its `children`, since they are
//! known only once the children are.
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

use crate::common::{js_space_index, js_trim, normalize_reference, unescape_string};
use crate::node::{Definition, ListType, NodeId, NodeType, Tree};
use crate::options::Options;

/// A label's identifier, as mdast's: normalized as CommonMark matches
/// labels ([`normalize_reference`]), in lower case, as the canonical
/// runtime's `toLowerCase` writes it (Rust's full mapping is the same one,
/// final sigma included). Two labels a definition and a reference would
/// match on have the same identifier.
pub fn label_identifier(raw_label: &str) -> String {
    let key = normalize_reference(raw_label);
    if key.is_ascii() {
        key.to_ascii_lowercase()
    } else {
        key.to_lowercase()
    }
}

fn reference_type_value(tree: &Tree, node: NodeId) -> Value {
    string(
        tree.get(node)
            .reference_type
            .map_or("shortcut", |t| t.as_str()),
    )
}

/// An object value with these members, in this order.
pub(crate) fn object(entries: Vec<(&'static str, Value)>) -> Value {
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
/// The children are the AST's, definitions among them, each by the lines
/// it spans.
pub(crate) fn item_is_spread(tree: &Tree, item: NodeId) -> bool {
    let mut prev_end: Option<usize> = None;
    for child in block_sequence(tree, item) {
        let (start, end) = match child {
            BlockChild::Definition(def) => (def.start, def.end),
            BlockChild::Node(id) => {
                let pos = tree.get(id).sourcepos;
                (pos[0][0], pos[1][0])
            }
        };
        if prev_end.is_some_and(|e| start > e + 1) {
            return true;
        }
        prev_end = Some(end);
    }
    false
}

/// One of a container's children in the AST's order.
pub(crate) enum BlockChild<'t> {
    Node(NodeId),
    Definition(&'t Definition),
}

/// A container's children in the AST's order: each child, and each
/// definition where it stands among them (see `DefinitionSite`).
pub(crate) fn block_sequence(tree: &Tree, container: NodeId) -> Vec<BlockChild<'_>> {
    let defs = &tree.get(container).definitions;
    let mut out = Vec::new();
    let mut d = 0;
    while d < defs.len() && defs[d].after.is_none() {
        out.push(BlockChild::Definition(&defs[d].def));
        d += 1;
    }
    let mut c = tree.get(container).first_child;
    while let Some(id) = c {
        out.push(BlockChild::Node(id));
        while d < defs.len() && defs[d].after == Some(id) {
            out.push(BlockChild::Definition(&defs[d].def));
            d += 1;
        }
        c = tree.get(id).next;
    }
    out
}

/// A definition's node.
pub fn definition_node(def: &Definition) -> Value {
    object(vec![
        ("type", string("definition")),
        ("identifier", Value::String(label_identifier(&def.label))),
        ("label", Value::String(unescape_string(&def.label))),
        ("url", string(&def.destination)),
        (
            "title",
            nullable_string(&def.title.clone().filter(|t| !t.is_empty())),
        ),
    ])
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
pub fn inline_children(tree: &Tree, node: NodeId, opts: &Options) -> Vec<Value> {
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
                NodeType::LinkReference => object(vec![
                    ("type", string("linkReference")),
                    (
                        "identifier",
                        Value::String(label_identifier(&container_node.label)),
                    ),
                    (
                        "label",
                        Value::String(unescape_string(&container_node.label)),
                    ),
                    ("referenceType", reference_type_value(tree, container)),
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

            NodeType::Emph
            | NodeType::Strong
            | NodeType::Del
            | NodeType::Link
            | NodeType::LinkReference => {
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

            NodeType::ImageReference => frame.push(object(vec![
                ("type", string("imageReference")),
                ("identifier", Value::String(label_identifier(&child.label))),
                ("label", Value::String(unescape_string(&child.label))),
                ("referenceType", reference_type_value(tree, child_id)),
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

/// The fields of a container block's node known when it opens, `type`
/// first, in the order the node has them: what precedes its `children`.
/// Its fields known only when it closes are [`block_tail`]'s. Mirrors
/// `ts/src/ast.ts` `blockHead`.
pub fn block_head(tree: &Tree, node: NodeId) -> Vec<(&'static str, Value)> {
    let n = tree.get(node);
    match n.node_type {
        NodeType::BlockQuote => vec![("type", string("blockquote"))],
        NodeType::List => {
            let data = n.list_data.as_ref();
            let ordered = data.is_some_and(|d| d.list_type == ListType::Ordered);
            let start = if ordered {
                Value::Number(data.map_or(1, |d| d.start) as f64)
            } else {
                Value::Null
            };
            vec![
                ("type", string("list")),
                ("ordered", Value::Bool(ordered)),
                ("start", start),
            ]
        }
        NodeType::Item => vec![("type", string("listItem"))],
        NodeType::Paragraph => vec![("type", string("paragraph"))],
        NodeType::Heading => vec![
            ("type", string("heading")),
            ("depth", Value::Number(n.level as f64)),
        ],
        NodeType::Table => {
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
            vec![("type", string("table")), ("align", Value::array(align))]
        }
        NodeType::TableRow => vec![("type", string("tableRow"))],
        NodeType::TableCell => vec![("type", string("tableCell"))],
        _ => Vec::new(),
    }
}

/// The fields of a container block's node that follow its `children`.
pub fn block_tail(tree: &Tree, node: NodeId) -> Vec<(&'static str, Value)> {
    let n = tree.get(node);
    match n.node_type {
        NodeType::List => {
            let tight = n.list_data.as_ref().is_none_or(|d| d.tight);
            vec![("spread", Value::Bool(!tight))]
        }
        NodeType::Item => {
            let checked = match n.checked {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            };
            vec![
                ("spread", Value::Bool(item_is_spread(tree, node))),
                ("checked", checked),
            ]
        }
        _ => Vec::new(),
    }
}

/// A leaf block's node: a code block, an HTML block or a thematic break.
pub fn leaf_block(tree: &Tree, node: NodeId) -> Option<Value> {
    let n = tree.get(node);
    match n.node_type {
        NodeType::ThematicBreak => Some(object(vec![("type", string("thematicBreak"))])),

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

        _ => None,
    }
}

/// A container node of the AST: its head, its children, then its tail.
fn container_node(tree: &Tree, node: NodeId, children: Vec<Value>) -> Value {
    let mut entries = block_head(tree, node);
    entries.push(("children", Value::array(children)));
    entries.extend(block_tail(tree, node));
    object(entries)
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
        NodeType::Paragraph | NodeType::Heading => Some(container_node(
            tree,
            node,
            inline_children(tree, node, opts),
        )),

        NodeType::ThematicBreak | NodeType::CodeBlock | NodeType::HtmlBlock => {
            leaf_block(tree, node)
        }

        NodeType::BlockQuote | NodeType::Item => {
            // An item is only reached here if it is somehow detached from
            // its list; a list builds its own items below.
            let children = built_children(tree, node, built);
            Some(container_node(tree, node, children))
        }

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
                    cells.push(container_node(tree, c, inline_children(tree, c, opts)));
                }
                rows.push(container_node(tree, r, cells));
            }
            Some(container_node(tree, node, rows))
        }

        NodeType::List => {
            // The items were built before the list (children precede
            // their parent in the build order) and each already took its
            // own children out of `built`; take the items the same way.
            let items = built_children(tree, node, built);
            Some(container_node(tree, node, items))
        }

        _ => None,
    }
}

/// The already-built projections of `node`'s children, in source order,
/// with the definitions among them, taken out of `built`.
fn built_children(tree: &Tree, node: NodeId, built: &mut [Option<Value>]) -> Vec<Value> {
    let mut out = Vec::new();
    for child in block_sequence(tree, node) {
        match child {
            BlockChild::Definition(def) => out.push(definition_node(def)),
            BlockChild::Node(id) => {
                if let Some(value) = built[id].take() {
                    out.push(value);
                }
            }
        }
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

// --- resolving references -------------------------------------------------

/// What [`resolve_references`] reads a reference's destination from.
struct Resource {
    url: Value,
    title: Value,
}

fn get<'v>(node: &'v Value, key: &str) -> Option<&'v Value> {
    match node {
        Value::Object(map) => map.get(key),
        _ => None,
    }
}

fn get_str<'v>(node: &'v Value, key: &str) -> &'v str {
    match get(node, key) {
        Some(Value::String(s)) => s,
        _ => "",
    }
}

/// The tree with its references resolved, as CommonMark reads a document:
/// each `linkReference` and `imageReference` whose identifier a definition
/// has becomes the `link` or the `image` that definition gives (the first
/// definition of an identifier wins), one no definition has becomes its
/// source text again (`[`, its text, `]`, then `[label]` for a full
/// reference and `[]` for a collapsed one, as mdast-util-to-hast reverts
/// one), and the definitions leave the tree. Adjacent text is merged, as
/// the parse merges it. A new tree; the one given is not changed. Mirrors
/// `ts/src/ast.ts` `resolveReferences`.
///
/// The parse reads a bracketed label as a reference whether or not the
/// document defines it, so where one with no definition decided how the
/// text around it nests (emphasis that would have crossed it, a link whose
/// text holds it), the result is the tree that reading gives, not the one
/// CommonMark gives: the loss the manifest declares. Every other document
/// resolves to the tree CommonMark reads.
///
/// Iterative over the containers, since block nesting is the input's to
/// choose.
///
/// ```
/// let doc = tabnas_markdown::parse("[a]\n\n[a]: /u")?;
/// let resolved = tabnas_markdown::resolve_references(&doc).to_json();
/// assert_eq!(resolved["children"][0]["children"][0]["type"], "link");
/// assert_eq!(resolved["children"][0]["children"][0]["url"], "/u");
/// # Ok::<(), tabnas_markdown::MarkdownError>(())
/// ```
pub fn resolve_references(doc: &Value) -> Value {
    let mut defs: std::collections::HashMap<String, Resource> = std::collections::HashMap::new();
    let mut stack: Vec<&Value> = vec![doc];
    while let Some(node) = stack.pop() {
        if get_str(node, "type") == "definition" {
            defs.entry(get_str(node, "identifier").to_string())
                .or_insert_with(|| Resource {
                    url: get(node, "url").cloned().unwrap_or(Value::Null),
                    title: get(node, "title").cloned().unwrap_or(Value::Null),
                });
            continue;
        }
        if let Some(Value::Array(children)) = get(node, "children") {
            for c in children.iter().rev() {
                stack.push(c);
            }
        }
    }

    struct Frame<'v> {
        node: &'v Value,
        children: &'v [Value],
        i: usize,
        out: Vec<Value>,
    }
    fn push(out: &mut Vec<Value>, node: Value) {
        if get_str(&node, "type") == "text" {
            if let Some(last) = out.last_mut() {
                if get_str(last, "type") == "text" {
                    let value = format!("{}{}", get_str(last, "value"), get_str(&node, "value"));
                    *last = object(vec![
                        ("type", string("text")),
                        ("value", Value::String(value)),
                    ]);
                    return;
                }
            }
        }
        out.push(node);
    }
    fn text(out: &mut Vec<Value>, value: String) {
        push(
            out,
            object(vec![
                ("type", string("text")),
                ("value", Value::String(value)),
            ]),
        );
    }
    fn suffix(node: &Value) -> String {
        match get_str(node, "referenceType") {
            "full" => format!("[{}]", get_str(node, "label")),
            "collapsed" => "[]".to_string(),
            _ => String::new(),
        }
    }
    fn children_of(node: &Value) -> &[Value] {
        match get(node, "children") {
            Some(Value::Array(items)) => items,
            _ => &[],
        }
    }
    fn rebuilt(node: &Value, children: Vec<Value>) -> Value {
        let mut map = IndexMap::new();
        if let Value::Object(fields) = node {
            for (k, v) in fields.iter() {
                // `children` keeps its place, its value set below.
                map.insert(
                    k.clone(),
                    if k == "children" {
                        Value::Null
                    } else {
                        v.clone()
                    },
                );
            }
        }
        map.insert("children".to_string(), Value::array(children));
        Value::object(map)
    }

    let mut stack = vec![Frame {
        node: doc,
        children: children_of(doc),
        i: 0,
        out: Vec::new(),
    }];
    loop {
        let frame = stack
            .last_mut()
            .expect("the document's frame is popped last");
        if frame.i >= frame.children.len() {
            let done = stack.pop().expect("a frame to pop");
            let node = done.node;
            let Some(parent) = stack.last_mut() else {
                return rebuilt(node, done.out);
            };
            if get_str(node, "type") == "linkReference" {
                match defs.get(get_str(node, "identifier")) {
                    Some(res) => push(
                        &mut parent.out,
                        object(vec![
                            ("type", string("link")),
                            ("url", res.url.clone()),
                            ("title", res.title.clone()),
                            ("children", Value::array(done.out)),
                        ]),
                    ),
                    None => {
                        text(&mut parent.out, "[".to_string());
                        for c in done.out {
                            push(&mut parent.out, c);
                        }
                        text(&mut parent.out, format!("]{}", suffix(node)));
                    }
                }
            } else {
                let value = rebuilt(node, done.out);
                push(&mut parent.out, value);
            }
            continue;
        }
        let child = &frame.children[frame.i];
        frame.i += 1;
        match get_str(child, "type") {
            "definition" => {}
            "imageReference" => {
                let alt = get_str(child, "alt").to_string();
                match defs.get(get_str(child, "identifier")) {
                    Some(res) => push(
                        &mut frame.out,
                        object(vec![
                            ("type", string("image")),
                            ("url", res.url.clone()),
                            ("title", res.title.clone()),
                            ("alt", Value::String(alt)),
                        ]),
                    ),
                    None => text(&mut frame.out, format!("![{}]{}", alt, suffix(child))),
                }
            }
            _ => {
                if let Some(Value::Array(items)) = get(child, "children") {
                    stack.push(Frame {
                        node: child,
                        children: items,
                        i: 0,
                        out: Vec::new(),
                    });
                } else {
                    push(&mut frame.out, child.clone());
                }
            }
        }
    }
}
