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

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cli::{Cli, Cmd};
use crate::config::{self, Config};
use crate::reconcile;
use crate::scan::{self, lang::Registry};
use crate::state::{self, State};

pub struct Ctx {
    pub root: PathBuf,
    /// The directory unclop was run in; relative paths on the command line start here.
    pub workdir: PathBuf,
    pub config: Config,
    pub registry: Registry,
}

pub struct Output {
    pub lines: Vec<String>,
    pub json: Option<Value>,
    pub code: i32,
    /// The command for the footer when it is not plain `next` or `report`.
    pub then: Option<String>,
}

pub fn dispatch(cli: Cli) -> Result<i32> {
    let workdir = match cli.dir {
        Some(d) => d
            .canonicalize()
            .with_context(|| format!("cannot open directory {}", d.display()))?,
        None => std::env::current_dir().context("cannot determine the current directory")?,
    };

    match cli.cmd {
        Cmd::Init => init::run(&workdir, cli.config.as_deref()),
        Cmd::DebugTree { file } => debug::run(&file),
        cmd => {
            // Like git, a command run in a subdirectory works on the project above it
            // instead of starting a second review there.
            let root = workdir
                .ancestors()
                .find(|d| d.join(state::STATE_FILE).is_file())
                .map(Path::to_path_buf)
                .with_context(|| {
                    format!(
                        "no {} in {} or any directory above it. Run `unclop init` in the project root.",
                        state::STATE_FILE,
                        workdir.display()
                    )
                })?;
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
                workdir,
                config,
                registry,
            };
            let (output, pending, files) = state::with_lock(&root, |sf| {
                let mut st = sf.load()?;
                refresh(&ctx, &mut st)?;
                let out = match cmd {
                    Cmd::Next {
                        worker,
                        only,
                        force,
                        ..
                    } => next::run(&ctx, &mut st, worker.as_deref(), only, force)?,
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
    state.scan_key = scan::scan_key(&ctx.registry, &ctx.config);
    Ok(())
}

pub fn then_command(pending: usize) -> &'static str {
    if pending == 0 {
        "unclop report"
    } else {
        "unclop next"
    }
}

pub fn footer(pending: usize, files: usize, then: &str) -> String {
    format!(
        "{pending} pending across {files} {} · then: {then}",
        if files == 1 { "file" } else { "files" },
    )
}

fn emit(out: Output, pending: usize, files: usize, json_mode: bool) -> i32 {
    let then = out
        .then
        .unwrap_or_else(|| then_command(pending).to_string());
    if json_mode {
        let mut value = out
            .json
            .unwrap_or_else(|| Value::Object(Default::default()));
        if let Value::Object(map) = &mut value {
            map.insert("pending".into(), pending.into());
            map.insert("pending_files".into(), files.into());
            map.insert("then".into(), then.clone().into());
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
        println!("{}", footer(pending, files, &then));
    }
    out.code
}

/// A path as the user typed it, relative to `workdir` or absolute, as the
/// project-relative, slash-separated form used in state. `.` and `..` are resolved
/// without touching the file system, so the path need not exist yet.
pub fn rel_path(root: &Path, workdir: &Path, given: &str) -> Result<String> {
    let joined = workdir.join(given);
    let joined = joined.canonicalize().unwrap_or(joined);
    let mut normalized = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    let rel = normalized
        .strip_prefix(root)
        .with_context(|| format!("{given} is outside the project at {}", root.display()))?;
    Ok(rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Preview of an item's text for command replies.
pub fn preview(text: &str) -> String {
    let mut t: String = text.chars().take(crate::manifest::PREVIEW_CHARS).collect();
    if text.chars().count() > crate::manifest::PREVIEW_CHARS {
        t.push('…');
    }
    t
}
