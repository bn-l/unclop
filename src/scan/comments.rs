//! Comment extraction: collect, merge adjacent line comments, classify doc comments, drop noise.

use tree_sitter::{Node, Tree};

use super::lang::{Lang, LangId};
use super::{RawItem, end_row, only_whitespace_before, scope_of, text, walk_nodes};
use crate::ids::{Kind, is_noise_comment, is_pragma, normalize_comment, strip_quotes};

struct Group<'t> {
    first: Node<'t>,
    last: Node<'t>,
    line_style: bool,
    starts_line: bool,
    column: usize,
    doc: bool,
    docstring: bool,
    pragma: bool,
}

pub fn extract(lang: &Lang, tree: &Tree, src: &str, out: &mut Vec<RawItem>) {
    let mut nodes: Vec<(Node, bool)> = Vec::new();
    walk_nodes(tree.root_node(), |n| {
        if lang.comment_kinds.contains(&n.kind()) {
            nodes.push((n, false));
        } else if lang.id == LangId::Python && is_docstring(n) {
            nodes.push((n, true));
        }
    });

    let mut groups: Vec<Group> = Vec::new();
    for (node, docstring) in nodes {
        let starts_line = only_whitespace_before(src, node.start_byte());
        let line_style = !docstring && is_line_style(lang.id, node, src);
        let doc = docstring || has_doc_marker(lang.id, node, src);
        let column = node.start_position().column;
        // A pragma line stands alone: merged into the prose next to it, it would be
        // rewritten along with the prose, or hide that prose as noise.
        let pragma = line_style && is_pragma(&normalize_comment(text(node, src)));
        if let Some(g) = groups.last_mut() {
            let adjacent = line_style
                && g.line_style
                && starts_line
                && g.starts_line
                && !pragma
                && !g.pragma
                && g.column == column
                && node.start_position().row == end_row(g.last, src) + 1
                && doc == g.doc;
            if adjacent {
                g.last = node;
                continue;
            }
        }
        groups.push(Group {
            first: node,
            last: node,
            line_style,
            starts_line,
            column,
            doc,
            docstring,
            pragma,
        });
    }

    for g in groups {
        let raw = src[g.first.start_byte()..g.last.end_byte()].trim_end_matches(['\n', '\r']);
        let normalized = if g.docstring {
            normalize_comment(strip_quotes(raw))
        } else {
            normalize_comment(raw)
        };
        if normalized.is_empty() || is_noise_comment(&normalized, raw, g.first.start_position().row)
        {
            continue;
        }
        let doc = g.doc || precedes_declaration(lang.id, g.last, src);
        out.push(RawItem {
            kind: if doc { Kind::Doc } else { Kind::Comment },
            start: g.first.start_byte(),
            end: g.first.start_byte() + raw.len(),
            row0: g.first.start_position().row as u32,
            row1: end_row(g.last, src) as u32,
            text: normalized,
            scope: scope_of(lang.id, g.first, src),
        });
    }
}

fn is_line_style(lang: LangId, node: Node, src: &str) -> bool {
    match lang {
        LangId::Rust => node.kind() == "line_comment",
        _ => {
            let t = text(node, src);
            t.starts_with("//")
                || (t.starts_with('#') && !t.starts_with("#!")
                    || t.starts_with("#!") && node.start_position().row > 0)
                    && !t.starts_with("=begin")
        }
    }
}

fn has_doc_marker(lang: LangId, node: Node, src: &str) -> bool {
    match lang {
        LangId::Rust => node.child_by_field_name("doc").is_some(),
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript | LangId::C | LangId::Cpp => {
            let t = text(node, src);
            (t.starts_with("/**") && !t.starts_with("/**/"))
                || t.starts_with("///")
                || t.starts_with("/*!")
        }
        _ => false,
    }
}

/// Go and Ruby have no doc marker: a comment ending on the line above a declaration is its doc.
fn precedes_declaration(lang: LangId, last: Node, src: &str) -> bool {
    let kinds: &[&str] = match lang {
        LangId::Go => &[
            "package_clause",
            "function_declaration",
            "method_declaration",
            "type_declaration",
            "var_declaration",
            "const_declaration",
        ],
        LangId::Ruby => &["method", "singleton_method", "class", "module"],
        LangId::Python => &[
            "function_definition",
            "class_definition",
            "decorated_definition",
        ],
        LangId::Bash => &["function_definition"],
        _ => return false,
    };
    let Some(next) = last.next_sibling() else {
        return false;
    };
    kinds.contains(&next.kind()) && next.start_position().row == end_row(last, src) + 1
}

/// A Python string that is the first statement of a module, class or function body.
pub fn is_docstring(node: Node) -> bool {
    if node.kind() != "string" {
        return false;
    }
    let Some(stmt) = node.parent() else {
        return false;
    };
    if stmt.kind() != "expression_statement" || stmt.named_child_count() != 1 {
        return false;
    }
    let Some(container) = stmt.parent() else {
        return false;
    };
    let ok = match container.kind() {
        "module" => true,
        "block" => matches!(
            container.parent().map(|p| p.kind()),
            Some("function_definition" | "class_definition")
        ),
        _ => false,
    };
    if !ok {
        return false;
    }
    let mut prev = stmt.prev_named_sibling();
    while let Some(p) = prev {
        if p.kind() != "comment" {
            return false;
        }
        prev = p.prev_named_sibling();
    }
    true
}
