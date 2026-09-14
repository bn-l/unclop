use anyhow::{Result, bail};
use serde_json::json;

use super::{Ctx, Output, rel_path};
use crate::state::{Item, State, Status};

fn reset(item: &mut Item) {
    item.status = Status::Pending;
    item.ticks.fill(false);
    item.was = None;
    item.changed_after_done = false;
}

pub fn run(
    ctx: &Ctx,
    state: &mut State,
    ids: &[String],
    changed: bool,
    files: &[String],
) -> Result<Output> {
    if ids.is_empty() && files.is_empty() && !changed {
        bail!("give item ids, --changed, or --file <path>");
    }
    let mut lines = Vec::new();
    let mut results = Vec::new();
    let mut code = 0;

    for query in ids {
        match state.resolve(query) {
            Ok((path, id)) => {
                let item = state.get_mut(&path, &id).expect("resolved item exists");
                reset(item);
                lines.push(format!("{}: pending again", item.id));
                results.push(json!({ "id": item.id, "outcome": "reopened" }));
            }
            Err(e) => {
                lines.push(format!("{query}: {e}"));
                code = 1;
            }
        }
    }

    if changed {
        let mut n = 0;
        for item in state.all_items_mut() {
            if item.status == Status::Done && item.changed_after_done {
                reset(item);
                results.push(json!({ "id": item.id, "outcome": "reopened" }));
                n += 1;
            }
        }
        lines.push(format!(
            "{n} items that changed after being marked done are pending again"
        ));
    }

    for given in files {
        let rel = rel_path(&ctx.root, given);
        match state.files.get_mut(&rel) {
            Some(entry) if entry.record.skipped => {
                entry.record.skipped = false;
                entry.record.mtime = 0;
                entry.record.size = 0;
                lines.push(format!(
                    "{rel}: file no longer skipped; its items will appear on the next scan"
                ));
                results.push(json!({ "file": rel, "outcome": "unskipped" }));
            }
            Some(_) => lines.push(format!("{rel}: was not skipped")),
            None => {
                lines.push(format!("{rel}: unknown file"));
                code = 1;
            }
        }
    }

    Ok(Output {
        lines,
        json: Some(json!({ "results": results })),
        code,
    })
}
