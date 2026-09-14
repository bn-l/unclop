use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "unclop",
    version,
    about = "Inventory comments, identifiers and strings with tree-sitter and hand them to an agent in chunks"
)]
pub struct Cli {
    /// Run against this directory instead of the current one
    #[arg(short = 'C', long = "dir", global = true, value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// Config file to use instead of the XDG default
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Write the default config if it is missing, scan the tree, print counts
    Init,

    /// Rescan, then print the next chunk of pending items with the prompt and rules
    Next {
        /// Take only the files assigned to worker I of N, e.g. 2/4
        #[arg(long, value_name = "I/N")]
        worker: Option<String>,
        /// Hand out a new chunk even if the previous one has unresolved items
        #[arg(long)]
        force: bool,
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Mark items done. Each argument is ID:RULES, e.g. 77b1e0a2f1:1,2,3
    Done {
        #[arg(required = true, value_name = "ID:RULES")]
        items: Vec<String>,
        /// Accept items whose text has not changed since they were issued
        #[arg(long)]
        keep: bool,
    },

    /// Skip items by id, or whole files with --file
    Skip {
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Skip every item in this file and never list it again
        #[arg(long = "file", value_name = "PATH")]
        files: Vec<String>,
    },

    /// Return items to pending
    Reopen {
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Reopen every done item whose text changed after it was marked done
        #[arg(long)]
        changed: bool,
        /// Stop skipping this file
        #[arg(long = "file", value_name = "PATH")]
        files: Vec<String>,
    },

    /// Counts per file. Exits 1 while anything is pending
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Before and after for every done item, plus everything skipped
    Report {
        #[arg(long)]
        json: bool,
    },

    /// Print the tree-sitter parse of one file
    #[command(hide = true, name = "debug-tree")]
    DebugTree { file: PathBuf },
}
