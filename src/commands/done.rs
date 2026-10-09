use anyhow::Result;
use serde_json::json;

use super::{Ctx, Output, next};
use crate::state::{State, Status, key_of};

fn parse_rules(spec: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let n: usize = part
            .parse()
            .map_err(|_| anyhow::anyhow!("rules must be numbers, e.g. 1,2,3 (got {part:?})"))?;
        out.push(n);
    }
    if out.is_empty() {
        anyhow::bail!("list the rules you checked, e.g. 1,2,3");
    }
    Ok(out)
}

/// The chunk key an id was issued under. Called for ids that no longer exist in
/// state, so a match means the source was deleted.
fn issued_key(state: &State, query: &str) -> Option<String> {
    state.chunks.values().find_map(|c| {
        c.keys
            .iter()
            .find(|k| {
                k.split_once('#').is_some_and(|(_, id)| {
                    id == query || (query.len() >= 6 && id.starts_with(query))
                })
            })
            .cloned()
    })
}

pub fn run(ctx: &Ctx, state: &mut State, args: &[String], keep: bool) -> Result<Output> {
    let mut lines = Vec::new();
    let mut results = Vec::new();
    let mut code = 0;
    let mut touched = Vec::new();

    for arg in args {
        let Some((query, rules_spec)) = arg.split_once(':') else {
            lines.push(format!(
                "{arg}: list the rules you checked, e.g. {arg}:1,2,3"
            ));
            code = 1;
            continue;
        };
        let rules = match parse_rules(rules_spec) {
            Ok(r) => r,
            Err(e) => {
                lines.push(format!("{query}: {e}"));
                code = 1;
                continue;
            }
        };
        let (path, id) = match state.resolve(query) {
            Ok(found) => found,
            Err(e) => {
                if let Some(key) = issued_key(state, query) {
                    lines.push(format!("{query}: gone from source, counted as done"));
                    results.push(json!({ "id": query, "outcome": "gone" }));
                    touched.push(key);
                } else {
                    lines.push(format!("{query}: {e}"));
                    code = 1;
                }
                continue;
            }
        };

        let key = key_of(&path, &id);
        touched.push(key.clone());
        let snapshot = state
            .chunks
            .values()
            .find_map(|c| c.snapshot.get(&key).cloned());
        let rule_count = {
            let item = state.item_by_key(&key).expect("resolved item exists");
            ctx.config.rules.for_category(item.kind.category()).len()
        };
        let item = state.get_mut(&path, &id).expect("resolved item exists");

        if item.status == Status::Done {
            lines.push(format!("{}: already done", item.id));
            results.push(json!({ "id": item.id, "outcome": "already_done" }));
            continue;
        }
        if let Some(snap) = &snapshot
            && *snap == item.text
            && !keep
        {
            lines.push(format!(
                    "{}: unchanged since it was issued. Edit it, or pass --keep if it is fine as it is.",
                    item.id
                ));
            results.push(json!({ "id": item.id, "outcome": "unchanged" }));
            code = 1;
            continue;
        }

        let bad: Vec<String> = rules
            .iter()
            .filter(|r| **r == 0 || **r > rule_count)
            .map(|r| r.to_string())
            .collect();
        if !bad.is_empty() {
            lines.push(format!(
                "{}: no rule {} (this item has {rule_count})",
                item.id,
                bad.join(",")
            ));
            results.push(json!({ "id": item.id, "outcome": "bad_rule" }));
            code = 1;
            continue;
        }

        item.ticks.resize(rule_count, false);
        for r in &rules {
            item.ticks[r - 1] = true;
        }
        if item.ticks.iter().all(|t| *t) {
            item.status = Status::Done;
            item.alias = None;
            item.changed_after_done = false;
            lines.push(format!("{}: done", item.id));
            results.push(json!({ "id": item.id, "outcome": "done" }));
        } else {
            let remaining: Vec<String> = item
                .ticks
                .iter()
                .enumerate()
                .filter(|(_, t)| !**t)
                .map(|(i, _)| (i + 1).to_string())
                .collect();
            lines.push(format!(
                "{}: rules {} still unchecked",
                item.id,
                remaining.join(",")
            ));
            results.push(json!({ "id": item.id, "outcome": "partial", "remaining": remaining }));
        }
    }

    Ok(Output {
        lines,
        json: Some(json!({ "results": results })),
        code,
        then: next::continue_run(state, &touched),
    })
}
