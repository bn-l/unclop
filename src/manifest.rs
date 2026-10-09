//! Rendering of a chunk for the agent, as text or JSON.

use serde_json::{Value, json};

use crate::config::Config;
use crate::ids::Category;
use crate::state::Item;

pub struct ChunkFile<'a> {
    pub path: &'a str,
    pub pending_in_file: usize,
    pub items: Vec<&'a Item>,
}

pub const PREVIEW_CHARS: usize = 100;

pub fn line_span(item: &Item) -> String {
    if item.line[0] == item.line[1] {
        format!("L{}", item.line[0])
    } else {
        format!("L{}-{}", item.line[0], item.line[1])
    }
}

pub fn display_text(item: &Item) -> String {
    let mut t: String = item.text.chars().take(PREVIEW_CHARS).collect();
    if item.text.chars().count() > PREVIEW_CHARS {
        t.push('…');
    }
    if item.kind.category() == Category::String {
        format!("\"{t}\"")
    } else {
        t
    }
}

/// `1,2,3` for the rule list that applies to this item.
pub fn rules_arg(config: &Config, item: &Item) -> String {
    let n = config.rules.for_category(item.kind.category()).len();
    (1..=n).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
}

pub fn item_line(item: &Item) -> String {
    let mut line = format!(
        "  {:<13} {:<8} {:<9} {}",
        item.id,
        item.kind.label(),
        line_span(item),
        display_text(item)
    );
    if let Some(scope) = &item.scope {
        line.push_str(&format!("  in {scope}"));
    }
    line
}

pub fn render_text(
    config: &Config,
    files: &[ChunkFile],
    total_pending: usize,
    only: Option<Category>,
) -> String {
    let count: usize = files.iter().map(|f| f.items.len()).sum();
    let mut out = String::new();
    out.push_str(&format!(
        "unclop · {count} items in this chunk · {total_pending} pending overall\n\n"
    ));
    out.push_str(&config.prompt);
    out.push_str("\n\n");
    if let Some(notes) = &config.notes {
        out.push_str(notes);
        out.push_str("\n\n");
    }

    let present: Vec<Category> = Category::ALL
        .into_iter()
        .filter(|c| {
            files
                .iter()
                .any(|f| f.items.iter().any(|i| i.kind.category() == *c))
        })
        .collect();
    for cat in &present {
        let rules = config.rules.for_category(*cat);
        if rules.is_empty() {
            continue;
        }
        out.push_str(&format!("Rules for {}:\n", cat.label()));
        for (i, rule) in rules.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, rule));
        }
    }
    out.push('\n');

    let pending = match only {
        Some(c) => format!("pending {}", c.label()),
        None => "pending".to_string(),
    };
    for file in files {
        out.push_str(&format!(
            "## {}  ({} of {} {pending} in this file)\n\n",
            file.path,
            file.items.len(),
            file.pending_in_file
        ));
        for item in &file.items {
            out.push_str(&item_line(item));
            out.push('\n');
        }
        out.push('\n');
    }

    out.push_str("Mark each item when finished, listing every rule you checked:\n");
    let all: Vec<&Item> = files.iter().flat_map(|f| f.items.iter().copied()).collect();
    for group in all.chunks(8) {
        let args: Vec<String> = group
            .iter()
            .map(|i| format!("{}:{}", i.id, rules_arg(config, i)))
            .collect();
        out.push_str(&format!("  unclop done {}\n", args.join(" ")));
    }
    out.push_str(
        "If an item is fine as it is, add --keep. Skip names or text dictated from outside, or whole generated files:\n",
    );
    out.push_str("  unclop skip <id>...        unclop skip --file <path>\n");
    out
}

pub fn item_json(config: &Config, item: &Item) -> Value {
    json!({
        "id": item.id,
        "key": item.key(),
        "path": item.path,
        "kind": item.kind.label(),
        "category": item.kind.category().label(),
        "line": item.line,
        "text": item.text,
        "scope": item.scope,
        "status": item.status,
        "ticks": item.ticks,
        "was": item.was,
        "rules": rules_arg(config, item),
    })
}

pub fn render_json(
    config: &Config,
    files: &[ChunkFile],
    total_pending: usize,
    only: Option<Category>,
) -> Value {
    let files_json: Vec<Value> = files
        .iter()
        .map(|f| {
            json!({
                "path": f.path,
                "pending_in_file": f.pending_in_file,
                "items": f.items.iter().map(|i| item_json(config, i)).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "prompt": config.prompt,
        "notes": config.notes,
        "rules": {
            "identifier": config.rules.identifier,
            "comment": config.rules.comment,
            "string": config.rules.string,
        },
        "total_pending": total_pending,
        "only": only,
        "files": files_json,
    })
}
