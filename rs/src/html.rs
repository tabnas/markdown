/* Copyright (c) 2021-2026 Richard Rodger, MIT License */

//! HTML renderer for the finished CommonMark tree. Port of `ts/src/html.ts`
//! and `go/html.go`; keep the three in step.
//!
//! The conformance suite compares this output to the spec's expected HTML
//! byte for byte, so newline placement is a correctness contract here, not
//! a formatting preference. The discipline that makes it come out right is
//! the reference renderer's:
//!
//! - every block writes `cr()` before its opening tag and after its closing
//!   tag, and `cr()` is a no-op when the buffer is already at the start of
//!   a line;
//! - no block ever emits a newline on behalf of a neighbour or a child.
//!
//! Because a block only ever asks for "be at line start", nested blocks
//! compose without doubling up: `<li>` deliberately does *not* end a line,
//! and the newline in `<li>\n<p>...` is the paragraph's own leading `cr()`.
//! A tight item, whose paragraph is skipped entirely, therefore renders as
//! `<li>text</li>` with no interior newlines at all.
//!
//! The renderer reads only the finished tree, never `string_content`,
//! which the inline phase has already consumed.

use crate::common::{escape_xml, js_space_index, normalize_uri};
use crate::node::{ListType, NodeId, NodeType, TableAlign, Tree};
use crate::options::Options;

/// An HTML attribute as name and already-escaped value.
struct Attr<'a>(&'a str, String);

/// The output buffer and the one bit of state `cr` needs.
struct Renderer {
    buf: String,
    /// Exactly "the buffer is empty or ends with a newline", maintained by
    /// `lit` and `tag` rather than re-read from the buffer. An empty buffer
    /// counts as at-line-start, so a leading `cr()` (every top-level block
    /// opens with one) writes nothing.
    at_line_start: bool,
}

impl Renderer {
    fn lit(&mut self, s: &str) {
        // An empty write must not claim the line is dirty, or the next
        // `cr()` would insert a spurious newline.
        if s.is_empty() {
            return;
        }
        self.buf.push_str(s);
        self.at_line_start = s.ends_with('\n');
    }

    /// Append a newline only if the buffer does not already end with one.
    fn cr(&mut self) {
        if !self.at_line_start {
            self.lit("\n");
        }
    }

    /// Write an opening, closing or self-closing tag. A tag always ends
    /// with `>`, so the buffer is never at a line start after one.
    fn tag(&mut self, name: &str, attrs: &[Attr<'_>], self_closing: bool) {
        self.buf.push('<');
        self.buf.push_str(name);
        for Attr(attr_name, value) in attrs {
            self.buf.push(' ');
            self.buf.push_str(attr_name);
            self.buf.push_str("=\"");
            self.buf.push_str(value);
            self.buf.push('"');
        }
        if self_closing {
            self.buf.push_str(" /");
        }
        self.buf.push('>');
        self.at_line_start = false;
    }
}

// --- GFM disallowed raw HTML (tagfilter) ------------------------------------

/// GFM's tagfilter set. These nine tags change how the *rest* of a document
/// is interpreted (everything after `<xmp>` or `<plaintext>` stops being
/// markup at all), so GFM neutralises them by rewriting the leading `<` as
/// `&lt;`, opening and closing, in any case. Every other tag still passes
/// through verbatim, as CommonMark requires.
///
/// This is purely a rendering concern: nothing rewrites the node, so the
/// literal in the tree and the `value` in the AST stay exactly as written.
///
/// No name is a prefix of another, so the order here is immaterial.
pub const DISALLOWED_TAGS: [&str; 9] = [
    "title",
    "textarea",
    "style",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "script",
    "plaintext",
];

/// The TypeScript's lookahead, `(?=[\t\n\f\r />]|$)`: the tag name must be
/// followed by whitespace, `/`, `>` or the end of the text, so
/// `<scriptlet>` and `<titles>` are untouched.
fn is_tag_name_end(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ' | b'/' | b'>')
}

/// The byte length of the disallowed-tag match starting at `s[i]` (which
/// must be `<`): the `<`, an optional `/` and the tag name, exactly what
/// the TypeScript's regex captures. `None` for no match.
///
/// Case folding is ASCII-only, deliberately. The TypeScript applies the `i`
/// flag without `u`, which never folds a non-ASCII code point onto an
/// ASCII one.
fn disallowed_tag_len(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if j < s.len() && s[j] == b'/' {
        j += 1;
    }

    for name in DISALLOWED_TAGS {
        let name = name.as_bytes();
        if s.len() - j < name.len() {
            continue;
        }
        if !s[j..j + name.len()]
            .iter()
            .zip(name)
            .all(|(c, n)| c.to_ascii_lowercase() == *n)
        {
            continue;
        }
        let end = j + name.len();
        if end == s.len() || is_tag_name_end(s[end]) {
            return Some(end - i);
        }
    }

    None
}

fn filter_disallowed_tags(s: &str) -> String {
    // Cheap reject: the vast majority of raw HTML holds none of these.
    if !s.contains('<') {
        return s.to_string();
    }

    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut matched = false;
    let mut last = 0;

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(n) = disallowed_tag_len(bytes, i) else {
            i += 1;
            continue;
        };
        if !matched {
            matched = true;
            out.reserve(s.len() + 8);
        }
        out.push_str(&s[last..i]);
        // Only the leading `<` changes; the tag name is copied as written.
        out.push_str("&lt;");
        out.push_str(&s[i + 1..i + n]);
        last = i + n;
        i = last;
    }

    if !matched {
        return s.to_string();
    }
    out.push_str(&s[last..]);
    out
}

/// Raw HTML as written, minus the tags GFM's tagfilter neutralises.
fn raw_html(literal: &str, gfm: bool) -> String {
    if gfm {
        filter_disallowed_tags(literal)
    } else {
        literal.to_string()
    }
}

/// Everything before the first whitespace run, which is what the
/// TypeScript gets from `info.split(/\s+/)[0]`: an info string that
/// *starts* with whitespace yields `""`, which suppresses the class
/// attribute entirely. The block phase trims the info string, but an entity
/// such as `&#32;` unescapes to a space afterwards, so leading whitespace
/// is reachable.
fn first_info_word(info: &str) -> &str {
    match js_space_index(info) {
        Some((sp, _)) => &info[..sp],
        None => info,
    }
}

/// Render `tree` (one that has been through both parse phases) as HTML.
///
/// Two options reach the renderer. `breaks` turns soft line breaks into
/// hard ones. `gfm` selects one thing only, the disallowed-raw-HTML
/// filter, which the extension defines at render time rather than at
/// parse time. Everything else GFM adds is settled by then: by the time a
/// `del` node or an item's checked state exists, the renderer just prints
/// it.
///
/// `gfm` defaults to whatever the *document* was parsed with rather than to
/// the package default, so [`render_html`] with `None` on a `gfm: false`
/// parse renders plain CommonMark. Explicit options still win.
pub fn render_html(tree: &Tree, options: Option<&Options>) -> String {
    let root = tree.root();
    let opts = Options {
        gfm: options.map_or(tree.get(root).gfm, |o| o.gfm),
        breaks: options.is_some_and(|o| o.breaks),
    };
    render_html_with(tree, opts)
}

/// Render with already-resolved options, the way the Go `RenderHTML`
/// takes them.
pub fn render_html_with(tree: &Tree, opts: Options) -> String {
    // Section 6.9 soft line breaks: rendered as a newline, or as a hard
    // break when the caller asks for `breaks`.
    let softbreak = if opts.breaks { "<br />\n" } else { "\n" };

    let mut r = Renderer {
        buf: String::new(),
        at_line_start: true,
    };

    // GFM tables carry their alignment once, on the table, but it is
    // rendered per cell. Tracking the current table's alignments and the
    // current row's column as the walk goes is what keeps that an O(1)
    // lookup instead of counting a cell's previous siblings; a table can
    // never contain another table (its cells hold inlines only), so one of
    // each is enough.
    let mut table_align: &[TableAlign] = &[];
    let mut cell_index = 0usize;

    let mut walker = tree.walker(tree.root());

    while let Some(event) = walker.next(tree) {
        let id = event.node;
        let node = tree.get(id);
        let entering = event.entering;

        match node.node_type {
            // The document wrapper renders nothing of its own.
            NodeType::Document => {}

            NodeType::Paragraph => {
                // Section 5.3: "if a list is tight, we remove the <p> tags
                // from the item contents". The paragraph's grandparent is
                // the list; a paragraph whose grandparent is a list is
                // necessarily an item's direct child.
                let grandparent = node.parent.and_then(|p| tree.get(p).parent);
                let tight = grandparent.is_some_and(|g| {
                    let g = tree.get(g);
                    g.node_type == NodeType::List && g.list_data.as_ref().is_some_and(|d| d.tight)
                });

                if entering {
                    if !tight {
                        r.cr();
                        r.tag("p", &[], false);
                    }
                    // GFM task list item. The block phase consumed the
                    // `[x]` marker and left the state on the item, so the
                    // checkbox is written here, at the head of the item's
                    // first paragraph: directly after `<li>` when the list
                    // is tight, and inside the `<p>` when it is loose. The
                    // trailing space is the separator the extension's own
                    // output shows between the checkbox and the item text.
                    if let Some(item) = node.parent.map(|p| tree.get(p)) {
                        if item.node_type == NodeType::Item && item.first_child == Some(id) {
                            if let Some(checked) = item.checked {
                                let mut attrs = Vec::new();
                                if checked {
                                    attrs.push(Attr("checked", String::new()));
                                }
                                attrs.push(Attr("disabled", String::new()));
                                attrs.push(Attr("type", "checkbox".to_string()));
                                r.tag("input", &attrs, false);
                                r.lit(" ");
                            }
                        }
                    }
                } else if !tight {
                    r.tag("/p", &[], false);
                    r.cr();
                }
            }

            NodeType::Heading => {
                let name = format!("h{}", node.level);
                if entering {
                    r.cr();
                    r.tag(&name, &[], false);
                } else {
                    r.tag(&format!("/{name}"), &[], false);
                    r.cr();
                }
            }

            NodeType::ThematicBreak => {
                r.cr();
                r.tag("hr", &[], true);
                r.cr();
            }

            NodeType::BlockQuote => {
                r.cr();
                r.tag(
                    if entering {
                        "blockquote"
                    } else {
                        "/blockquote"
                    },
                    &[],
                    false,
                );
                r.cr();
            }

            NodeType::List => {
                let data = node.list_data.as_ref();
                let ordered = data.is_some_and(|d| d.list_type == ListType::Ordered);
                let name = if ordered { "ol" } else { "ul" };
                if entering {
                    let mut attrs = Vec::new();
                    // Section 5.3: the start number is only rendered when it
                    // is not 1, and `start="0"` is meaningful, so this is an
                    // inequality, not a truthiness test.
                    if let Some(d) = data {
                        if ordered && d.start != 1 {
                            attrs.push(Attr("start", d.start.to_string()));
                        }
                    }
                    r.cr();
                    r.tag(name, &attrs, false);
                } else {
                    r.cr();
                    r.tag(&format!("/{name}"), &[], false);
                }
                r.cr();
            }

            NodeType::Item => {
                // No `cr()` after `<li>`: a tight item renders
                // `<li>text</li>`, and a loose one gets its newline from the
                // leading `cr()` of the first child block.
                if entering {
                    r.tag("li", &[], false);
                } else {
                    r.tag("/li", &[], false);
                    r.cr();
                }
            }

            NodeType::CodeBlock => {
                let mut attrs = Vec::new();
                // Section 4.5: "the first word of the info string is
                // typically used to specify the language". The info string
                // arrives from the block phase already backslash-unescaped
                // and entity-decoded; only the escaping for attribute
                // context is left to do here.
                let info = node.info.as_deref().unwrap_or("");
                let first_word = if info.is_empty() {
                    ""
                } else {
                    first_info_word(info)
                };
                if !first_word.is_empty() {
                    attrs.push(Attr(
                        "class",
                        format!("language-{}", escape_xml(first_word)),
                    ));
                }
                r.cr();
                r.tag("pre", &[], false);
                r.tag("code", &attrs, false);
                // The block phase guarantees the literal already ends with
                // a newline (or is empty, for an empty fenced block, which
                // renders as `<pre><code></code></pre>`).
                r.lit(&escape_xml(&node.literal));
                r.tag("/code", &[], false);
                r.tag("/pre", &[], false);
                r.cr();
            }

            NodeType::HtmlBlock => {
                // Section 4.6: raw HTML passes through verbatim, unescaped,
                // except for the nine tags GFM's tagfilter neutralises.
                r.cr();
                r.lit(&raw_html(&node.literal, opts.gfm));
                r.cr();
            }

            // --- GFM tables ---
            //
            // `<thead>` and `<tbody>` have no node of their own: the header
            // row is the one row flagged as such, and the body is every row
            // after it. That is also why `<tbody>` is written by the *first*
            // body row rather than unconditionally: a table with no body
            // rows must have no `<tbody>` at all.
            NodeType::Table => {
                if entering {
                    table_align = node.table_align.as_deref().unwrap_or(&[]);
                    r.cr();
                    r.tag("table", &[], false);
                } else {
                    r.cr();
                    r.tag("/table", &[], false);
                }
                r.cr();
            }

            NodeType::TableRow => {
                let header = node.is_header_row;
                if entering {
                    if header {
                        r.cr();
                        r.tag("thead", &[], false);
                    } else if node.prev.is_none_or(|p| tree.get(p).is_header_row) {
                        r.cr();
                        r.tag("tbody", &[], false);
                    }
                    r.cr();
                    r.tag("tr", &[], false);
                    cell_index = 0;
                } else {
                    r.cr();
                    r.tag("/tr", &[], false);
                    r.cr();
                    if header {
                        r.tag("/thead", &[], false);
                    } else if node.next.is_none() {
                        r.tag("/tbody", &[], false);
                    }
                }
                r.cr();
            }

            NodeType::TableCell => {
                let name = if node.parent.is_some_and(|p| tree.get(p).is_header_row) {
                    "th"
                } else {
                    "td"
                };
                if entering {
                    let mut attrs = Vec::new();
                    // Out of range and a column with no colon both mean no
                    // attribute at all.
                    if let Some(Some(align)) = table_align.get(cell_index) {
                        attrs.push(Attr("align", align.as_str().to_string()));
                    }
                    cell_index += 1;
                    r.tag(name, &attrs, false);
                } else {
                    r.tag(&format!("/{name}"), &[], false);
                    r.cr();
                }
            }

            NodeType::Text => r.lit(&escape_xml(&node.literal)),

            NodeType::Softbreak => r.lit(softbreak),

            NodeType::Linebreak => {
                r.tag("br", &[], true);
                r.cr();
            }

            NodeType::Code => {
                r.tag("code", &[], false);
                r.lit(&escape_xml(&node.literal));
                r.tag("/code", &[], false);
            }

            NodeType::HtmlInline => r.lit(&raw_html(&node.literal, opts.gfm)),

            NodeType::Emph => r.tag(if entering { "em" } else { "/em" }, &[], false),

            NodeType::Strong => r.tag(if entering { "strong" } else { "/strong" }, &[], false),

            NodeType::Del => r.tag(if entering { "del" } else { "/del" }, &[], false),

            NodeType::Link => {
                if entering {
                    let mut attrs =
                        vec![Attr("href", escape_xml(&normalize_uri(&node.destination)))];
                    // An empty title is dropped: `[link](/url "")` renders as
                    // a bare `<a href="/url">`.
                    if let Some(title) = node.title.as_deref().filter(|t| !t.is_empty()) {
                        attrs.push(Attr("title", escape_xml(title)));
                    }
                    r.tag("a", &attrs, false);
                } else {
                    r.tag("/a", &[], false);
                }
            }

            NodeType::Image => {
                if entering {
                    let mut attrs = vec![
                        Attr("src", escape_xml(&normalize_uri(&node.destination))),
                        Attr("alt", escape_xml(&alt_text(tree, id))),
                    ];
                    if let Some(title) = node.title.as_deref().filter(|t| !t.is_empty()) {
                        attrs.push(Attr("title", escape_xml(title)));
                    }
                    r.tag("img", &attrs, true);
                    // The children have been consumed into `alt`; jump the
                    // walk straight to this image's exit event so they are
                    // not also rendered as markup.
                    walker.resume_at(id, false);
                }
            }
        }
    }

    r.buf
}

/// Section 6.4 images: "the image description gets represented as the alt
/// attribute", as plain text. Descend the subtree keeping the text and
/// code-span literals and dropping the markup wrappers, so
/// `![foo *bar*](/url)` yields `alt="foo bar"` and a nested link
/// contributes only its text.
///
/// Raw inline HTML and line breaks inside a description are not exercised
/// by the spec suite and the two reference implementations disagree: cmark
/// escapes the raw HTML into the attribute and turns breaks into spaces,
/// commonmark.js emits the HTML verbatim (producing a malformed attribute)
/// and turns breaks into newlines. The literal is kept, escaped, since the
/// result is attribute-context text, and breaks follow commonmark.js.
fn alt_text(tree: &Tree, image: NodeId) -> String {
    let mut alt = String::new();

    let mut walker = tree.walker(image);
    walker.next(tree); // discard the image's own entering event

    while let Some(event) = walker.next(tree) {
        if event.node == image {
            break;
        }
        if !event.entering {
            continue;
        }
        let node = tree.get(event.node);
        match node.node_type {
            NodeType::Text | NodeType::Code | NodeType::HtmlInline => alt.push_str(&node.literal),
            NodeType::Softbreak | NodeType::Linebreak => alt.push('\n'),
            // Containers (emph, strong, link, a nested image) contribute
            // nothing themselves; the walk still descends into their
            // children.
            _ => {}
        }
    }

    alt
}
