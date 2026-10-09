use anyhow::{Result, bail};
use serde_json::json;

use super::{Ctx, Output, next, rel_path};
use crate::state::{ResolveError, State, Status, key_of};

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
                lines.push(format!("{}: skipped", item.id));
                results.push(json!({ "id": item.id, "outcome": "skipped" }));
            }
            Err(e) => {
                let gone = match e {
                    ResolveError::NotFound(_) => state.issued_key(query),
                    ResolveError::Ambiguous(_) => None,
                };
                if let Some(key) = gone {
                    lines.push(format!("{query}: gone from source, nothing to skip"));
                    results.push(json!({ "id": query, "outcome": "gone" }));
                    touched.push(key);
                } else {
                    lines.push(format!("{query}: {e}"));
                    code = 1;
                }
            }
        }
    }

    for given in files {
        let rel = match rel_path(&ctx.root, &ctx.workdir, given) {
            Ok(rel) => rel,
            Err(e) => {
                lines.push(format!("{given}: {e:#}"));
                code = 1;
                continue;
            }
        };
        let abs = ctx.root.join(&rel);
        if let Some(entry) = state.files.get_mut(&rel) {
            entry.record.skipped = true;
            touched.extend(entry.items.iter().map(|i| i.key()));
            let dropped = entry.items.len();
            entry.items.clear();
            lines.push(format!("{rel}: file skipped ({dropped} items dropped)"));
            results.push(json!({ "file": rel, "outcome": "skipped", "dropped": dropped }));
        } else if abs.exists() || ctx.registry.for_path(&abs).is_none() {
            // On disk but not in state: an unsupported language, ignored, over the
            // size limit or not UTF-8. Recording a skip for it would do nothing.
            lines.push(format!(
                "{rel}: unclop does not review this file, so there is nothing to skip"
            ));
            results.push(json!({ "file": rel, "outcome": "not_reviewed" }));
            code = 1;
        } else {
            state.entry_mut(&rel).record.skipped = true;
            lines.push(format!(
                "{rel}: file skipped (not on disk yet; it stays skipped when it appears)"
            ));
            results.push(json!({ "file": rel, "outcome": "skipped", "dropped": 0 }));
        }
    }

    Ok(Output {
        lines,
        json: Some(json!({ "results": results })),
        code,
        then: next::continue_run(state, &touched),
    })
}
