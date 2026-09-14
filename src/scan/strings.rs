//! String literal extraction, driven by `queries/<lang>/strings.scm`.
//! `@string` marks a literal; `@skip` marks a context whose literals are not prose.

use tree_sitter::{Node, QueryCursor, StreamingIterator, Tree};

use super::comments::is_docstring;
use super::lang::{Lang, LangId};
use super::{RawItem, end_row, scope_of, text};
use crate::ids::{Kind, collapse_ws, strip_quotes, worth_string};

/// The literal's content: pieces of a concatenated string joined, heredoc
/// terminators dropped, quotes and prefixes stripped.
fn content(node: Node, src: &str) -> String {
    match node.kind() {
        "concatenated_string" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter(|c| c.kind().contains("string"))
                .map(|c| strip_quotes(text(c, src)).to_string())
                .collect::<Vec<_>>()
                .join("")
        }
        "heredoc_body" => {
            let mut cursor = node.walk();
            let end = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "heredoc_end")
                .map(|c| c.start_byte())
                .unwrap_or(node.end_byte());
            src[node.start_byte()..end].to_string()
        }
        _ => strip_quotes(text(node, src)).to_string(),
    }
}

pub fn extract(lang: &Lang, tree: &Tree, src: &str, min_words: usize, out: &mut Vec<RawItem>) {
    let Some(query) = &lang.strings else {
        return;
    };
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), src.as_bytes());

    let mut skips: Vec<(usize, usize)> = Vec::new();
    let mut literals: Vec<Node> = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures().iter() {
            match names[capture.index as usize] {
                "string" => literals.push(capture.node),
                "skip" => skips.push((capture.node.start_byte(), capture.node.end_byte())),
                _ => {}
            }
        }
    }

    literals.sort_by_key(|n| (n.start_byte(), std::cmp::Reverse(n.end_byte())));
    let mut last_end = 0usize;
    for node in literals {
        if node.start_byte() < last_end {
            continue;
        }
        last_end = node.end_byte();
        let (s, e) = (node.start_byte(), node.end_byte());
        if skips.iter().any(|(ss, se)| s >= *ss && e <= *se) {
            continue;
        }
        if lang.id == LangId::Python && is_docstring(node) {
            continue;
        }
        let normalized = collapse_ws(&content(node, src));
        if !worth_string(&normalized, min_words) {
            continue;
        }
        out.push(RawItem {
            kind: Kind::Str,
            start: s,
            end: e,
            row0: node.start_position().row as u32,
            row1: end_row(node, src) as u32,
            text: normalized,
            scope: scope_of(lang.id, node, src),
        });
    }
}
