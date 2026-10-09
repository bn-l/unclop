use anyhow::Result;
use serde_json::json;

use super::{Ctx, Output, preview};
use crate::ids::Category;
use crate::manifest::item_json;
use crate::state::{Item, State, Status};

fn shown(item: &Item, text: &str) -> String {
    if item.kind.category() == Category::String {
        format!("\"{}\"", preview(text))
    } else {
        preview(text)
    }
}

pub fn run(ctx: &Ctx, state: &State) -> Result<Output> {
    let mut lines = Vec::new();
    let mut files_json = Vec::new();
    let mut skipped_files = Vec::new();
    let (mut rewritten, mut kept, mut skipped, mut pending) = (0usize, 0usize, 0usize, 0usize);

    for (path, entry) in &state.files {
        if entry.record.skipped {
            skipped_files.push(path.clone());
            continue;
        }
        pending += entry.items.iter().filter(|i| i.is_pending()).count();
        let settled: Vec<&Item> = entry.items.iter().filter(|i| !i.is_pending()).collect();
        if settled.is_empty() {
            continue;
        }
        lines.push(format!("## {path}"));
        let mut items_json = Vec::new();
        for item in settled {
            match (item.status, &item.was) {
                // `was` survives a reopen; an edit back to that text is not a rewrite.
                (Status::Done, Some(was)) if *was != item.text => {
                    rewritten += 1;
                    lines.push(format!(
                        "  {:<8} {}",
                        item.kind.label(),
                        shown(item, &item.text)
                    ));
                    lines.push(format!("  {:<8} was: {}", "", shown(item, was)));
                }
                (Status::Done, _) => {
                    kept += 1;
                    lines.push(format!(
                        "  {:<8} kept: {}",
                        item.kind.label(),
                        shown(item, &item.text)
                    ));
                }
                (Status::Skipped, _) => {
                    skipped += 1;
                    lines.push(format!(
                        "  {:<8} skipped: {}",
                        item.kind.label(),
                        shown(item, &item.text)
                    ));
                }
                (Status::Pending, _) => {}
            }
            items_json.push(item_json(&ctx.config, item));
        }
        lines.push(String::new());
        files_json.push(json!({ "path": path, "items": items_json }));
    }

    if !skipped_files.is_empty() {
        lines.push("Skipped files:".to_string());
        for f in &skipped_files {
            lines.push(format!("  {f}"));
        }
        lines.push(String::new());
    }

    lines.insert(
        0,
        format!(
            "{} done ({rewritten} rewritten, {kept} kept as they were), {skipped} skipped, {pending} pending, {} files skipped",
            rewritten + kept,
            skipped_files.len()
        ),
    );
    lines.insert(1, String::new());

    Ok(Output {
        lines,
        json: Some(json!({
            "summary": {
                "rewritten": rewritten,
                "kept": kept,
                "skipped": skipped,
                "pending": pending,
                "skipped_files": skipped_files,
            },
            "files": files_json,
        })),
        code: 0,
        then: None,
    })
}
