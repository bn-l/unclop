//! Command line definition and the help text.
//!
//! The top-level help is one complete document: every command with its full
//! description and flags, the workflow, the id rules, the files and the exit
//! codes. `-h`, `--help`, `help` and a bare `unclop` all print it. Subcommands
//! keep their own `--help` built from the same constants.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::ids::Category;

const WIDTH: usize = 88;

const ABOUT: &str = "Lists every comment, identifier and string literal in a codebase and tracks the review of each one";

const LONG_ABOUT: &str = "\
unclop lists every comment, declaration-site identifier and prose string literal in a \
codebase and tracks the review of each one. It parses the source with tree-sitter and \
keeps its state in .unclop.jsonl at the project root.

The tool is a work queue for a coding agent. The agent asks for a chunk of pending \
items with `next`, edits them in the source, reports each one with `done` or `skip` \
and asks for the next chunk. Every chunk repeats the prompt and the rules from the \
config so the agent reads them fresh each time.";

const WORKFLOW: &str =
    "  1. A person runs `unclop init` once in the project root. It writes the config file
     when none exists and scans the tree. The prompt and the rules in the config are
     placeholders until a person edits them.
  2. The agent runs `unclop next`. The output is the prompt, the numbered rules and a
     chunk of pending items. Each item has an id, a kind, a line span and a scope.
  3. The agent edits the source. Renaming a declaration, rewriting a comment and
     deleting a comment or a string are all valid edits.
  4. The agent runs the `unclop done` command printed at the end of the chunk. Items
     whose name or text is fixed by something outside the codebase and whole generated
     files are marked with `unclop skip` instead.
  5. The agent repeats from step 2. `unclop next` exits with code 2 while items from
     the previous chunk are still pending. When nothing is pending it prints a pointer
     to `unclop report`.";

const IDS: &str =
    "  An id is ten hex characters, for example 77b1e0a2f1. Identical text repeated in one
  file gets a suffix: 77b1e0a2f1~2. Commands accept a full id, a prefix of six or more
  characters that matches one item, or path#id when a prefix matches several items.
  An item keeps its state when its text changes and stays reachable by the id the
  chunk printed.";

const RESCANNING: &str =
    "  Every command rescans the tree before it does anything else. Edits are picked up
  without a separate refresh command and the line numbers in a chunk are current at
  the moment it is printed. Files whose size and modification time are unchanged are
  not parsed again.";

const FILES: &str =
    "  .unclop.jsonl                        state, one JSON record per line. Commit it.
  .unclop.yaml                         optional project notes appended to the prompt
                                       and an optional chunk_size override
  .unclopignore                        extra ignore rules in gitignore syntax
  $XDG_CONFIG_HOME/unclop/config.yaml  prompt, rules, chunk_size and strings.min_words";

const EXIT_CODES: &str = "  0   the command completed
  1   `done` or `skip` rejected an argument, or `status` found pending items
  2   `next` found unresolved items from the previous chunk. An invalid command line
      also exits with code 2.
  70  an error stopped the command, for example a missing config file";

const INIT_LONG: &str = "\
Writes the default config to the XDG path when no config exists there and prints the \
path. Then scans the current directory, records every item in .unclop.jsonl and prints \
the number of identifiers, comments and strings found.

Run this once per project before the agent starts. The default config holds \
placeholder text for the prompt and the rules. Edit both before the first `next`.";

const NEXT_LONG: &str = "\
Prints the prompt, the numbered rules for each category present in the chunk, the \
pending items grouped by file and the exact `done` command for those items. Each item \
line holds the id, the kind, the line span, the text and the enclosing declaration.

A chunk holds whole files. Files are added in path order until the next file would \
push the chunk past chunk_size. A single file with more pending items than chunk_size \
is split and continues in the next chunk. Within a file the order is identifiers, then \
comments, then strings, each in source order.

--only limits the chunk to one category. Running `next --only identifier` until no \
identifier is pending and then `next --only comment` reviews every identifier in the \
codebase before the first comment, so comments are rewritten against the final names. \
When the category has no pending item the command says so and exits with code 0.

The command exits with code 2 and lists the unresolved items when items from this \
worker's previous chunk are still pending. Mark them with `done` or `skip`, or pass \
--force to receive a new chunk anyway.

When no item is pending the command prints a pointer to `report` and exits with \
code 0.";

const DONE_LONG: &str = "\
Records which rules were checked for each item. RULES is a comma-separated list of the \
rule numbers printed in the chunk for the item's category. An item becomes done when \
every rule for its category has been listed, in one call or across several calls. A \
partial list leaves the item pending and the reply names the rules still unchecked.

The command rescans first and compares each item's text with the text at the time the \
chunk was issued. An item whose text is unchanged is rejected with exit code 1 unless \
--keep is present. An item whose source was deleted is reported as gone and counted as \
done. An item whose text changed is accepted and the old text is kept for `report`.

Several ids can be passed in one call. Every argument is processed and the exit code \
is 1 if any argument was rejected.";

const SKIP_LONG: &str = "\
Marks items skipped so they leave the pending set without an edit. Skip an item when \
its name or text is fixed by something outside the codebase, such as a trait method, an \
interface member, a wire format or a third-party API.

--file marks a whole file skipped, drops its items and keeps it out of future chunks. \
Use it for generated files. `reopen --file` undoes it.";

const REOPEN_LONG: &str = "\
Sets items back to pending with every rule unchecked. --changed selects every done item \
whose text was edited after it was marked done. --file stops skipping a file so its \
items appear again on the next scan.";

const STATUS_LONG: &str = "\
Prints one row per file with pending, done and skipped counts, then totals per \
category. Skipped files are marked. The exit code is 1 while any item is pending and 0 \
otherwise, so a script can loop on it.";

const REPORT_LONG: &str = "\
Groups settled items by file. A done item whose text changed is printed with its \
current text and its old text on a `was:` line. A done item whose text did not change \
is printed as kept. Skipped items and skipped files are listed. Pending items are not \
shown.";

const DIR_HELP: &str = "Run in this directory instead of the current one. The state file and the \
project notes file are read from and written to it.";
const CONFIG_HELP: &str = "Read this config file instead of $XDG_CONFIG_HOME/unclop/config.yaml. The \
UNCLOP_CONFIG environment variable does the same.";
const WORKER_HELP: &str = "Take only the files assigned to worker I of N, for example 2/4. Files are \
assigned by a hash of their path so N agents running 1/N through N/N cover every file once.";
const ONLY_HELP: &str = "Take only items of this category: identifier, comment or string.";
const FORCE_HELP: &str = "Print a new chunk even when the previous chunk has unresolved items.";
const NEXT_JSON_HELP: &str = "Print the chunk as JSON. The object holds the prompt, the rules, the files \
with their items and the exit code.";
const ITEMS_HELP: &str =
    "An item id followed by a colon and the rule numbers checked, for example 77b1e0a2f1:1,2,3.";
const KEEP_HELP: &str = "Accept items whose text has not changed since the chunk was issued. Use it when \
the item is correct as written.";
const SKIP_IDS_HELP: &str = "Item ids to skip.";
const SKIP_FILE_HELP: &str =
    "Skip every item in this file and leave the file out of future chunks.";
const REOPEN_IDS_HELP: &str = "Item ids to return to pending.";
const CHANGED_HELP: &str = "Return every done item whose text changed after it was marked done.";
const REOPEN_FILE_HELP: &str = "Stop skipping this file.";
const STATUS_JSON_HELP: &str = "Print the counts as JSON.";
const REPORT_JSON_HELP: &str = "Print the report as JSON.";

const TEMPLATE: &str = "{about}\n\n{usage-heading} {usage}{after-help}";

fn parse_category(s: &str) -> Result<Category, String> {
    Category::parse(s).ok_or_else(|| "expected identifier, comment or string".to_string())
}

/// Re-wraps paragraphs of `text` to `WIDTH` with `indent` leading spaces on every line.
fn wrap(text: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let limit = WIDTH.saturating_sub(indent);
    let mut out = String::new();
    for (i, paragraph) in text.split("\n\n").enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > limit {
                out.push_str(&pad);
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            out.push_str(&pad);
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// One flag with its description in a hanging-indent column.
fn option(out: &mut String, indent: usize, flag: &str, help: &str) {
    const COLUMN: usize = 20;
    let pad = " ".repeat(indent);
    let body = wrap(help, indent + COLUMN);
    if flag.len() < COLUMN {
        let first = body.trim_start();
        out.push_str(&format!("{pad}{flag:<COLUMN$}{first}"));
    } else {
        out.push_str(&format!("{pad}{flag}\n{body}"));
    }
}

fn command(out: &mut String, usage: &[&str], long: &str, opts: &[(&str, &str)]) {
    for u in usage {
        out.push_str("  ");
        out.push_str(u);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&wrap(long, 6));
    if !opts.is_empty() {
        out.push('\n');
        for (flag, help) in opts {
            option(out, 6, flag, help);
        }
    }
    out.push('\n');
}

/// The complete top-level help below the usage line.
pub fn main_help() -> String {
    let mut s = String::new();
    s.push_str(&wrap(LONG_ABOUT, 0));
    s.push('\n');

    s.push_str("Workflow:\n");
    s.push_str(WORKFLOW);
    s.push_str("\n\n");

    s.push_str("Commands:\n\n");
    command(&mut s, &["unclop init"], INIT_LONG, &[]);
    command(
        &mut s,
        &["unclop next [--worker I/N] [--only <CATEGORY>] [--force] [--json]"],
        NEXT_LONG,
        &[
            ("--worker I/N", WORKER_HELP),
            ("--only <CATEGORY>", ONLY_HELP),
            ("--force", FORCE_HELP),
            ("--json", NEXT_JSON_HELP),
        ],
    );
    command(
        &mut s,
        &["unclop done <ID:RULES>... [--keep]"],
        DONE_LONG,
        &[("<ID:RULES>", ITEMS_HELP), ("--keep", KEEP_HELP)],
    );
    command(
        &mut s,
        &["unclop skip <ID>...", "unclop skip --file <PATH>..."],
        SKIP_LONG,
        &[("<ID>", SKIP_IDS_HELP), ("--file <PATH>", SKIP_FILE_HELP)],
    );
    command(
        &mut s,
        &[
            "unclop reopen <ID>...",
            "unclop reopen --changed",
            "unclop reopen --file <PATH>...",
        ],
        REOPEN_LONG,
        &[
            ("<ID>", REOPEN_IDS_HELP),
            ("--changed", CHANGED_HELP),
            ("--file <PATH>", REOPEN_FILE_HELP),
        ],
    );
    command(
        &mut s,
        &["unclop status [--json]"],
        STATUS_LONG,
        &[("--json", STATUS_JSON_HELP)],
    );
    command(
        &mut s,
        &["unclop report [--json]"],
        REPORT_LONG,
        &[("--json", REPORT_JSON_HELP)],
    );

    s.push_str("Global options:\n");
    option(&mut s, 2, "-C, --dir <DIR>", DIR_HELP);
    option(&mut s, 2, "--config <PATH>", CONFIG_HELP);
    option(&mut s, 2, "-h, --help", "Print this help.");
    option(&mut s, 2, "-V, --version", "Print the version.");
    s.push('\n');

    s.push_str("Ids:\n");
    s.push_str(IDS);
    s.push_str("\n\n");
    s.push_str("Rescanning:\n");
    s.push_str(RESCANNING);
    s.push_str("\n\n");
    s.push_str("Files:\n");
    s.push_str(FILES);
    s.push_str("\n\n");
    s.push_str("Exit codes:\n");
    s.push_str(EXIT_CODES);
    s
}

#[derive(Parser, Debug)]
#[command(
    name = "unclop",
    version,
    about = ABOUT,
    help_template = TEMPLATE,
    after_help = main_help(),
    arg_required_else_help = true,
    disable_help_subcommand = false
)]
pub struct Cli {
    #[arg(short = 'C', long = "dir", global = true, value_name = "DIR", help = DIR_HELP)]
    pub dir: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH", help = CONFIG_HELP)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Write the default config if it is missing, scan the tree and print counts
    #[command(long_about = wrap(INIT_LONG, 0))]
    Init,

    /// Rescan and print the next chunk of pending items with the prompt and rules
    #[command(long_about = wrap(NEXT_LONG, 0))]
    Next {
        #[arg(long, value_name = "I/N", help = WORKER_HELP)]
        worker: Option<String>,
        #[arg(long, value_name = "CATEGORY", value_parser = parse_category, help = ONLY_HELP)]
        only: Option<Category>,
        #[arg(long, help = FORCE_HELP)]
        force: bool,
        #[arg(long, help = NEXT_JSON_HELP)]
        json: bool,
    },

    /// Mark items done. Each argument is ID:RULES, for example 77b1e0a2f1:1,2,3
    #[command(long_about = wrap(DONE_LONG, 0))]
    Done {
        #[arg(required = true, value_name = "ID:RULES", help = ITEMS_HELP)]
        items: Vec<String>,
        #[arg(long, help = KEEP_HELP)]
        keep: bool,
    },

    /// Mark items skipped, or skip a whole file with --file
    #[command(long_about = wrap(SKIP_LONG, 0))]
    Skip {
        #[arg(value_name = "ID", help = SKIP_IDS_HELP)]
        ids: Vec<String>,
        #[arg(long = "file", value_name = "PATH", help = SKIP_FILE_HELP)]
        files: Vec<String>,
    },

    /// Return items to pending
    #[command(long_about = wrap(REOPEN_LONG, 0))]
    Reopen {
        #[arg(value_name = "ID", help = REOPEN_IDS_HELP)]
        ids: Vec<String>,
        #[arg(long, help = CHANGED_HELP)]
        changed: bool,
        #[arg(long = "file", value_name = "PATH", help = REOPEN_FILE_HELP)]
        files: Vec<String>,
    },

    /// Print counts per file and per category. Exits with code 1 while anything is pending
    #[command(long_about = wrap(STATUS_LONG, 0))]
    Status {
        #[arg(long, help = STATUS_JSON_HELP)]
        json: bool,
    },

    /// Print the old and new text of every done item and list everything skipped
    #[command(long_about = wrap(REPORT_LONG, 0))]
    Report {
        #[arg(long, help = REPORT_JSON_HELP)]
        json: bool,
    },

    /// Print the tree-sitter parse of one file
    #[command(hide = true, name = "debug-tree")]
    DebugTree { file: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_fits_the_width_and_names_every_command() {
        let help = main_help();
        for line in help.lines() {
            assert!(line.len() <= WIDTH + 4, "line too long: {line:?}");
        }
        for cmd in ["init", "next", "done", "skip", "reopen", "status", "report"] {
            assert!(help.contains(&format!("unclop {cmd}")), "missing {cmd}");
        }
        for flag in [
            "--worker",
            "--only",
            "--force",
            "--keep",
            "--changed",
            "--file",
            "--json",
            "--dir",
            "--config",
        ] {
            assert!(help.contains(flag), "missing {flag}");
        }
        assert!(help.contains("Exit codes:"));
    }
}
