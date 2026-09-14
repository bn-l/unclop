use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use tree_sitter::{Node, Parser};

use crate::scan::lang::Registry;
use crate::scan::text;

pub fn run(file: &Path) -> Result<i32> {
    let registry = Registry::new()?;
    let Some(lang) = registry.for_path(file) else {
        bail!("no grammar for {}", file.display());
    };
    let src = fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let mut parser = Parser::new();
    parser.set_language(&lang.language)?;
    let tree = parser.parse(&src, None).context("parse failed")?;
    println!("language: {}", lang.name);
    dump(tree.root_node(), &src, 0, None);
    Ok(0)
}

fn dump(node: Node, src: &str, depth: usize, field: Option<&str>) {
    let indent = "  ".repeat(depth);
    let field = field.map(|f| format!("{f}: ")).unwrap_or_default();
    let s = node.start_position();
    let e = node.end_position();
    let mut line = format!(
        "{indent}{field}{} [{}:{}-{}:{}]",
        node.kind(),
        s.row + 1,
        s.column,
        e.row + 1,
        e.column
    );
    if node.named_child_count() == 0 {
        let t: String = text(node, src).chars().take(60).collect();
        line.push_str(&format!("  {t:?}"));
    }
    println!("{line}");
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for (i, child) in children.into_iter().enumerate() {
        if !child.is_named() {
            continue;
        }
        dump(child, src, depth + 1, node.field_name_for_child(i as u32));
    }
}
