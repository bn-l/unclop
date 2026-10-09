use anyhow::{Result, bail};
use serde_json::json;

use super::{Ctx, Output, next, rel_path};
use crate::state::{State, Status, key_of};

pub fn run(ctx: &Ctx, state: &mut State, ids: &[String], files: &[String]) -> Result<Output> {
    if ids.is_empty() && files.is_empty() {
        bail!("give item ids, or --file <path> for whole files");
    }
    let mut lines = Vec::new();
    let mut results = Vec::new();
    let mut code = 0;
    let mut touched = Vec::new();

    for query in ids {
        match state.resolve(query) {
            Ok((path, id)) => {
                touched.push(key_of(&path, &id));
                let item = state.get_mut(&path, &id).expect("resolved item exists");
                item.status = Status::Skipped;
                item.alias = None;
                lines.push(format!("{}: skipped", item.id));
                results.push(json!({ "id": item.id, "outcome": "skipped" }));
            }
            Err(e) => {
                lines.push(format!("{query}: {e}"));
                code = 1;
            }
        }
    }

    for given in files {
        let rel = rel_path(&ctx.root, given);
        let exists = ctx.root.join(&rel).is_file();
        let entry = state.entry_mut(&rel);
        entry.record.skipped = true;
        touched.extend(entry.items.iter().map(|i| i.key()));
        let dropped = entry.items.len();
        entry.items.clear();
        if exists {
            lines.push(format!("{rel}: file skipped ({dropped} items dropped)"));
        } else {
            lines.push(format!("{rel}: file skipped (not found on disk right now)"));
        }
        results.push(json!({ "file": rel, "outcome": "skipped", "dropped": dropped }));
    }

    Ok(Output {
        lines,
        json: Some(json!({ "results": results })),
        code,
        then: next::continue_run(state, &touched),
    })
}
