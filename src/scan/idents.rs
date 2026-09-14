//! Identifier extraction at declaration sites, driven by `queries/<lang>/decls.scm`.
//!
//! Capture names are kinds (`@fn`, `@var`, ...). A `.pattern` suffix means the
//! captured node is a binding pattern to walk for identifiers; a `.declarator`
//! suffix means a C-style declarator chain to follow down to its name.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, QueryCursor, StreamingIterator, Tree};

use super::lang::{Lang, LangId};
use super::{RawItem, scope_of, text};
use crate::ids::Kind;

/// Node kinds that are names. Anything else captured directly is dropped
/// (computed keys, string keys, numbers).
const NAME_KINDS: &[&str] = &[
    "identifier",
    "type_identifier",
    "property_identifier",
    "private_property_identifier",
    "field_identifier",
    "shorthand_property_identifier_pattern",
    "shorthand_field_identifier",
    "variable_name",
    "word",
    "constant",
    "instance_variable",
    "class_variable",
    "global_variable",
    "namespace_identifier",
    "setter",
];

const RUBY_RESERVED: &[&str] = &[
    "initialize",
    "to_s",
    "to_str",
    "to_a",
    "to_h",
    "to_proc",
    "inspect",
    "each",
    "<=>",
    "==",
    "eql?",
    "hash",
    "call",
    "method_missing",
    "respond_to_missing?",
    "respond_to?",
    "coerce",
    "dup",
    "clone",
    "freeze",
    "included",
    "extended",
    "inherited",
    "prepended",
    "method_added",
    "self",
];

pub fn extract(lang: &Lang, tree: &Tree, src: &str, out: &mut Vec<RawItem>) {
    let Some(query) = &lang.decls else {
        return;
    };
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), src.as_bytes());

    let mut candidates: Vec<(Node, Kind)> = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures().iter() {
            let name = names[capture.index as usize];
            if name.starts_with('_') || name == "skip" {
                continue;
            }
            if let Some(kind_name) = name.strip_suffix(".pattern") {
                let Some(kind) = Kind::parse(kind_name) else {
                    continue;
                };
                collect_bindings(lang.id, capture.node, &mut |n| candidates.push((n, kind)));
            } else if let Some(kind_name) = name.strip_suffix(".declarator") {
                let Some(kind) = Kind::parse(kind_name) else {
                    continue;
                };
                for (leaf, is_function) in resolve_declarator(capture.node) {
                    let kind = if is_function && matches!(kind, Kind::Var | Kind::Field) {
                        Kind::Fn
                    } else {
                        kind
                    };
                    candidates.push((leaf, kind));
                }
            } else if let Some(kind) = Kind::parse(name) {
                candidates.push((capture.node, kind));
            }
        }
    }

    let mut best: HashMap<usize, (Node, Kind)> = HashMap::new();
    for (node, kind) in candidates {
        if !NAME_KINDS.contains(&node.kind()) {
            continue;
        }
        match best.get(&node.start_byte()) {
            Some((_, existing)) if existing.priority() >= kind.priority() => {}
            _ => {
                best.insert(node.start_byte(), (node, kind));
            }
        }
    }
    let mut list: Vec<(Node, Kind)> = best.into_values().collect();
    list.sort_by_key(|(n, _)| n.start_byte());

    // Bash has no declarations, only assignments; list each name once per scope.
    let mut seen_vars: HashSet<(String, Option<String>)> = HashSet::new();

    for (node, kind) in list {
        let name = text(node, src);
        let kind = adjust_kind(lang.id, node, kind, src);
        if excluded(lang.id, node, kind, name, src) {
            continue;
        }
        let scope = scope_of(lang.id, node, src);
        if lang.id == LangId::Bash
            && kind == Kind::Var
            && !seen_vars.insert((name.to_string(), scope.clone()))
        {
            continue;
        }
        out.push(RawItem {
            kind,
            start: node.start_byte(),
            end: node.end_byte(),
            row0: node.start_position().row as u32,
            row1: node.end_position().row as u32,
            text: name.to_string(),
            scope,
        });
    }
}

fn leaf_kinds(lang: LangId) -> &'static [&'static str] {
    match lang {
        LangId::Rust => &["identifier", "shorthand_field_identifier"],
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript => {
            &["identifier", "shorthand_property_identifier_pattern"]
        }
        _ => &["identifier"],
    }
}

/// Nodes whose subtree holds uses, not bindings.
fn opaque(lang: LangId, kind: &str) -> bool {
    match lang {
        LangId::Rust => matches!(
            kind,
            "scoped_identifier" | "range_pattern" | "generic_type" | "scoped_type_identifier"
        ),
        LangId::Python => matches!(
            kind,
            "attribute" | "subscript" | "call" | "keyword_argument"
        ),
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript => matches!(
            kind,
            "member_expression"
                | "subscript_expression"
                | "call_expression"
                | "type_annotation"
                | "this"
        ),
        LangId::Go => matches!(kind, "selector_expression" | "index_expression"),
        _ => false,
    }
}

/// Child fields that hold expressions or types rather than bindings.
fn skip_field(lang: LangId, parent_kind: &str, field: Option<&str>) -> bool {
    let Some(field) = field else {
        return false;
    };
    if matches!(
        field,
        "type" | "value" | "right" | "default" | "return_type" | "alternative"
    ) {
        return true;
    }
    match lang {
        LangId::Rust => matches!(
            (parent_kind, field),
            ("tuple_struct_pattern", "type")
                | ("struct_pattern", "type")
                | ("field_pattern", "name")
        ),
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript => {
            matches!((parent_kind, field), ("pair_pattern", "key"))
        }
        _ => false,
    }
}

fn collect_bindings<'t>(lang: LangId, node: Node<'t>, f: &mut dyn FnMut(Node<'t>)) {
    if opaque(lang, node.kind()) {
        return;
    }
    if leaf_kinds(lang).contains(&node.kind()) {
        f(node);
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for (i, child) in children.into_iter().enumerate() {
        if !child.is_named() {
            continue;
        }
        let field = node.field_name_for_child(i as u32);
        if skip_field(lang, node.kind(), field) {
            continue;
        }
        collect_bindings(lang, child, f);
    }
}

/// Follows a C or C++ declarator down to its name. Returns the name node and
/// whether a function declarator was passed on the way.
pub fn resolve_declarator(node: Node) -> Vec<(Node, bool)> {
    let mut cur = node;
    let mut is_function = false;
    loop {
        match cur.kind() {
            "identifier" | "field_identifier" | "type_identifier" => {
                return vec![(cur, is_function)];
            }
            "function_declarator" => {
                is_function = true;
                let Some(next) = cur.child_by_field_name("declarator") else {
                    return Vec::new();
                };
                cur = next;
            }
            "init_declarator"
            | "pointer_declarator"
            | "array_declarator"
            | "parenthesized_declarator"
            | "attributed_declarator"
            | "reference_declarator" => {
                let next = cur.child_by_field_name("declarator").or_else(|| {
                    let mut c = cur.walk();
                    cur.named_children(&mut c).find(|n| {
                        n.kind().ends_with("declarator") || n.kind().ends_with("identifier")
                    })
                });
                let Some(next) = next else { return Vec::new() };
                cur = next;
            }
            "structured_binding_declarator" => {
                let mut c = cur.walk();
                return cur
                    .named_children(&mut c)
                    .filter(|n| n.kind() == "identifier")
                    .map(|n| (n, false))
                    .collect();
            }
            _ => return Vec::new(),
        }
    }
}

fn adjust_kind(lang: LangId, node: Node, kind: Kind, src: &str) -> Kind {
    match lang {
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript if kind == Kind::Var => {
            let mut cur = node.parent();
            while let Some(n) = cur {
                match n.kind() {
                    "variable_declarator" => {
                        let direct =
                            n.child_by_field_name("name").map(|c| c.id()) == Some(node.id());
                        if direct
                            && let Some(value) = n.child_by_field_name("value")
                            && matches!(
                                value.kind(),
                                "arrow_function" | "function_expression" | "generator_function"
                            )
                        {
                            return Kind::Fn;
                        }
                        let is_const = n
                            .parent()
                            .filter(|p| p.kind() == "lexical_declaration")
                            .and_then(|p| p.child_by_field_name("kind"))
                            .is_some_and(|k| text(k, src) == "const");
                        return if is_const { Kind::Const } else { Kind::Var };
                    }
                    "statement_block" | "program" | "class_body" | "formal_parameters" => break,
                    _ => cur = n.parent(),
                }
            }
            kind
        }
        _ => kind,
    }
}

fn excluded(lang: LangId, node: Node, kind: Kind, name: &str, src: &str) -> bool {
    if name.is_empty() || name == "_" {
        return true;
    }
    match lang {
        LangId::Rust => name == "self" || (kind == Kind::Fn && in_trait_impl(node)),
        LangId::Python => {
            name == "self"
                || name == "cls"
                || (kind == Kind::Fn && name.starts_with("__") && name.ends_with("__"))
        }
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript => {
            name == "this" || (kind == Kind::Fn && name == "constructor")
        }
        LangId::Cpp => {
            kind == Kind::Fn && (cpp_is_constructor(node, name, src) || cpp_has_override(node, src))
        }
        LangId::Ruby => name == "self" || (kind == Kind::Fn && RUBY_RESERVED.contains(&name)),
        _ => false,
    }
}

fn in_trait_impl(node: Node) -> bool {
    let Some(item) = node.parent() else {
        return false;
    };
    if item.kind() != "function_item" {
        return false;
    }
    let Some(list) = item.parent() else {
        return false;
    };
    let Some(imp) = list.parent() else {
        return false;
    };
    imp.kind() == "impl_item" && imp.child_by_field_name("trait").is_some()
}

fn enclosing_function_declarator(node: Node) -> Option<Node> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "function_declarator" => return Some(n),
            "pointer_declarator"
            | "parenthesized_declarator"
            | "reference_declarator"
            | "attributed_declarator" => cur = n.parent(),
            _ => return None,
        }
    }
    None
}

fn cpp_is_constructor(node: Node, name: &str, src: &str) -> bool {
    let Some(decl) = enclosing_function_declarator(node) else {
        return false;
    };
    let mut cur = decl.parent();
    while let Some(n) = cur {
        match n.kind() {
            "class_specifier" | "struct_specifier" => {
                return n
                    .child_by_field_name("name")
                    .is_some_and(|c| text(c, src) == name);
            }
            "field_declaration_list"
            | "field_declaration"
            | "function_definition"
            | "declaration"
            | "template_declaration"
            | "access_specifier" => cur = n.parent(),
            _ => return false,
        }
    }
    false
}

fn cpp_has_override(node: Node, src: &str) -> bool {
    let Some(decl) = enclosing_function_declarator(node) else {
        return false;
    };
    let has = |n: Node| {
        let mut c = n.walk();
        n.named_children(&mut c)
            .any(|ch| ch.kind() == "virtual_specifier" && text(ch, src) == "override")
    };
    if has(decl) {
        return true;
    }
    decl.parent().is_some_and(has)
}
