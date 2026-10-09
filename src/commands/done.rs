use anyhow::Result;
use serde_json::json;

use super::{Ctx, Output, next};
use crate::state::{ResolveError, State, Status, key_of};

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
                // Only an id that matches nothing can belong to a deleted item; an
                // ambiguous one matches items that still exist.
                let gone = match e {
                    ResolveError::NotFound(_) => state.issued_key(query),
                    ResolveError::Ambiguous(_) => None,
                };
                if let Some(key) = gone {
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
            .chunk_holding(&key)
            .and_then(|c| c.snapshot.get(&key).cloned());
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
            // The alias stays: it reserves the id the chunk printed, so no new item
            // takes it while the agent may still type it.
            item.status = Status::Done;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{Config, Rules};
    use crate::ids::Kind;
    use crate::scan::lang::Registry;
    use crate::state::{Chunk, Item};

    #[test]
    fn the_latest_chunk_supplies_the_snapshot() {
        let ctx = Ctx {
            root: PathBuf::new(),
            workdir: PathBuf::new(),
            config: Config {
                rules: Rules {
                    identifier: vec!["a".into()],
                    ..Rules::default()
                },
                ..Config::default()
            },
            registry: Registry::default(),
        };
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![Item {
            path: "a.rs".into(),
            id: "aaaaaaaaaa".into(),
            kind: Kind::Fn,
            text: "new_name".into(),
            scope: None,
            line: [1, 1],
            status: Status::Pending,
            ticks: vec![false],
            was: None,
            changed_after_done: false,
            alias: None,
            ctx: None,
        }];
        // A chunk from an earlier one-worker run, then the current chunk of worker 1/2.
        for (worker, issued, text) in [("1/1", 1, "old_name"), ("1/2", 2, "new_name")] {
            state.chunks.insert(
                worker.into(),
                Chunk {
                    worker: worker.into(),
                    keys: vec!["a.rs#aaaaaaaaaa".into()],
                    issued,
                    snapshot: [("a.rs#aaaaaaaaaa".to_string(), text.to_string())].into(),
                    only: None,
                },
            );
        }
        let out = run(&ctx, &mut state, &["aaaaaaaaaa:1".to_string()], false).unwrap();
        assert!(out.lines[0].contains("unchanged"), "{:?}", out.lines);
        assert!(state.files["a.rs"].items[0].is_pending());
    }
}
