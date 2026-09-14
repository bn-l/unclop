//! Command dispatch. Every state-touching command runs under the lock as:
//! load state, rescan and reconcile, do the work, save, print, then a footer
//! with the remaining count and the next command to run.

pub mod debug;
pub mod done;
pub mod init;
pub mod next;
pub mod reopen;
pub mod report;
pub mod skip;
pub mod status;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cli::{Cli, Cmd};
use crate::config::{self, Config};
use crate::reconcile;
use crate::scan::{self, lang::Registry};
use crate::state::{self, State};

pub struct Ctx {
    pub root: PathBuf,
    pub config: Config,
    pub registry: Registry,
}

pub struct Output {
    pub lines: Vec<String>,
    pub json: Option<Value>,
    pub code: i32,
}

pub fn dispatch(cli: Cli) -> Result<i32> {
    let root = match cli.dir {
        Some(d) => d
            .canonicalize()
            .with_context(|| format!("cannot open directory {}", d.display()))?,
        None => std::env::current_dir().context("cannot determine the current directory")?,
    };

    match cli.cmd {
        Cmd::Init => init::run(&root, cli.config.as_deref()),
        Cmd::DebugTree { file } => debug::run(&file),
        cmd => {
            let json_mode = matches!(
                cmd,
                Cmd::Next { json: true, .. }
                    | Cmd::Status { json: true }
                    | Cmd::Report { json: true }
            );
            let config = config::load(&root, cli.config.as_deref())?;
            let registry = Registry::new()?;
            let ctx = Ctx {
                root: root.clone(),
                config,
                registry,
            };
            let (output, pending, files) = state::with_lock(&root, |sf| {
                let mut st = sf.load()?;
                refresh(&ctx, &mut st)?;
                let out = match cmd {
                    Cmd::Next { worker, force, .. } => {
                        next::run(&ctx, &mut st, worker.as_deref(), force)?
                    }
                    Cmd::Done { items, keep } => done::run(&ctx, &mut st, &items, keep)?,
                    Cmd::Skip { ids, files } => skip::run(&ctx, &mut st, &ids, &files)?,
                    Cmd::Reopen {
                        ids,
                        changed,
                        files,
                    } => reopen::run(&ctx, &mut st, &ids, changed, &files)?,
                    Cmd::Status { .. } => status::run(&ctx, &st)?,
                    Cmd::Report { .. } => report::run(&ctx, &st)?,
                    Cmd::Init | Cmd::DebugTree { .. } => unreachable!(),
                };
                sf.save(&st)?;
                let (pending, files) = st.pending();
                Ok((out, pending, files))
            })?;
            Ok(emit(output, pending, files, json_mode))
        }
    }
}

pub fn refresh(ctx: &Ctx, state: &mut State) -> Result<()> {
    let scanned = scan::scan_all(&ctx.registry, &ctx.root, &ctx.config, state)?;
    reconcile::reconcile(state, scanned, &ctx.config.rules);
    Ok(())
}

pub fn then_command(pending: usize) -> &'static str {
    if pending == 0 {
        "unclop report"
    } else {
        "unclop next"
    }
}

pub fn footer(pending: usize, files: usize) -> String {
    format!(
        "{pending} pending across {files} {} · then: {}",
        if files == 1 { "file" } else { "files" },
        then_command(pending)
    )
}

fn emit(out: Output, pending: usize, files: usize, json_mode: bool) -> i32 {
    if json_mode {
        let mut value = out
            .json
            .unwrap_or_else(|| Value::Object(Default::default()));
        if let Value::Object(map) = &mut value {
            map.insert("pending".into(), pending.into());
            map.insert("pending_files".into(), files.into());
            map.insert("then".into(), then_command(pending).into());
            map.insert("exit_code".into(), out.code.into());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    } else {
        for line in &out.lines {
            println!("{line}");
        }
        println!();
        println!("{}", footer(pending, files));
    }
    out.code
}

/// A path as the user typed it, normalized to the repo-relative, slash-separated
/// form used in state.
pub fn rel_path(root: &Path, given: &str) -> String {
    let p = Path::new(given);
    let rel: PathBuf = if p.is_absolute() {
        p.strip_prefix(root)
            .map(|r| r.to_path_buf())
            .unwrap_or_else(|_| p.to_path_buf())
    } else {
        p.to_path_buf()
    };
    let mut s = rel
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    while s.ends_with('/') {
        s.pop();
    }
    s
}

/// Preview of an item's text for command replies.
pub fn preview(text: &str) -> String {
    let mut t: String = text.chars().take(crate::manifest::PREVIEW_CHARS).collect();
    if text.chars().count() > crate::manifest::PREVIEW_CHARS {
        t.push('…');
    }
    t
}
