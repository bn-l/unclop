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

/// Printed after the prompt in every chunk. It lives in the binary rather than the config
/// so that an agent copying or editing the config cannot drop it. The items are bullets,
/// not numbers, so they are not mistaken for the rule numbers `done` takes.
pub const USAGE_RULES: &str = "\
Rules for using unclop. The user set these and they are not negotiable:
  - Work in a loop: run `unclop next`, handle every item in the batch, run the `unclop done` command printed at the end, then run the command on the `then:` line. Repeat until `unclop next` prints \"Nothing pending\".
  - Read each item in its source and decide its new name or text yourself, one item at a time. Never write or run a script, regex or name mapping that decides names or text for many items at once. You may use a rename tool, such as your editor's or language server's rename, to apply a name you chose to every reference.
  - Use unclop only through the commands it prints. Do not read or edit .unclop.jsonl, do not edit or copy the config file, do not pass --config or --force, and do not set UNCLOP_CONFIG. Do not create or edit .unclop.yaml in the project root unless the user has specifically asked for it. The batch size and the rules are the user's choice.
";

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

/// One item as a chunk lists it, under `id`.
pub fn item_line(item: &Item, id: &str) -> String {
    let mut line = format!(
        "  {:<13} {:<8} {:<9} {}",
        id,
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
    out.push_str(USAGE_RULES);
    out.push('\n');

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
        if let Some(prompt) = config.prompts.get(cat) {
            out.push_str(prompt);
            out.push_str("\n\n");
        }
        out.push_str(&format!("Rules for {}:\n", cat.label()));
        for (i, rule) in rules.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, rule));
        }
        out.push('\n');
    }

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
            // A new chunk shows current ids; `next` clears the aliases after rendering.
            out.push_str(&item_line(item, &item.id));
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
        "prompts": config.prompts,
        "notes": config.notes,
        "usage_rules": USAGE_RULES,
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
