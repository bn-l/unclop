use anyhow::Result;
use serde_json::json;

use super::{Ctx, Output};
use crate::state::{Counts, State};

pub fn run(_ctx: &Ctx, state: &State) -> Result<Output> {
    let width = state
        .files
        .keys()
        .map(|p| p.len())
        .max()
        .unwrap_or(4)
        .clamp(4, 70);
    let mut lines = vec![format!(
        "{:<width$}  {:>7} {:>6} {:>7}",
        "file", "pending", "done", "skipped"
    )];
    let mut files_json = Vec::new();

    for (path, entry) in &state.files {
        if entry.record.skipped {
            lines.push(format!(
                "{:<width$}  {:>7} {:>6} {:>7}",
                path, "-", "-", "file"
            ));
            files_json.push(json!({ "path": path, "skipped_file": true }));
            continue;
        }
        let mut c = Counts::default();
        for item in &entry.items {
            c.add(item);
        }
        if c.pending + c.done + c.skipped == 0 {
            continue;
        }
        lines.push(format!(
            "{:<width$}  {:>7} {:>6} {:>7}",
            path, c.pending, c.done, c.skipped
        ));
        files_json.push(json!({
            "path": path,
            "pending": c.pending,
            "done": c.done,
            "skipped": c.skipped,
        }));
    }

    lines.push(String::new());
    let mut by_cat = serde_json::Map::new();
    for (cat, c) in state.counts_by_category() {
        lines.push(format!(
            "{:<12} {:>7} pending {:>6} done {:>7} skipped",
            cat.label(),
            c.pending,
            c.done,
            c.skipped
        ));
        by_cat.insert(
            cat.label().to_string(),
            json!({ "pending": c.pending, "done": c.done, "skipped": c.skipped }),
        );
    }
    let total = state.counts();
    lines.push(format!(
        "{:<12} {:>7} pending {:>6} done {:>7} skipped",
        "total", total.pending, total.done, total.skipped
    ));

    Ok(Output {
        lines,
        json: Some(json!({
            "files": files_json,
            "by_category": by_cat,
            "totals": { "pending": total.pending, "done": total.done, "skipped": total.skipped },
        })),
        code: if total.pending > 0 { 1 } else { 0 },
    })
}
