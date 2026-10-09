use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use serde_json::json;
use xxhash_rust::xxh3::xxh3_64;

use super::{Ctx, Output};
use crate::ids::Category;
use crate::manifest::{self, ChunkFile, item_json, item_line};
use crate::state::{Chunk, Item, State};

pub fn parse_worker(spec: Option<&str>) -> Result<(usize, usize)> {
    let Some(spec) = spec else {
        return Ok((1, 1));
    };
    let Some((i, n)) = spec.split_once('/') else {
        bail!("--worker takes I/N, e.g. 2/4");
    };
    let i: usize = i
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("--worker takes I/N, e.g. 2/4"))?;
    let n: usize = n
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("--worker takes I/N, e.g. 2/4"))?;
    if n == 0 || i == 0 || i > n {
        bail!("--worker I/N needs 1 <= I <= N");
    }
    Ok((i, n))
}

pub fn assigned(path: &str, worker: usize, workers: usize) -> bool {
    workers == 1 || (xxh3_64(path.as_bytes()) % workers as u64) as usize == worker - 1
}

pub fn run(
    ctx: &Ctx,
    state: &mut State,
    worker: Option<&str>,
    only: Option<Category>,
    force: bool,
) -> Result<Output> {
    let (wi, wn) = parse_worker(worker)?;
    let wkey = format!("{wi}/{wn}");

    if !force && let Some(chunk) = state.chunks.get(&wkey) {
        let unfinished: Vec<Item> = chunk
            .keys
            .iter()
            .filter_map(|k| state.item_by_key(k))
            .filter(|i| i.is_pending())
            .cloned()
            .collect();
        if !unfinished.is_empty() {
            let mut lines = vec![format!(
                "Unfinished from your last chunk ({} of {}):",
                unfinished.len(),
                chunk.keys.len()
            )];
            lines.extend(unfinished.iter().map(item_line));
            lines.push(String::new());
            lines.push(
                    "Mark them with `unclop done <id>:<rules>` or `unclop skip <id>`, or pass --force to move on."
                        .to_string(),
                );
            let json = json!({
                "unfinished": unfinished.iter().map(|i| item_json(&ctx.config, i)).collect::<Vec<_>>(),
            });
            return Ok(Output {
                lines,
                json: Some(json),
                code: 2,
            });
        }
    }

    let chunk_size = ctx.config.chunk_size;
    let mut files: Vec<ChunkFile> = Vec::new();
    let mut count = 0usize;
    for (path, entry) in &state.files {
        if entry.record.skipped || !assigned(path, wi, wn) {
            continue;
        }
        let mut pending: Vec<&Item> = entry
            .items
            .iter()
            .filter(|i| i.is_pending() && only.is_none_or(|c| i.kind.category() == c))
            .collect();
        if pending.is_empty() {
            continue;
        }
        pending.sort_by_key(|i| (i.kind.category(), i.line[0], i.line[1]));
        let n = pending.len();
        if files.is_empty() {
            let take = n.min(chunk_size);
            pending.truncate(take);
            files.push(ChunkFile {
                path,
                pending_in_file: n,
                items: pending,
            });
            count += take;
            if n > chunk_size {
                break;
            }
        } else if count + n > chunk_size {
            break;
        } else {
            files.push(ChunkFile {
                path,
                pending_in_file: n,
                items: pending,
            });
            count += n;
        }
    }

    let (total_pending, _) = state.pending();
    if files.is_empty() {
        let lines = if total_pending == 0 {
            vec!["Nothing pending. Run: unclop report".to_string()]
        } else if let Some(cat) = only {
            let in_cat = state
                .files
                .values()
                .flat_map(|e| &e.items)
                .filter(|i| i.is_pending() && i.kind.category() == cat)
                .count();
            vec![if in_cat == 0 {
                format!(
                    "No {} pending. {total_pending} items remain in other categories.",
                    cat.label()
                )
            } else {
                format!(
                    "No {} pending for worker {wkey}. {in_cat} remain for other workers.",
                    cat.label()
                )
            }]
        } else {
            vec![format!(
                "Nothing pending for worker {wkey}. {total_pending} items remain for other workers."
            )]
        };
        state.chunks.remove(&wkey);
        return Ok(Output {
            lines,
            json: Some(json!({ "files": [], "total_pending": total_pending, "only": only })),
            code: 0,
        });
    }

    let keys: Vec<String> = files
        .iter()
        .flat_map(|f| f.items.iter().map(|i| i.key()))
        .collect();
    let snapshot: BTreeMap<String, String> = files
        .iter()
        .flat_map(|f| f.items.iter().map(|i| (i.key(), i.text.clone())))
        .collect();
    let text = manifest::render_text(&ctx.config, &files, total_pending, only);
    let json = manifest::render_json(&ctx.config, &files, total_pending, only);
    drop(files);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state.chunks.insert(
        wkey.clone(),
        Chunk {
            worker: wkey,
            keys: keys.clone(),
            issued: now,
            snapshot,
        },
    );
    for key in &keys {
        if let Some((path, id)) = key.split_once('#')
            && let Some(item) = state.get_mut(path, id)
        {
            item.alias = None;
        }
    }

    Ok(Output {
        lines: text.lines().map(String::from).collect(),
        json: Some(json),
        code: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_spec() {
        assert_eq!(parse_worker(None).unwrap(), (1, 1));
        assert_eq!(parse_worker(Some("2/4")).unwrap(), (2, 4));
        assert!(parse_worker(Some("0/4")).is_err());
        assert!(parse_worker(Some("5/4")).is_err());
        assert!(parse_worker(Some("x")).is_err());
    }

    #[test]
    fn workers_partition_paths() {
        let paths = [
            "a.rs", "b/c.rs", "d.py", "e.go", "f.ts", "g.rb", "h.sh", "i.c",
        ];
        for p in paths {
            let owners: Vec<usize> = (1..=3).filter(|w| assigned(p, *w, 3)).collect();
            assert_eq!(owners.len(), 1, "{p} must belong to exactly one worker");
        }
    }
}
