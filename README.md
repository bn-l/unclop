# unclop

`unclop` inventories every comment, identifier, and string literal in your codebase with tree-sitter, owns the review state, and walks a coding agent through fixing them one chunk at a time — with the prompt and rules restated in every chunk.

It is built for the cleanup pass after a model has been let loose on your code: `process_validated_user_data_result` instead of `parse_input`, `// Utilizes the robust helper to seamlessly parse the input` instead of `// parse the input`. A formatter won't rewrite prose and reviewing hundreds of these by hand doesn't scale, so `unclop` turns the whole repository into a finite, ordered work queue.

## Why it helps

- **Nothing slips through.** Every comment, declaration-site identifier, and prose string is found and tracked; `status` always tells you what is left.
- **The agent can't forget the rules.** They are printed at the top of every chunk, next to the items they apply to, instead of relying on instructions buried deep in the context window.
- **The agent can't rubber-stamp.** `done` rescans the file and refuses any item whose text is unchanged since it was issued, unless `--keep` says it was fine as it was.
- **Progress survives edits.** Items are identified by content hash, so ticks survive renames, moves, line shifts, and file renames. Done stays done.
- **You get an audit trail.** `report` shows before → after for every rewritten item, plus everything that was skipped.

## Features

- **10 languages** compiled in: Rust, Python, TypeScript, TSX, JavaScript, Go, C, C++, Ruby, Bash. Each language is a Cargo feature if you want a smaller binary.
- **Three item categories**: comments (including doc comments and Python docstrings), identifiers at declaration sites, and string literals that read as prose.
- **Noise is filtered up front**: pragmas, license headers, shebangs, import paths, object keys, Go struct tags, boilerplate JSX attributes, URLs, hashes, dates.
- **Content-hash identity**: ids survive edits, deleted items count as done, rewrites carry their state forward and are flagged for `reopen --changed`.
- **Chunked manifests for agents**: prompt + per-category rules + items with file, kind, line, scope, and the exact `done` command to run.
- **Parallel workers**: `next --worker 2/4` gives each worker a disjoint set of files; `next`, `status`, and `report` support `--json` for orchestrators.
- **Transparent, committed state**: one grep-able `.unclop.jsonl` file, no database. Locked, atomically written, recoverable after an interrupted save.
- **Agent-friendly exit codes**: `next` refuses (exit 2) while the previous chunk is unresolved; `status` exits 1 while anything is pending.

## Install

Requires a Rust toolchain (edition 2024, so Rust 1.85+).

```sh
cargo install --path .
# or straight from the repo
cargo install --git https://github.com/bn-l/unclop.git
```

To build with a subset of languages:

```sh
cargo install --path . --no-default-features --features lang-rust,lang-python
```

## Quick start

```sh
cd your-project
unclop init      # writes ~/.config/unclop/config.yaml, scans, prints counts
```

Edit the generated config — the prompt and rules are placeholders. Then hand the task to your agent, for example:

> Run `unclop next` in this repo. Rewrite every item it lists in the source,
> report back with the `done` command it prints, and repeat until nothing is pending.

A chunk looks like this:

```text
$ unclop next
unclop · 3 items in this chunk · 47 pending overall

Write comments, identifiers and strings in plain, direct English. ...

Rules for identifiers:
  1. no filler prefixes or suffixes (validated_, processed_, _result)
  2. ...
Rules for comments:
  1. ...
Rules for strings:
  1. ...

## src/parser.rs  (3 of 9 pending in this file)

  77b1e0a2f1  fn      L22      process_validated_user_data_result  in impl Parser
  a3f9c1d2e0  doc     L12-14   Utilizes the robust helper to seamlessly parse the input
  5e5e5e5e5e  string  L40      "An unexpected error has occurred while processing"

Mark each item when finished, listing every rule you checked:
  unclop done 77b1e0a2f1:1,2,3 a3f9c1d2e0:1,2,3 5e5e5e5e5e:1,2,3
```

The agent edits the code, then reports back:

```sh
unclop done 77b1e0a2f1:1,2,3 a3f9c1d2e0:1,2,3
unclop next
```

Guardrails built into the loop:

- Rules are numbered per category; `done` only accepts numbers that exist for that item, and an item is done only when every rule is ticked.
- If the text is byte-for-byte unchanged since the chunk was issued, `done` refuses it: edit it, or pass `--keep` if it is genuinely fine.
- If an item was deleted from source, `done` reports it as gone and counts it as done.
- `next` exits 2 while items from the previous chunk are unresolved; `--force` moves on anyway.
- When nothing is pending, `next` points at `report`, which prints `was → now` for every rewrite and lists everything skipped.

## Commands

| Command | What it does |
|---|---|
| `unclop init` | Write the default config if missing, scan and reconcile, print counts per category |
| `unclop next [--worker I/N] [--force] [--json]` | Print the next chunk. Exits 2 while your previous chunk has unresolved items |
| `unclop done <id>:<rules>... [--keep]` | Mark items done, listing the rules checked, e.g. `done 77b1e0a2f1:1,2,3` |
| `unclop skip <id>...` / `unclop skip --file <path>...` | Skip items, or a whole generated file, permanently |
| `unclop reopen <id>...` / `--changed` / `--file <path>...` | Return items to pending; `--changed` reopens everything edited after it was done |
| `unclop status [--json]` | Counts per file and category; exits 1 while anything is pending |
| `unclop report [--json]` | Before → after for every done item, plus everything skipped |

Global flags: `-C <dir>` runs against another directory, `--config <path>` uses a different config file.

Ids are 10 hex characters. Any unique prefix of 6 or more works; when a prefix is ambiguous, use `path#id`.

## What gets inventoried

**Comments.** Single-line and block comments; adjacent line comments at the same column merge into one item. Doc comments (`///`, `//!`, `/**`) and Python docstrings count as docs. Pragmas (`eslint-...`, `noqa`, `nolint`, `//go:...`), license/SPDX/copyright headers, shebangs, and separator lines are dropped.

**Identifiers.** Declaration sites only: functions, types, variants, fields, variables, constants, parameters, modules, macros. Uses are never listed. Per-language exclusions cover Rust trait-impl methods, Python dunders and `self`/`cls`, constructors and `override` methods in C++, Ruby's core method names, and `_` everywhere.

**Strings.** Only literals that read as prose: at least two words containing letters (configurable), at least 60% letters among non-space characters, and not a URL, path, hash, date, email, or data URI. Import/require paths, object keys, Go struct tags, Rust attributes, and boilerplate JSX attributes are excluded; Python docstrings are counted as comments, not strings.

| Language | Extensions |
|---|---|
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| JavaScript | `.js`, `.mjs`, `.cjs`, `.jsx` |
| Go | `.go` |
| C | `.c`, `.h` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.ipp` |
| Ruby | `.rb`, `.rake`, `.gemspec` |
| Bash | `.sh`, `.bash`, `.zsh` |

Extensionless files with a `bash`/`sh`/`zsh`, `python`, `ruby`, or `node` shebang are also picked up.

## How it works

1. **Walk and parse.** The tree walk respects `.gitignore` and `.unclopignore`, and always ignores `node_modules`, `target`, `vendor`, `dist`, `build`, and `.git`. Files over 2 MB and non-UTF-8 files are skipped. Supported files are parsed with tree-sitter in parallel.
2. **Extract and filter.** Per-language tree-sitter queries find comments, declarations, and string literals. Comments are merged and classified, identifiers go through per-language exclusion lists, and strings through the prose filter. Every item gets a scope label (`in impl Parser`) and a source span.
3. **Identify.** An item's id is the first 10 hex characters of an xxh3-64 hash of `category + kind + normalized text`. Lines and paths are metadata, never identity; duplicate content within a file gets a `~2`, `~3` ordinal. Because identity is content, the tool always knows which text it is reviewing regardless of where it moved.
4. **Reconcile.** Every command rescans first and diffs the old and new id sequences per file (Myers). Equal runs keep their status and ticks; inside rewrites, items are paired by kind so state carries across edits; items that moved, or a file that was renamed, keep state by matching content hashes across files. A done item that gets rewritten keeps its done status but is recorded with its old text (`was`) and flagged `changed_after_done`, so `report` can show the before and `reopen --changed` can bring it back.
5. **Store.** State lives in `.unclop.jsonl` at the project root — commit it with the code. Each command takes an exclusive lock, writes through a transient `.unclop.jsonl.tmp` journal, then rewrites the state file in place; an interrupted save is recovered from the journal on the next run. Files whose mtime and size are unchanged are not re-parsed, so repeated runs are cheap.
6. **Serve.** `next` packs whole files into a chunk (default 25 items), in source order with identifiers first, then comments, then strings. It stores a snapshot of each item's text so `done` can tell whether it was actually edited.

`PLAN.md` documents the full design and rationale.

## Configuration

`unclop init` writes `$XDG_CONFIG_HOME/unclop/config.yaml` (or `~/.config/unclop/config.yaml`). `--config <path>` or the `UNCLOP_CONFIG` environment variable overrides the location.

```yaml
prompt: |
  Write comments, identifiers and strings in plain, direct English. ...
rules:
  comment:
    - ...
  identifier:
    - ...
  string:
    - ...
chunk_size: 25
strings:
  min_words: 2
```

Rules are plain strings numbered by position — those numbers are what the agent references in `done`. A project-root `.unclop.yaml` can append repo-specific `notes:` to the prompt and override `chunk_size:`.

The default config contains placeholders; write your own prompt and rules before the first real run.

## Development

```sh
cargo test
```

The suite covers state round-trips and recovery, reconcile scenarios (renames, moves, deletes, duplicates), extraction snapshots per language, and an end-to-end CLI walkthrough. The hidden `unclop debug-tree <file>` command prints a file's tree-sitter parse, which helps when adding or fixing a query under `queries/`.

## License

MIT
