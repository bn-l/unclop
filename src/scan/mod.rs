//! Walks the tree, parses each supported file and produces items in source order.

pub mod comments;
pub mod idents;
pub mod lang;
pub mod strings;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;
use tree_sitter::{Node, Parser};
use xxhash_rust::xxh3::xxh3_64;

use crate::config::Config;
use crate::ids::{Kind, hash_id};
use crate::state::{Item, State, Status};
use lang::{Lang, LangId, Registry};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const BUILTIN_IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    ".git",
    "__pycache__",
    ".venv",
    "venv",
];

/// One extracted thing before it gets an id.
pub struct RawItem {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub row0: u32,
    pub row1: u32,
    pub text: String,
    pub scope: Option<String>,
}

pub struct ScanFile {
    pub mtime: u64,
    pub size: u64,
    pub items: Vec<Item>,
}

pub enum Scanned {
    /// Same mtime and size as the state record; stored items still hold.
    Unchanged,
    Parsed(ScanFile),
}

pub fn walk(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .add_custom_ignore_filename(".unclopignore");
    builder.filter_entry(|entry| {
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        if !is_dir {
            return true;
        }
        let name = entry.file_name().to_string_lossy();
        !BUILTIN_IGNORED_DIRS.contains(&name.as_ref())
    });

    let mut out = Vec::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("unclop: skipping: {err}");
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .with_context(|| format!("{} is outside {}", entry.path().display(), root.display()))?;
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        out.push((entry.into_path(), rel));
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

/// What the stored items depend on besides the files themselves: the unclop
/// version, the queries and `strings.min_words`.
pub fn scan_key(registry: &Registry, config: &Config) -> String {
    format!(
        "{}/{}/{:016x}",
        env!("CARGO_PKG_VERSION"),
        config.strings.min_words,
        registry.fingerprint()
    )
}

pub fn scan_all(
    registry: &Registry,
    root: &Path,
    config: &Config,
    state: &State,
) -> Result<BTreeMap<String, Scanned>> {
    let files = walk(root)?;
    let same_settings = state.scan_key == scan_key(registry, config);
    let results: Vec<Result<Option<(String, Scanned)>>> = files
        .par_iter()
        .map(|(abs, rel)| scan_one(registry, abs, rel, config, state, same_settings))
        .collect();
    let mut map = BTreeMap::new();
    for r in results {
        if let Some((rel, scanned)) = r? {
            map.insert(rel, scanned);
        }
    }
    Ok(map)
}

fn scan_one(
    registry: &Registry,
    abs: &Path,
    rel: &str,
    config: &Config,
    state: &State,
    same_settings: bool,
) -> Result<Option<(String, Scanned)>> {
    let Some(lang) = registry.for_path(abs) else {
        return Ok(None);
    };
    let meta = fs::metadata(abs).with_context(|| format!("stat {}", abs.display()))?;
    let size = meta.len();
    if size > MAX_FILE_BYTES {
        return Ok(None);
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(entry) = state.files.get(rel)
        && (entry.record.skipped
            || (same_settings && entry.record.mtime == mtime && entry.record.size == size))
    {
        return Ok(Some((rel.to_string(), Scanned::Unchanged)));
    }
    let bytes = fs::read(abs).with_context(|| format!("reading {}", abs.display()))?;
    let Ok(src) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let items = extract(lang, rel, &src, config)?;
    Ok(Some((
        rel.to_string(),
        Scanned::Parsed(ScanFile { mtime, size, items }),
    )))
}

/// Parses one file and returns its items in source order, ids assigned.
pub fn extract(lang: &Lang, rel: &str, src: &str, config: &Config) -> Result<Vec<Item>> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.language)
        .with_context(|| format!("loading {} grammar", lang.name))?;
    let Some(tree) = parser.parse(src, None) else {
        return Ok(Vec::new());
    };

    let mut raws: Vec<RawItem> = Vec::new();
    comments::extract(lang, &tree, src, &mut raws);
    idents::extract(lang, &tree, src, &mut raws);
    strings::extract(lang, &tree, src, config.strings.min_words, &mut raws);
    raws.sort_by_key(|r| (r.start, r.kind.category(), r.end));

    let lines: Vec<&str> = src.lines().collect();
    let mut counts: HashMap<String, u32> = HashMap::new();
    let items = raws
        .into_iter()
        .map(|r| {
            let base = hash_id(r.kind, &r.text);
            let n = counts.entry(base.clone()).or_insert(0);
            *n += 1;
            let id = if *n == 1 { base } else { format!("{base}~{n}") };
            let ctx = context_hash(&lines, r.row1 as usize);
            Item {
                path: rel.to_string(),
                id,
                kind: r.kind,
                text: r.text,
                scope: r.scope,
                line: [r.row0 + 1, r.row1 + 1],
                status: Status::Pending,
                ticks: Vec::new(),
                was: None,
                changed_after_done: false,
                alias: None,
                ctx: Some(ctx),
            }
        })
        .collect();
    Ok(items)
}

/// Hash of the line at `row` and the next non-blank line, trimmed. Two identical
/// comments or declarations usually differ in the code around them.
fn context_hash(lines: &[&str], row: usize) -> u64 {
    let here = lines.get(row).map_or("", |l| l.trim());
    let next = lines
        .iter()
        .skip(row + 1)
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    xxh3_64(format!("{here}\n{next}").as_bytes())
}

/// Pre-order visit of every node.
pub fn walk_nodes<'t>(root: Node<'t>, mut f: impl FnMut(Node<'t>)) {
    let mut cursor = root.walk();
    loop {
        f(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

pub fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

/// Row of the last visible character of a node. Some grammars include the
/// trailing newline in line-comment nodes, which puts `end_position` on the
/// following line.
pub fn end_row(node: Node, src: &str) -> usize {
    let t = text(node, src).trim_end_matches(['\n', '\r']);
    node.start_position().row + t.matches('\n').count()
}

/// Name of a C or C++ declarator for scope labels. Unlike
/// `idents::resolve_declarator` this follows qualified names too, so an
/// out-of-line `Foo::bar` definition still labels the items inside it.
fn declarator_name<'a>(node: Node<'a>, src: &str) -> Option<Node<'a>> {
    let mut cur = node;
    loop {
        match cur.kind() {
            "identifier" | "field_identifier" | "type_identifier" | "destructor_name"
            | "operator_name" => {
                return Some(cur);
            }
            "qualified_identifier" | "template_function" => {
                cur = cur.child_by_field_name("name")?;
            }
            "function_declarator"
            | "init_declarator"
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
                cur = next?;
            }
            _ => {
                let _ = src;
                return None;
            }
        }
    }
}

/// True when only spaces or tabs precede `byte` on its line.
pub fn only_whitespace_before(src: &str, byte: usize) -> bool {
    let line_start = src[..byte].rfind('\n').map_or(0, |i| i + 1);
    src[line_start..byte].chars().all(|c| c == ' ' || c == '\t')
}

/// Label for a declaration that encloses other items, plus the id of its own name
/// node so that the name is not reported as being inside itself.
fn scope_label(lang: LangId, node: Node, src: &str) -> Option<(String, Option<usize>)> {
    let kind = node.kind();
    let named = |word: &str| -> Option<(String, Option<usize>)> {
        let n = node.child_by_field_name("name")?;
        Some((format!("{word} {}", text(n, src)), Some(n.id())))
    };
    match lang {
        LangId::Rust => match kind {
            "function_item" | "function_signature_item" => named("fn"),
            "struct_item" => named("struct"),
            "enum_item" => named("enum"),
            "union_item" => named("union"),
            "trait_item" => named("trait"),
            "mod_item" => named("mod"),
            "macro_definition" => named("macro"),
            "impl_item" => {
                let ty = node.child_by_field_name("type").map(|n| text(n, src))?;
                let label = match node.child_by_field_name("trait") {
                    Some(t) => format!("impl {} for {ty}", text(t, src)),
                    None => format!("impl {ty}"),
                };
                Some((label, None))
            }
            _ => None,
        },
        LangId::Python => match kind {
            "function_definition" => named("def"),
            "class_definition" => named("class"),
            _ => None,
        },
        LangId::TypeScript | LangId::Tsx | LangId::JavaScript => match kind {
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                named("function")
            }
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                named("method")
            }
            "class_declaration" | "abstract_class_declaration" | "class" => named("class"),
            "interface_declaration" => named("interface"),
            "type_alias_declaration" => named("type"),
            "enum_declaration" => named("enum"),
            "internal_module" => named("namespace"),
            "arrow_function" | "function_expression" => {
                let parent = node.parent()?;
                match parent.kind() {
                    "variable_declarator" => {
                        let n = parent.child_by_field_name("name")?;
                        Some((format!("function {}", text(n, src)), Some(n.id())))
                    }
                    "pair" => {
                        let n = parent.child_by_field_name("key")?;
                        Some((format!("function {}", text(n, src)), Some(n.id())))
                    }
                    _ => None,
                }
            }
            _ => None,
        },
        LangId::Go => match kind {
            "function_declaration" => named("func"),
            "method_declaration" => named("method"),
            "type_spec" => named("type"),
            _ => None,
        },
        LangId::C | LangId::Cpp => match kind {
            "function_definition" => {
                let declarator = node.child_by_field_name("declarator")?;
                let leaf = declarator_name(declarator, src)?;
                Some((format!("fn {}", text(leaf, src)), Some(leaf.id())))
            }
            "struct_specifier" => named("struct"),
            "union_specifier" => named("union"),
            "enum_specifier" => named("enum"),
            "class_specifier" => named("class"),
            "namespace_definition" => named("namespace"),
            _ => None,
        },
        LangId::Ruby => match kind {
            "method" => named("def"),
            "singleton_method" => {
                let n = node.child_by_field_name("name")?;
                Some((format!("def self.{}", text(n, src)), Some(n.id())))
            }
            "class" => named("class"),
            "module" => named("module"),
            _ => None,
        },
        LangId::Bash => match kind {
            "function_definition" => named("function"),
            _ => None,
        },
    }
}

/// Nearest enclosing declaration label for `node`, excluding the declaration whose
/// own name `node` is.
pub fn scope_of(lang: LangId, node: Node, src: &str) -> Option<String> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if let Some((label, name_id)) = scope_label(lang, n, src)
            && name_id != Some(node.id())
        {
            return Some(label);
        }
        cur = n.parent();
    }
    None
}
