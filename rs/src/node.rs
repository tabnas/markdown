/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! Native CommonMark node tree, the internal parse result. Port of
//! `ts/src/node.ts` and `go/node.go`; keep the three in step.
//!
//! The canonical tree is linked (parent / firstChild / lastChild / prev /
//! next) rather than a children array, because both phases of the
//! CommonMark algorithm (spec 0.31.2 Appendix A) splice nodes mid-walk: the
//! block phase keeps a spine of open blocks and closes them from the tail,
//! and the inline phase's delimiter stack rewrites sibling runs in place.
//!
//! This port keeps exactly those links, but the nodes live in an arena
//! ([`Tree`]) and the links are indices ([`NodeId`]) rather than pointers.
//! That is the one shape difference from the other two runtimes, and it is
//! what Rust's ownership rules ask for: a node that points at its parent
//! and its siblings cannot be a plain owned value, and reference counting
//! would make every splice a borrow-checker negotiation. An arena gives
//! the same O(1) splices with plain integer bookkeeping, and an unlinked
//! node simply becomes unreachable. A driver that writes blocks as they
//! finish and keeps no tree gives a written block's slots back
//! ([`Tree::free_subtree`], [`crate::block::BlockParser::release_block`]),
//! which the next nodes fill, so a long document's arena stays as large as
//! its widest open block, not its length (tabnas/markdown#100).
//!
//! The public, mdast-adjacent AST is produced from this tree by `ast.rs`.
//! That projection is lossy on purpose (it drops raw inline HTML boundaries
//! and source positions); the tree is what `html.rs` renders for CommonMark
//! conformance.

/// The node kinds. String-valued in the other two runtimes so that debug
/// output and the AST projection read the same; [`NodeType::as_str`] is
/// the same spelling here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeType {
    // Containers.
    Document,
    BlockQuote,
    List,
    Item,
    // Leaf blocks.
    Paragraph,
    Heading,
    ThematicBreak,
    CodeBlock,
    HtmlBlock,
    // GFM tables (extension): a leaf block as far as the spec's algorithm is
    // concerned (no block-level element can be inserted in one) but a
    // container as far as the tree is concerned, since its rows and cells
    // are real nodes and its cells hold inlines.
    Table,
    TableRow,
    TableCell,
    // Inlines.
    Text,
    Softbreak,
    Linebreak,
    Code,
    HtmlInline,
    Emph,
    Strong,
    Link,
    Image,
    Del,
    // A link or an image whose destination a label names, kept unresolved:
    // an inline parse that reads references as references (the AST's, see
    // `ast.rs`) makes these where the resolving parse would look the label
    // up. The HTML renderer never sees one, since the tree it renders comes
    // from the resolving parse.
    LinkReference,
    ImageReference,
}

impl NodeType {
    /// The canonical name, as the TypeScript union and the Go constants
    /// spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Document => "document",
            NodeType::BlockQuote => "block_quote",
            NodeType::List => "list",
            NodeType::Item => "item",
            NodeType::Paragraph => "paragraph",
            NodeType::Heading => "heading",
            NodeType::ThematicBreak => "thematic_break",
            NodeType::CodeBlock => "code_block",
            NodeType::HtmlBlock => "html_block",
            NodeType::Table => "table",
            NodeType::TableRow => "table_row",
            NodeType::TableCell => "table_cell",
            NodeType::Text => "text",
            NodeType::Softbreak => "softbreak",
            NodeType::Linebreak => "linebreak",
            NodeType::Code => "code",
            NodeType::HtmlInline => "html_inline",
            NodeType::Emph => "emph",
            NodeType::Strong => "strong",
            NodeType::Link => "link",
            NodeType::Image => "image",
            NodeType::Del => "del",
            NodeType::LinkReference => "link_reference",
            NodeType::ImageReference => "image_reference",
        }
    }

    /// Containers may hold children; leaves carry only `literal`.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            NodeType::Document
                | NodeType::BlockQuote
                | NodeType::List
                | NodeType::Item
                | NodeType::Paragraph
                | NodeType::Heading
                | NodeType::Table
                | NodeType::TableRow
                | NodeType::TableCell
                | NodeType::Emph
                | NodeType::Strong
                | NodeType::Link
                | NodeType::Image
                | NodeType::Del
                | NodeType::LinkReference
                | NodeType::ImageReference
        )
    }
}

/// How a reference names its label (section 6.3): `[a][b]`, `[a][]` or
/// `[a]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceType {
    Full,
    Collapsed,
    Shortcut,
}

impl ReferenceType {
    /// The name the AST's `referenceType` carries.
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceType::Full => "full",
            ReferenceType::Collapsed => "collapsed",
            ReferenceType::Shortcut => "shortcut",
        }
    }
}

/// A link reference definition (section 4.7) as the block phase read it
/// from the front of a paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The label's raw source text, between its brackets.
    pub label: String,
    /// The destination, already entity-decoded and backslash-unescaped.
    pub destination: String,
    /// The title, likewise.
    pub title: Option<String>,
    /// The source lines the definition spans, 1-based, both included.
    pub start: usize,
    pub end: usize,
}

/// Where a definition stands among a container's children: after the
/// child `after` (`None`: before the first), in source order. Definitions
/// are not blocks in the spec's algorithm, which reads them out of a
/// paragraph and drops a paragraph that held nothing else, so they are
/// kept beside the children rather than among them: the block phase's own
/// reads of a container's children (list looseness, the blank-line flags)
/// never see one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionSite {
    pub after: Option<NodeId>,
    pub def: Definition,
}

/// Per-column alignment of a GFM table, from the colons in its delimiter
/// row: `:--` left, `--:` right, `:-:` center. A column with no colon is
/// `None`, which is the canonical runtime's `null` and the Go port's
/// `AlignNone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

impl Align {
    /// The attribute value the renderer writes and the AST carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Right => "right",
            Align::Center => "center",
        }
    }
}

/// One column's alignment; `None` for a delimiter cell with no colon.
pub type TableAlign = Option<Align>;

/// Bullet or ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListType {
    Bullet,
    Ordered,
}

impl ListType {
    pub fn as_str(self) -> &'static str {
        match self {
            ListType::Bullet => "bullet",
            ListType::Ordered => "ordered",
        }
    }
}

/// What sections 5.2 and 5.3 need to decide list identity and rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListData {
    pub list_type: ListType,
    /// Tight lists omit the `<p>` wrapper around item paragraphs. Decided
    /// at finalize.
    pub tight: bool,
    /// Ordered-list start number; ignored for bullet lists.
    pub start: i64,
    /// `.` or `)` for ordered lists; a change of delimiter starts a new list.
    pub delimiter: String,
    /// `-`, `+` or `*` for bullet lists; a change of char starts a new list.
    pub bullet_char: String,
    /// Columns of padding between the marker and the item content.
    pub padding: usize,
    /// Column at which the marker starts.
    pub marker_offset: usize,
}

/// `[[startLine, startCol], [endLine, endCol]]`, all 1-based, with columns
/// counted in characters (not bytes) as the spec counts them.
pub type SourcePos = [[usize; 2]; 2];

/// An index into a [`Tree`]'s arena.
pub type NodeId = usize;

/// One node of the tree.
#[derive(Debug, Clone)]
pub struct MdNode {
    pub node_type: NodeType,

    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,

    pub sourcepos: SourcePos,

    /// Text payload for text / code / html_block / html_inline / code_block.
    pub literal: String,

    /// Heading level 1..6.
    pub level: usize,

    /// Link/image destination, already entity-decoded and
    /// backslash-unescaped.
    pub destination: String,
    /// Link/image title, already entity-decoded and backslash-unescaped.
    /// `None` is the canonical runtime's `null` (the Go port's `HasTitle`
    /// false).
    pub title: Option<String>,

    /// Fenced code info string (raw, entity-decoded); `None` for indented
    /// code, which is the canonical `null` and Go's `HasInfo` false.
    pub info: Option<String>,
    /// True for fenced code blocks, false for indented.
    pub is_fenced: bool,
    pub fence_char: u8,
    pub fence_length: usize,
    pub fence_offset: usize,

    pub list_data: Option<ListData>,

    /// GFM tables: one entry per column, in order, set on the **table** node
    /// by the block phase from the delimiter row. Every row of the table,
    /// header and body alike, has exactly this many cells, because short
    /// rows are padded and long ones are truncated. `None` on every other
    /// node type.
    pub table_align: Option<Vec<TableAlign>>,

    /// GFM tables: true on the one `table_row` that is the header row, which
    /// is always the table's first child. mdast has no header flag and
    /// relies on that convention, but the renderer needs to choose `<th>`
    /// over `<td>` without walking back up to the table.
    pub is_header_row: bool,

    /// GFM task list items (`- [x] foo`): `Some(true)` / `Some(false)` for a
    /// checked / unchecked item, `None` for an ordinary one. Set on `item`
    /// nodes by the block phase when `gfm` is on, once the marker has been
    /// consumed from the item's first paragraph. mdast's field and mdast's
    /// semantics; `ast.rs` projects it straight through to
    /// `listItem.checked`.
    pub checked: Option<bool>,

    /// The `gfm` parse option, recorded on the **document** node by the
    /// block phase and meaningless anywhere else. The renderer defaults to
    /// it when called without options, so a tree parsed as plain CommonMark
    /// renders as plain CommonMark.
    pub gfm: bool,

    // --- block-phase bookkeeping, not part of the rendered tree ---
    /// Open blocks accept further lines; closed ones are finalized.
    pub open: bool,
    /// Accumulated raw content, consumed by the inline phase.
    pub string_content: String,
    /// A block whose last line was blank cannot be lazily continued.
    pub last_line_blank: bool,
    pub last_line_checked: bool,
    /// A paragraph's source line where `string_content` begins: its first
    /// line, moved on past each definition read off the front, so a
    /// definition read later knows its lines.
    pub content_line: usize,

    /// A `link_reference`'s or `image_reference`'s label (its raw source
    /// text, between its brackets) and the form that named it.
    pub label: String,
    pub reference_type: Option<ReferenceType>,

    /// A container's link reference definitions, read from paragraphs
    /// among its children, each with where it stands.
    pub definitions: Vec<DefinitionSite>,
}

impl MdNode {
    /// An open, unlinked node of the given type.
    pub fn new(node_type: NodeType) -> Self {
        MdNode {
            node_type,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
            sourcepos: [[0, 0], [0, 0]],
            literal: String::new(),
            level: 0,
            destination: String::new(),
            title: None,
            info: None,
            is_fenced: false,
            fence_char: 0,
            fence_length: 0,
            fence_offset: 0,
            list_data: None,
            table_align: None,
            is_header_row: false,
            checked: None,
            gfm: true,
            open: true,
            string_content: String::new(),
            last_line_blank: false,
            last_line_checked: false,
            content_line: 0,
            label: String::new(),
            reference_type: None,
            definitions: Vec::new(),
        }
    }

    /// Containers may hold children; leaves carry only `literal`.
    pub fn is_container(&self) -> bool {
        self.node_type.is_container()
    }
}

/// The arena holding one document's nodes, with the document at
/// [`Tree::root`].
#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<MdNode>,
    root: NodeId,
    /// Slots [`Tree::free_subtree`] gave back, which [`Tree::add`] fills
    /// before it grows the arena. Empty unless a driver that keeps no tree
    /// frees the blocks it has written (tabnas/markdown#100).
    free: Vec<NodeId>,
}

impl Tree {
    /// A tree holding only a fresh node of `root_type`, which becomes the
    /// root.
    pub fn new(root_type: NodeType) -> Self {
        Tree {
            nodes: vec![MdNode::new(root_type)],
            root: 0,
            free: Vec::new(),
        }
    }

    /// The root node's id (the document, for a parse).
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Add an unlinked node to the arena and return its id: a slot
    /// [`Tree::free_subtree`] gave back, when there is one.
    pub fn add(&mut self, node: MdNode) -> NodeId {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = node;
            return id;
        }
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Detach `node` and give its slot, and the slots of every node below
    /// it, back to the arena for [`Tree::add`] to fill, dropping what each
    /// held; answer the ids given back. Only for a node nothing will read
    /// again: a slot given back takes the next node added, under the same
    /// id. Iterative, since nesting is the input's to choose.
    pub fn free_subtree(&mut self, node: NodeId) -> Vec<NodeId> {
        self.unlink(node);
        let mut freed = Vec::new();
        let mut pending = vec![node];
        while let Some(id) = pending.pop() {
            let mut child = self.nodes[id].first_child;
            while let Some(c) = child {
                pending.push(c);
                child = self.nodes[c].next;
            }
            self.nodes[id] = MdNode::new(NodeType::Paragraph);
            freed.push(id);
        }
        self.free.extend(freed.iter().copied());
        freed
    }

    /// Add an open, unlinked node of `node_type`.
    pub fn new_node(&mut self, node_type: NodeType) -> NodeId {
        self.add(MdNode::new(node_type))
    }

    pub fn get(&self, id: NodeId) -> &MdNode {
        &self.nodes[id]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut MdNode {
        &mut self.nodes[id]
    }

    /// How many slots the arena holds, unlinked nodes' and given-back ones
    /// included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The children of `parent`, first to last.
    pub fn children(&self, parent: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut child = self.nodes[parent].first_child;
        while let Some(c) = child {
            out.push(c);
            child = self.nodes[c].next;
        }
        out
    }

    /// Add `child` as the last child of `parent`, detaching it first.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        if let Some(last) = self.nodes[parent].last_child {
            self.nodes[last].next = Some(child);
            self.nodes[child].prev = Some(last);
            self.nodes[parent].last_child = Some(child);
        } else {
            self.nodes[parent].first_child = Some(child);
            self.nodes[parent].last_child = Some(child);
        }
    }

    /// Add `child` as the first child of `parent`, detaching it first.
    pub fn prepend_child(&mut self, parent: NodeId, child: NodeId) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        if let Some(first) = self.nodes[parent].first_child {
            self.nodes[first].prev = Some(child);
            self.nodes[child].next = Some(first);
            self.nodes[parent].first_child = Some(child);
        } else {
            self.nodes[parent].first_child = Some(child);
            self.nodes[parent].last_child = Some(child);
        }
    }

    /// Place `sibling` directly after `node`.
    pub fn insert_after(&mut self, node: NodeId, sibling: NodeId) {
        self.unlink(sibling);
        let next = self.nodes[node].next;
        self.nodes[sibling].next = next;
        if let Some(n) = next {
            self.nodes[n].prev = Some(sibling);
        }
        self.nodes[sibling].prev = Some(node);
        self.nodes[node].next = Some(sibling);
        let parent = self.nodes[node].parent;
        self.nodes[sibling].parent = parent;
        if next.is_none() {
            if let Some(p) = parent {
                self.nodes[p].last_child = Some(sibling);
            }
        }
    }

    /// Place `sibling` directly before `node`.
    pub fn insert_before(&mut self, node: NodeId, sibling: NodeId) {
        self.unlink(sibling);
        let prev = self.nodes[node].prev;
        self.nodes[sibling].prev = prev;
        if let Some(p) = prev {
            self.nodes[p].next = Some(sibling);
        }
        self.nodes[sibling].next = Some(node);
        self.nodes[node].prev = Some(sibling);
        let parent = self.nodes[node].parent;
        self.nodes[sibling].parent = parent;
        if prev.is_none() {
            if let Some(p) = parent {
                self.nodes[p].first_child = Some(sibling);
            }
        }
    }

    /// Detach `node` from the tree, leaving the node itself intact.
    pub fn unlink(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = &self.nodes[node];
            (n.parent, n.prev, n.next)
        };
        match prev {
            Some(p) => self.nodes[p].next = next,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].first_child = next;
                }
            }
        }
        match next {
            Some(n) => self.nodes[n].prev = prev,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].last_child = prev;
                }
            }
        }
        let n = &mut self.nodes[node];
        n.parent = None;
        n.next = None;
        n.prev = None;
    }

    /// A depth-first walker rooted at `root`. Containers are entered and
    /// exited; leaves produce one event.
    pub fn walker(&self, root: NodeId) -> Walker {
        Walker {
            current: Some(root),
            root,
            entering: true,
        }
    }
}

/// One step of a depth-first walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkEvent {
    pub entering: bool,
    pub node: NodeId,
}

/// A depth-first walker over the subtree rooted at `root`. It holds ids
/// only, so the tree may be mutated between steps as long as the node the
/// walker stands on stays linked, which is what both parse phases rely on.
#[derive(Debug, Clone)]
pub struct Walker {
    current: Option<NodeId>,
    root: NodeId,
    entering: bool,
}

impl Walker {
    /// The next event, or `None` when the walk is done.
    pub fn next(&mut self, tree: &Tree) -> Option<WalkEvent> {
        let cur = self.current?;
        let entering = self.entering;
        let node = tree.get(cur);

        if entering && node.is_container() {
            if let Some(first) = node.first_child {
                self.current = Some(first);
                self.entering = true;
            } else {
                self.entering = false;
            }
        } else if cur == self.root {
            self.current = None;
        } else if let Some(next) = node.next {
            self.current = Some(next);
            self.entering = true;
        } else {
            self.current = node.parent;
            self.entering = false;
        }

        Some(WalkEvent {
            entering,
            node: cur,
        })
    }

    /// Reposition the walker.
    pub fn resume_at(&mut self, node: NodeId, entering: bool) {
        self.current = Some(node);
        self.entering = entering;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_keep_the_links_consistent() {
        let mut tree = Tree::new(NodeType::Document);
        let doc = tree.root();
        let a = tree.new_node(NodeType::Paragraph);
        let b = tree.new_node(NodeType::Paragraph);
        let c = tree.new_node(NodeType::Paragraph);
        tree.append_child(doc, a);
        tree.append_child(doc, c);
        tree.insert_after(a, b);
        assert_eq!(tree.children(doc), vec![a, b, c]);
        tree.unlink(b);
        assert_eq!(tree.children(doc), vec![a, c]);
        assert_eq!(tree.get(b).parent, None);
        tree.insert_before(a, b);
        assert_eq!(tree.children(doc), vec![b, a, c]);
        tree.prepend_child(doc, c);
        assert_eq!(tree.children(doc), vec![c, b, a]);
        assert_eq!(tree.get(doc).last_child, Some(a));
    }

    #[test]
    fn the_walker_enters_and_exits_containers() {
        let mut tree = Tree::new(NodeType::Document);
        let doc = tree.root();
        let p = tree.new_node(NodeType::Paragraph);
        let t = tree.new_node(NodeType::Text);
        tree.append_child(doc, p);
        tree.append_child(p, t);
        let mut walker = tree.walker(doc);
        let mut events = Vec::new();
        while let Some(event) = walker.next(&tree) {
            events.push((event.entering, event.node));
        }
        assert_eq!(
            events,
            vec![(true, doc), (true, p), (true, t), (false, p), (false, doc)]
        );
    }
}
