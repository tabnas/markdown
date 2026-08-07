/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

package tabnasmarkdown

// Native CommonMark node tree — the internal parse result. Port of
// ts/src/node.ts; keep the two in step.
//
// A linked tree (Parent/FirstChild/LastChild/Prev/Next) rather than a children
// slice, because both phases of the CommonMark algorithm (spec 0.31.2
// Appendix A) splice nodes mid-walk: the block phase keeps a spine of open
// blocks and closes them from the tail, and the inline phase's delimiter stack
// rewrites sibling runs in place.
//
// The public map-based AST this package has always returned is produced from
// this tree by ast.go. That projection is lossy on purpose; the tree is what
// html.go renders for CommonMark conformance.

// NodeType enumerates the node kinds. String constants rather than an int
// enum so that debugging output and the AST projection read the same.
type NodeType string

const (
	// Containers.
	NodeDocument   NodeType = "document"
	NodeBlockQuote NodeType = "block_quote"
	NodeList       NodeType = "list"
	NodeItem       NodeType = "item"

	// Leaf blocks.
	NodeParagraph     NodeType = "paragraph"
	NodeHeading       NodeType = "heading"
	NodeThematicBreak NodeType = "thematic_break"
	NodeCodeBlock     NodeType = "code_block"
	NodeHTMLBlock     NodeType = "html_block"

	// Inlines.
	NodeText       NodeType = "text"
	NodeSoftbreak  NodeType = "softbreak"
	NodeLinebreak  NodeType = "linebreak"
	NodeCode       NodeType = "code"
	NodeHTMLInline NodeType = "html_inline"
	NodeEmph       NodeType = "emph"
	NodeStrong     NodeType = "strong"
	NodeLink       NodeType = "link"
	NodeImage      NodeType = "image"
	NodeDel        NodeType = "del"
)

var containerTypes = map[NodeType]bool{
	NodeDocument:   true,
	NodeBlockQuote: true,
	NodeList:       true,
	NodeItem:       true,
	NodeParagraph:  true,
	NodeHeading:    true,
	NodeEmph:       true,
	NodeStrong:     true,
	NodeLink:       true,
	NodeImage:      true,
	NodeDel:        true,
}

// ListType distinguishes bullet from ordered lists.
type ListType string

const (
	ListBullet  ListType = "bullet"
	ListOrdered ListType = "ordered"
)

// ListData carries what §5.2/§5.3 need to decide list identity and rendering.
type ListData struct {
	Type ListType
	// Tight lists omit the <p> wrapper around item paragraphs. Set at finalize.
	Tight bool
	// Start number for ordered lists; ignored for bullet lists.
	Start int
	// '.' or ')' — a change of delimiter starts a new list.
	Delimiter string
	// '-', '+' or '*' — a change of char starts a new list.
	BulletChar string
	// Columns of padding between the marker and the item content.
	Padding int
	// Column at which the marker starts.
	MarkerOffset int
}

// SourcePos is [[startLine, startCol], [endLine, endCol]], all 1-based, with
// columns counted in characters (not bytes) as the spec counts them.
type SourcePos [2][2]int

// MdNode is one node of the tree.
type MdNode struct {
	Type NodeType

	Parent     *MdNode
	FirstChild *MdNode
	LastChild  *MdNode
	Prev       *MdNode
	Next       *MdNode

	SourcePos SourcePos

	// Text payload for text/code/html_block/html_inline/code_block.
	Literal string

	// Heading level 1..6.
	Level int

	// Link/image destination and title, already entity-decoded and
	// backslash-unescaped.
	Destination string
	Title       string
	HasTitle    bool

	// Fenced code info string (raw, entity-decoded); empty for indented code.
	Info        string
	HasInfo     bool
	IsFenced    bool
	FenceChar   byte
	FenceLength int
	FenceOffset int

	ListData *ListData

	// Checked is the GFM task list item state (`- [x] foo`): true or false for
	// a task item, and meaningless unless HasChecked is set. HasChecked is
	// Go's stand-in for the TypeScript's `checked: boolean | null` — an
	// ordinary item is `null` there and HasChecked:false here, the same shape
	// node.go already uses for Title/HasTitle and Info/HasInfo.
	//
	// Set on item nodes by the block phase when GFM is on, once the marker has
	// been consumed from the item's first paragraph. mdast's field and mdast's
	// semantics — ast.go projects the pair straight through to
	// `listItem.checked`.
	Checked    bool
	HasChecked bool

	// GFM is the parse option, recorded on the DOCUMENT node by the block
	// phase and meaningless anywhere else.
	//
	// TypeScript needs it: renderHTML(tree) may be called with no options at
	// all, and GFM's disallowed-raw-HTML filter is a render-time concern, so
	// the renderer defaults `gfm` to what the tree was parsed with rather than
	// to the package default. Go's RenderHTML takes an already-resolved
	// Options and so has no "absent" case to default — the field is here so
	// the tree carries the same information, and so a caller that wants
	// TypeScript's defaulting can spell it: RenderHTML(tree, Options{GFM:
	// tree.GFM, Breaks: false}).
	GFM bool

	// --- block-phase bookkeeping, not part of the rendered tree ---

	// Open blocks accept further lines; closed ones are finalized.
	Open bool
	// Accumulated raw content, consumed by the inline phase.
	StringContent []byte
	// A block whose last line was blank cannot be lazily continued.
	LastLineBlank   bool
	LastLineChecked bool
}

// NewNode returns an open node of the given type.
func NewNode(t NodeType) *MdNode {
	return &MdNode{Type: t, Open: true}
}

// IsContainer reports whether the node may hold children.
func (n *MdNode) IsContainer() bool { return containerTypes[n.Type] }

// AppendChild adds child as the last child, detaching it first.
func (n *MdNode) AppendChild(child *MdNode) {
	child.Unlink()
	child.Parent = n
	if n.LastChild != nil {
		n.LastChild.Next = child
		child.Prev = n.LastChild
		n.LastChild = child
	} else {
		n.FirstChild = child
		n.LastChild = child
	}
}

// PrependChild adds child as the first child, detaching it first.
func (n *MdNode) PrependChild(child *MdNode) {
	child.Unlink()
	child.Parent = n
	if n.FirstChild != nil {
		n.FirstChild.Prev = child
		child.Next = n.FirstChild
		n.FirstChild = child
	} else {
		n.FirstChild = child
		n.LastChild = child
	}
}

// InsertAfter places sibling directly after n.
func (n *MdNode) InsertAfter(sibling *MdNode) {
	sibling.Unlink()
	sibling.Next = n.Next
	if sibling.Next != nil {
		sibling.Next.Prev = sibling
	}
	sibling.Prev = n
	n.Next = sibling
	sibling.Parent = n.Parent
	if sibling.Next == nil && sibling.Parent != nil {
		sibling.Parent.LastChild = sibling
	}
}

// InsertBefore places sibling directly before n.
func (n *MdNode) InsertBefore(sibling *MdNode) {
	sibling.Unlink()
	sibling.Prev = n.Prev
	if sibling.Prev != nil {
		sibling.Prev.Next = sibling
	}
	sibling.Next = n
	n.Prev = sibling
	sibling.Parent = n.Parent
	if sibling.Prev == nil && sibling.Parent != nil {
		sibling.Parent.FirstChild = sibling
	}
}

// Unlink detaches n from the tree, leaving the node itself intact.
func (n *MdNode) Unlink() {
	if n.Prev != nil {
		n.Prev.Next = n.Next
	} else if n.Parent != nil {
		n.Parent.FirstChild = n.Next
	}

	if n.Next != nil {
		n.Next.Prev = n.Prev
	} else if n.Parent != nil {
		n.Parent.LastChild = n.Prev
	}

	n.Parent = nil
	n.Next = nil
	n.Prev = nil
}

// WalkEvent is one step of a depth-first walk. Containers produce an entering
// and a leaving event; leaves produce one.
type WalkEvent struct {
	Entering bool
	Node     *MdNode
}

// NodeWalker is a depth-first walker over the tree rooted at Root.
type NodeWalker struct {
	current  *MdNode
	root     *MdNode
	entering bool
}

// Walker returns a walker rooted at n.
func (n *MdNode) Walker() *NodeWalker {
	return &NodeWalker{current: n, root: n, entering: true}
}

// Next returns the next walk event, or nil when the walk is done.
func (w *NodeWalker) Next() *WalkEvent {
	cur := w.current
	entering := w.entering
	if cur == nil {
		return nil
	}

	container := cur.IsContainer()

	switch {
	case entering && container:
		if cur.FirstChild != nil {
			w.current = cur.FirstChild
			w.entering = true
		} else {
			w.entering = false
		}
	case cur == w.root:
		w.current = nil
	case cur.Next == nil:
		w.current = cur.Parent
		w.entering = false
	default:
		w.current = cur.Next
		w.entering = true
	}

	return &WalkEvent{Entering: entering, Node: cur}
}

// ResumeAt repositions the walker.
func (w *NodeWalker) ResumeAt(node *MdNode, entering bool) {
	w.current = node
	w.entering = entering
}
