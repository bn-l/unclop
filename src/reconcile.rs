//! Carries state from the previous scan onto the new one.
//!
//! Per file, the old and new id sequences are diffed without the `~n` ordinal.
//! Equal runs keep their state and their ids. Inside a replace hunk, old and new
//! items are paired positionally while their kinds match and the state carries
//! across the rewrite. Items left over on the old side are matched against
//! leftover new items in other files by id without the ordinal (moves). Anything
//! still unmatched on the old side is gone from source.
//!
//! Ids are unique across the project and an item keeps its id until its text
//! changes. New and rewritten items get the next free ordinal for their content,
//! skipping ids that an item, an alias or a chunk still refers to, so a printed id
//! never comes to mean a different item.

use std::collections::{BTreeMap, HashMap, HashSet};

use similar::{Algorithm, DiffOp, capture_diff_slices};

use crate::config::Rules;
use crate::scan::Scanned;
use crate::state::{Item, State, Status, split_key};

pub fn reconcile(state: &mut State, scanned: BTreeMap<String, Scanned>, rules: &Rules) {
    let mut renames: HashMap<String, String> = HashMap::new();
    let mut orphans: Vec<Item> = Vec::new();
    // New and rewritten items, by path and position, with the key a rewritten item
    // had before. They get their ids once every kept id is known.
    let mut unassigned: Vec<(String, usize, Option<String>)> = Vec::new();

    // A skipped file keeps its record while it is missing, so a file skipped
    // before it is generated stays skipped once it appears.
    let gone: Vec<String> = state
        .files
        .iter()
        .filter(|(p, e)| !scanned.contains_key(*p) && !e.record.skipped)
        .map(|(p, _)| p.clone())
        .collect();
    for path in gone {
        if let Some(entry) = state.files.remove(&path) {
            orphans.extend(entry.items);
        }
    }

    for (path, scanned) in scanned {
        let Scanned::Parsed(file) = scanned else {
            continue;
        };
        let entry = state.entry_mut(&path);
        entry.record.mtime = file.mtime;
        entry.record.size = file.size;
        if entry.record.skipped {
            entry.items.clear();
            continue;
        }

        let old = std::mem::take(&mut entry.items);
        let new = file.items;
        let old_ids: Vec<&str> = old.iter().map(|i| content_id(&i.id)).collect();
        let new_ids: Vec<&str> = new.iter().map(|i| content_id(&i.id)).collect();
        let ops = capture_diff_slices(Algorithm::Myers, &old_ids, &new_ids);

        // For each new item: the old item it continues and whether its text changed.
        let mut paired: Vec<Option<(usize, bool)>> = vec![None; new.len()];
        let mut taken = vec![false; old.len()];
        for op in ops {
            match op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => {
                    for k in 0..len {
                        paired[new_index + k] = Some((old_index + k, false));
                        taken[old_index + k] = true;
                    }
                }
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    // Pair each new item with the first unused old item of the
                    // same kind in the hunk, so a deleted comment next to a
                    // renamed variable does not break the rename pairing.
                    for k in 0..new_len {
                        let n = &new[new_index + k];
                        let found = (0..old_len)
                            .map(|j| old_index + j)
                            .find(|&j| !taken[j] && old[j].kind == n.kind);
                        if let Some(j) = found {
                            paired[new_index + k] = Some((j, true));
                            taken[j] = true;
                        }
                    }
                }
                DiffOp::Insert { .. } | DiffOp::Delete { .. } => {}
            }
        }
        repair_identical(&old, &new, &mut paired, &mut taken);

        let mut items = Vec::with_capacity(new.len());
        for (i, n) in new.iter().enumerate() {
            match paired[i] {
                Some((j, false)) => {
                    let mut item = carry(&old[j], n, false);
                    item.id = old[j].id.clone();
                    items.push(item);
                }
                Some((j, true)) => {
                    unassigned.push((path.clone(), i, Some(old[j].key())));
                    items.push(carry(&old[j], n, true));
                }
                None => {
                    unassigned.push((path.clone(), i, None));
                    items.push(n.clone());
                }
            }
        }
        for (o, t) in old.into_iter().zip(taken) {
            if !t {
                orphans.push(o);
            }
        }
        entry.items = items;
    }

    // Moves: an orphan whose content shows up once among new items in another file.
    let mut orphans_by_id: HashMap<String, Vec<Item>> = HashMap::new();
    for o in orphans {
        orphans_by_id
            .entry(content_id(&o.id).to_string())
            .or_default()
            .push(o);
    }
    let new_content = |state: &State, path: &str, i: usize| -> String {
        content_id(&state.files[path].items[i].id).to_string()
    };
    let mut fresh_count: HashMap<String, usize> = HashMap::new();
    for (path, i, before) in &unassigned {
        if before.is_none() {
            *fresh_count.entry(new_content(state, path, *i)).or_default() += 1;
        }
    }
    unassigned.retain(|(path, i, before)| {
        if before.is_some() {
            return true;
        }
        let content = new_content(state, path, *i);
        if fresh_count.get(&content) != Some(&1) {
            return true;
        }
        let Some(candidates) = orphans_by_id.get_mut(&content) else {
            return true;
        };
        let item = &mut state.files.get_mut(path).unwrap().items[*i];
        if candidates.len() != 1 || candidates[0].kind != item.kind {
            return true;
        }
        let o = candidates.pop().unwrap();
        *item = carry(&o, item, false);
        item.id = o.id.clone();
        renames.insert(o.key(), item.key());
        false
    });

    // Ids in use: every kept item's id, every alias and every id a chunk holds,
    // including chunk ids of deleted items that `done` reports as gone.
    let pending_ids: HashSet<(String, usize)> = unassigned
        .iter()
        .map(|(path, i, _)| (path.clone(), *i))
        .collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut duplicates: Vec<(String, usize, Option<String>)> = Vec::new();
    for (path, entry) in &state.files {
        for (i, item) in entry.items.iter().enumerate() {
            if pending_ids.contains(&(path.clone(), i)) {
                continue;
            }
            // State written by older versions repeats ids across files.
            if !used.insert(item.id.clone()) {
                duplicates.push((path.clone(), i, Some(item.key())));
            }
        }
    }
    used.extend(state.all_items().filter_map(|i| i.alias.clone()));
    used.extend(
        state
            .chunks
            .values()
            .flat_map(|c| c.keys.iter())
            .filter_map(|k| split_key(k).map(|(_, id)| id.to_string())),
    );
    for (path, i, before) in duplicates.into_iter().chain(unassigned) {
        let item = &mut state.files.get_mut(&path).unwrap().items[i];
        item.id = allocate(content_id(&item.id), &mut used);
        if let Some(before) = before {
            renames.insert(before, item.key());
        }
    }

    for item in state.all_items_mut() {
        let n = rules.for_category(item.kind.category()).len();
        item.ticks.resize(n, false);
    }

    if !renames.is_empty() {
        for chunk in state.chunks.values_mut() {
            for key in &mut chunk.keys {
                if let Some(new_key) = renames.get(key) {
                    *key = new_key.clone();
                }
            }
            let snapshot = std::mem::take(&mut chunk.snapshot);
            chunk.snapshot = snapshot
                .into_iter()
                .map(|(k, v)| (renames.get(&k).cloned().unwrap_or(k), v))
                .collect();
        }
    }
}

/// The diff pairs identical items in order, so when one of several identical
/// items in a file is deleted or added with no other item between them, the
/// survivors can be paired with the wrong copies. Among unchanged items of one
/// content, a new item whose context matches exactly one old copy is paired with
/// that copy. The rest keep the pair the diff found if that copy is still free,
/// then pair up in order.
fn repair_identical(
    old: &[Item],
    new: &[Item],
    paired: &mut [Option<(usize, bool)>],
    taken: &mut [bool],
) {
    let mut by_content: HashMap<&str, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (j, o) in old.iter().enumerate() {
        // Old items rewritten into something else are not copies any more.
        let rewritten = paired.contains(&Some((j, true)));
        if !rewritten {
            by_content.entry(content_id(&o.id)).or_default().0.push(j);
        }
    }
    for (i, n) in new.iter().enumerate() {
        if !matches!(paired[i], Some((_, true))) {
            by_content.entry(content_id(&n.id)).or_default().1.push(i);
        }
    }
    for (olds, news) in by_content.into_values() {
        if olds.len() == news.len() || olds.is_empty() || news.is_empty() {
            continue;
        }
        let found: Vec<Option<usize>> = news.iter().map(|&i| paired[i].map(|(j, _)| j)).collect();
        for &i in &news {
            paired[i] = None;
        }
        for &j in &olds {
            taken[j] = false;
        }
        for &i in &news {
            let Some(ctx) = new[i].ctx else { continue };
            let mut same = olds
                .iter()
                .filter(|&&j| !taken[j] && old[j].ctx == Some(ctx));
            if let (Some(&j), None) = (same.next(), same.next()) {
                paired[i] = Some((j, false));
                taken[j] = true;
            }
        }
        for (&i, j) in news.iter().zip(found) {
            if paired[i].is_none()
                && let Some(j) = j
                && !taken[j]
            {
                paired[i] = Some((j, false));
                taken[j] = true;
            }
        }
        let mut free = olds
            .iter()
            .filter(|&&j| !taken[j])
            .copied()
            .collect::<Vec<_>>()
            .into_iter();
        for &i in &news {
            if paired[i].is_none()
                && let Some(j) = free.next()
            {
                paired[i] = Some((j, false));
                taken[j] = true;
            }
        }
    }
}

/// `content`, or `content~2`, `content~3`, ... whichever is first not in `used`.
fn allocate(content: &str, used: &mut HashSet<String>) -> String {
    let mut id = content.to_string();
    let mut n = 1;
    while used.contains(&id) {
        n += 1;
        id = format!("{content}~{n}");
    }
    used.insert(id.clone());
    id
}

/// An id without its `~n` ordinal: the hash of the item's content.
fn content_id(id: &str) -> &str {
    id.split_once('~').map_or(id, |(hash, _)| hash)
}

fn carry(old: &Item, new: &Item, changed: bool) -> Item {
    let mut item = new.clone();
    item.status = old.status;
    item.ticks = old.ticks.clone();
    item.was = old.was.clone();
    item.changed_after_done = old.changed_after_done;
    item.alias = old.alias.clone();
    if changed {
        if item.was.is_none() {
            item.was = Some(old.text.clone());
        }
        if item.alias.is_none() {
            item.alias = Some(old.id.clone());
        }
        if old.status == Status::Done {
            item.changed_after_done = true;
        }
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Kind;
    use crate::scan::ScanFile;
    use crate::state::Chunk;

    fn rules() -> Rules {
        Rules {
            comment: vec!["a".into(), "b".into()],
            identifier: vec!["a".into(), "b".into(), "c".into()],
            string: vec!["a".into()],
        }
    }

    fn item(path: &str, id: &str, kind: Kind, text: &str, line: u32) -> Item {
        Item {
            path: path.into(),
            id: id.into(),
            kind,
            text: text.into(),
            scope: None,
            line: [line, line],
            status: Status::Pending,
            ticks: Vec::new(),
            was: None,
            changed_after_done: false,
            alias: None,
            ctx: None,
        }
    }

    fn done(mut i: Item) -> Item {
        i.status = Status::Done;
        i.ticks = vec![true, true, true];
        i
    }

    fn parsed(items: Vec<Item>) -> Scanned {
        Scanned::Parsed(ScanFile {
            mtime: 1,
            size: 1,
            items,
        })
    }

    #[test]
    fn shifted_lines_keep_state() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            done(item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1)),
            item("a.rs", "bbbbbbbbbb", Kind::Fn, "bar", 5),
        ];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![
                item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 10),
                item("a.rs", "bbbbbbbbbb", Kind::Fn, "bar", 15),
            ]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let items = &state.files["a.rs"].items;
        assert_eq!(items[0].status, Status::Done);
        assert_eq!(items[0].line, [10, 10]);
        assert_eq!(items[1].status, Status::Pending);
        assert_eq!(items[1].ticks.len(), 3);
    }

    #[test]
    fn rename_pairs_and_records_was() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1),
            done(item("a.rs", "bbbbbbbbbb", Kind::Fn, "processedResult", 5)),
            item("a.rs", "cccccccccc", Kind::Fn, "baz", 9),
        ];
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec!["a.rs#bbbbbbbbbb".into()],
                issued: 0,
                snapshot: [("a.rs#bbbbbbbbbb".to_string(), "processedResult".to_string())].into(),
                only: None,
            },
        );
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![
                item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1),
                item("a.rs", "dddddddddd", Kind::Fn, "result", 5),
                item("a.rs", "cccccccccc", Kind::Fn, "baz", 9),
            ]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let renamed = &state.files["a.rs"].items[1];
        assert_eq!(renamed.id, "dddddddddd");
        assert_eq!(renamed.status, Status::Done);
        assert_eq!(renamed.was.as_deref(), Some("processedResult"));
        assert_eq!(renamed.alias.as_deref(), Some("bbbbbbbbbb"));
        assert!(renamed.changed_after_done);
        assert_eq!(state.chunks["1/1"].keys, vec!["a.rs#dddddddddd"]);
        assert_eq!(
            state.chunks["1/1"].snapshot["a.rs#dddddddddd"],
            "processedResult"
        );
        assert_eq!(state.resolve("bbbbbbbbbb").unwrap().1, "dddddddddd");
    }

    #[test]
    fn deleted_neighbor_does_not_break_rename_pairing() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1),
            item(
                "a.rs",
                "cccccccccc",
                Kind::Comment,
                "leverage the helper",
                3,
            ),
            item("a.rs", "bbbbbbbbbb", Kind::Var, "processed_result", 4),
            item("a.rs", "eeeeeeeeee", Kind::Var, "message", 5),
        ];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![
                item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1),
                item("a.rs", "dddddddddd", Kind::Var, "result", 3),
                item("a.rs", "eeeeeeeeee", Kind::Var, "message", 4),
            ]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let items = &state.files["a.rs"].items;
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].id, "dddddddddd");
        assert_eq!(items[1].alias.as_deref(), Some("bbbbbbbbbb"));
        assert_eq!(items[1].was.as_deref(), Some("processed_result"));
        assert!(state.resolve("cccccccccc").is_err());
    }

    #[test]
    fn kind_mismatch_is_not_paired() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![done(item(
            "a.rs",
            "aaaaaaaaaa",
            Kind::Comment,
            "old note",
            1,
        ))];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![item("a.rs", "bbbbbbbbbb", Kind::Fn, "thing", 1)]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let items = &state.files["a.rs"].items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, Status::Pending);
        assert!(items[0].was.is_none());
    }

    #[test]
    fn deleted_items_disappear() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            done(item("a.rs", "aaaaaaaaaa", Kind::Comment, "gone", 1)),
            item("a.rs", "bbbbbbbbbb", Kind::Fn, "bar", 5),
        ];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![item("a.rs", "bbbbbbbbbb", Kind::Fn, "bar", 4)]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert_eq!(state.files["a.rs"].items.len(), 1);
        assert_eq!(state.files["a.rs"].items[0].id, "bbbbbbbbbb");
    }

    #[test]
    fn move_across_files_keeps_state() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![done(item("a.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1))];
        let scanned = [
            ("a.rs".to_string(), parsed(vec![])),
            (
                "b.rs".to_string(),
                parsed(vec![item("b.rs", "aaaaaaaaaa", Kind::Fn, "foo", 3)]),
            ),
        ]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert!(state.files["a.rs"].items.is_empty());
        let moved = &state.files["b.rs"].items[0];
        assert_eq!(moved.status, Status::Done);
        assert_eq!(moved.path, "b.rs");
    }

    #[test]
    fn renamed_file_keeps_state() {
        let mut state = State::default();
        state.entry_mut("old.rs").items =
            vec![done(item("old.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1))];
        let scanned = [(
            "new.rs".to_string(),
            parsed(vec![item("new.rs", "aaaaaaaaaa", Kind::Fn, "foo", 1)]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert!(!state.files.contains_key("old.rs"));
        assert_eq!(state.files["new.rs"].items[0].status, Status::Done);
    }

    #[test]
    fn unchanged_file_is_left_alone_and_ticks_resize() {
        let mut state = State::default();
        let mut it = item("a.rs", "aaaaaaaaaa", Kind::Comment, "x", 1);
        it.ticks = vec![true];
        state.entry_mut("a.rs").items = vec![it];
        let scanned = [("a.rs".to_string(), Scanned::Unchanged)].into();
        reconcile(&mut state, scanned, &rules());
        assert_eq!(state.files["a.rs"].items[0].ticks, vec![true, false]);
    }

    #[test]
    fn duplicate_ordinals_align() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            item("a.rs", "aaaaaaaaaa", Kind::Comment, "TODO", 1),
            done(item("a.rs", "aaaaaaaaaa~2", Kind::Comment, "TODO", 7)),
        ];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![
                item("a.rs", "aaaaaaaaaa", Kind::Comment, "TODO", 1),
                item("a.rs", "aaaaaaaaaa~2", Kind::Comment, "TODO", 9),
            ]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let items = &state.files["a.rs"].items;
        assert_eq!(items[0].status, Status::Pending);
        assert_eq!(items[1].status, Status::Done);
        assert_eq!(items[1].line, [9, 9]);
    }

    const X: &str = "xxxxxxxxxx";
    const X2: &str = "xxxxxxxxxx~2";
    const Y: &str = "yyyyyyyyyy";

    /// Two `processed_result` declarations in a.rs with `between` in the middle.
    /// The first is pending, the second done. The agent renames the first to
    /// `result`, which shifts the second's id from x~2 to x.
    fn rename_first_of_two(between: &[(&str, &str)]) -> State {
        let mid = |path: &str| -> Vec<Item> {
            between
                .iter()
                .map(|(id, text)| item(path, id, Kind::Var, text, 2))
                .collect()
        };
        let mut state = State::default();
        let mut old = vec![item("a.rs", X, Kind::Var, "processed_result", 1)];
        old.extend(mid("a.rs"));
        old.push(done(item("a.rs", X2, Kind::Var, "processed_result", 3)));
        state.entry_mut("a.rs").items = old;
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec![format!("a.rs#{X}"), format!("a.rs#{X2}")],
                issued: 0,
                snapshot: [
                    (format!("a.rs#{X}"), "processed_result".to_string()),
                    (format!("a.rs#{X2}"), "processed_result".to_string()),
                ]
                .into(),
                only: None,
            },
        );
        let mut new = vec![item("a.rs", Y, Kind::Var, "result", 1)];
        new.extend(mid("a.rs"));
        new.push(item("a.rs", X, Kind::Var, "processed_result", 3));
        reconcile(
            &mut state,
            [("a.rs".to_string(), parsed(new))].into(),
            &rules(),
        );
        state
    }

    #[test]
    fn renaming_one_of_two_close_duplicates_keeps_both_states() {
        let state = rename_first_of_two(&[("bbbbbbbbbb", "two")]);
        let items = &state.files["a.rs"].items;

        let renamed = &items[0];
        assert_eq!(renamed.id, Y);
        assert_eq!(renamed.status, Status::Pending);
        assert_eq!(renamed.was.as_deref(), Some("processed_result"));
        assert_eq!(renamed.alias.as_deref(), Some(X));

        // The untouched copy keeps its id; no other item ever takes it.
        let untouched = &items[2];
        assert_eq!(untouched.id, X2);
        assert_eq!(untouched.alias, None);
        assert_eq!(untouched.status, Status::Done);
        assert_eq!(untouched.was, None);
        assert!(!untouched.changed_after_done);

        let chunk = &state.chunks["1/1"];
        assert_eq!(chunk.keys, [format!("a.rs#{Y}"), format!("a.rs#{X2}")]);
        assert_eq!(chunk.snapshot.len(), 2);

        // The ids the chunk printed still reach the items they were printed for.
        assert_eq!(state.resolve(X).unwrap().1, Y);
        assert_eq!(state.resolve(X2).unwrap().1, X2);
        assert_eq!(state.resolve(Y).unwrap().1, Y);
    }

    #[test]
    fn renaming_one_of_two_far_duplicates_leaves_the_other_unchanged() {
        let state = rename_first_of_two(&[
            ("bbbbbbbbbb", "two"),
            ("cccccccccc", "alpha"),
            ("dddddddddd", "beta"),
            ("eeeeeeeeee", "gamma"),
        ]);
        let untouched = state.files["a.rs"].items.last().unwrap();
        assert_eq!(untouched.id, X2);
        assert_eq!(untouched.status, Status::Done);
        assert_eq!(untouched.was, None);
        assert!(!untouched.changed_after_done);
    }

    #[test]
    fn moving_the_second_duplicate_keeps_its_state() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            item("a.rs", X, Kind::Var, "processed_result", 1),
            done(item("a.rs", X2, Kind::Var, "processed_result", 2)),
        ];
        let scanned = [
            (
                "a.rs".to_string(),
                parsed(vec![item("a.rs", X, Kind::Var, "processed_result", 1)]),
            ),
            (
                "b.rs".to_string(),
                parsed(vec![item("b.rs", X, Kind::Var, "processed_result", 1)]),
            ),
        ]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert_eq!(state.files["a.rs"].items[0].status, Status::Pending);
        let moved = &state.files["b.rs"].items[0];
        assert_eq!(moved.status, Status::Done);
        assert_eq!(moved.id, X2);
        assert_eq!(moved.alias, None);
    }

    fn with_ctx(mut i: Item, ctx: u64) -> Item {
        i.ctx = Some(ctx);
        i
    }

    #[test]
    fn deleting_one_of_two_copies_keeps_the_others_id() {
        // Nothing between the copies tells them apart except the code around them.
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            with_ctx(item("a.rs", X, Kind::Var, "processed_result", 1), 1),
            with_ctx(done(item("a.rs", X2, Kind::Var, "processed_result", 2)), 2),
        ];
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec![format!("a.rs#{X}"), format!("a.rs#{X2}")],
                ..Chunk::default()
            },
        );
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![with_ctx(
                item("a.rs", X, Kind::Var, "processed_result", 1),
                2,
            )]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let survivor = &state.files["a.rs"].items[0];
        assert_eq!(survivor.id, X2);
        assert_eq!(survivor.status, Status::Done);
        assert_eq!(
            state.chunks["1/1"].keys,
            [format!("a.rs#{X}"), format!("a.rs#{X2}")]
        );
        // The deleted copy's printed id reaches nothing, so `done` can call it gone.
        assert!(state.resolve(X).is_err());
    }

    #[test]
    fn an_unclear_context_keeps_the_pairing_the_diff_found() {
        // Copies in two functions. The agent deletes the first function and edits
        // the line after the second copy, so no context matches; the function
        // around the second copy still shows which copy survived.
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![
            item("a.rs", "aaaaaaaaaa", Kind::Fn, "one", 1),
            with_ctx(item("a.rs", X, Kind::Var, "processed_result", 2), 1),
            item("a.rs", "bbbbbbbbbb", Kind::Fn, "two", 3),
            with_ctx(done(item("a.rs", X2, Kind::Var, "processed_result", 4)), 2),
        ];
        let scanned = [(
            "a.rs".to_string(),
            parsed(vec![
                item("a.rs", "bbbbbbbbbb", Kind::Fn, "two", 1),
                with_ctx(item("a.rs", X, Kind::Var, "processed_result", 2), 3),
            ]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        let survivor = &state.files["a.rs"].items[1];
        assert_eq!(survivor.id, X2);
        assert_eq!(survivor.status, Status::Done);
    }

    #[test]
    fn same_text_in_two_files_gets_distinct_ids() {
        let mut state = State::default();
        let scanned = [
            (
                "a.rs".to_string(),
                parsed(vec![item("a.rs", X, Kind::Param, "config_path", 1)]),
            ),
            (
                "b.rs".to_string(),
                parsed(vec![item("b.rs", X, Kind::Param, "config_path", 1)]),
            ),
        ]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert_eq!(state.files["a.rs"].items[0].id, X);
        assert_eq!(state.files["b.rs"].items[0].id, X2);
    }

    #[test]
    fn duplicate_ids_from_an_older_state_are_renumbered() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![done(item("a.rs", X, Kind::Param, "config_path", 1))];
        state.entry_mut("b.rs").items = vec![item("b.rs", X, Kind::Param, "config_path", 1)];
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec![format!("a.rs#{X}"), format!("b.rs#{X}")],
                snapshot: [
                    (format!("a.rs#{X}"), "config_path".to_string()),
                    (format!("b.rs#{X}"), "config_path".to_string()),
                ]
                .into(),
                ..Chunk::default()
            },
        );
        let scanned = [
            ("a.rs".to_string(), Scanned::Unchanged),
            ("b.rs".to_string(), Scanned::Unchanged),
        ]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert_eq!(state.files["a.rs"].items[0].id, X);
        assert_eq!(state.files["a.rs"].items[0].status, Status::Done);
        assert_eq!(state.files["b.rs"].items[0].id, X2);
        assert_eq!(state.files["b.rs"].items[0].status, Status::Pending);
        assert_eq!(
            state.chunks["1/1"].keys,
            [format!("a.rs#{X}"), format!("b.rs#{X2}")]
        );
        assert!(
            state.chunks["1/1"]
                .snapshot
                .contains_key(&format!("b.rs#{X2}"))
        );
    }

    #[test]
    fn ids_a_chunk_still_refers_to_are_not_reused() {
        let mut state = State::default();
        state.entry_mut("a.rs").items = vec![item("a.rs", X, Kind::Var, "processed_result", 1)];
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec![format!("a.rs#{X}")],
                ..Chunk::default()
            },
        );
        // The agent deletes the declaration, then a later edit adds the same text back.
        reconcile(
            &mut state,
            [("a.rs".to_string(), parsed(vec![]))].into(),
            &rules(),
        );
        reconcile(
            &mut state,
            [(
                "a.rs".to_string(),
                parsed(vec![item("a.rs", X, Kind::Var, "processed_result", 1)]),
            )]
            .into(),
            &rules(),
        );
        assert_eq!(state.files["a.rs"].items[0].id, X2);
    }

    #[test]
    fn skipped_file_drops_items() {
        let mut state = State::default();
        state.entry_mut("gen.rs").record.skipped = true;
        let scanned = [(
            "gen.rs".to_string(),
            parsed(vec![item("gen.rs", "aaaaaaaaaa", Kind::Fn, "x", 1)]),
        )]
        .into();
        reconcile(&mut state, scanned, &rules());
        assert!(state.files["gen.rs"].items.is_empty());
        assert!(state.files["gen.rs"].record.skipped);
    }
}
