use std::path::Path;

use anyhow::Result;

use super::{Ctx, footer, refresh};
use crate::config;
use crate::scan::lang::Registry;
use crate::state::{STATE_FILE, with_lock};

const BANNER: &str = r"
       .------.
       | slop |
       | ~~~~ |
  .----|______|----.
  |  *  unclop     |
  |                |
  '-+-+-+-+-+-+-+--'
    | | | | | | |
    | ' | | ' | '
    '   | '   '
        '
";

pub fn run(root: &Path, config_override: Option<&Path>) -> Result<i32> {
    println!("{BANNER}");

    let path = config::config_path(config_override)?;
    let created = config::write_default_if_absent(&path)?;
    println!(
        "{} config {}",
        if created { "Wrote" } else { "Using" },
        path.display()
    );

    let config = config::load(root, config_override)?;
    let registry = Registry::new()?;
    let ctx = Ctx {
        root: root.to_path_buf(),
        config,
        registry,
    };

    let (by_cat, (pending, pending_files), file_count) = with_lock(root, |sf| {
        let mut state = sf.load()?;
        refresh(&ctx, &mut state)?;
        sf.save(&state)?;
        Ok((
            state.counts_by_category(),
            state.pending(),
            state.files.len(),
        ))
    })?;

    println!("- Scanned {file_count} files");
    for (cat, c) in by_cat {
        println!("  {:<12} {}", cat.label(), c.pending + c.done + c.skipped);
    }
    println!();
    if created {
        println!(
            "- Placeholder prompts are in: {}. Please edit that file before using.",
            path.display()
        );
    }
    println!();
    println!("- Unclop stores data in {STATE_FILE} (author suggests committing it).");
    println!();
    println!("{}", footer(pending, pending_files));
    println!();
    Ok(0)
}
