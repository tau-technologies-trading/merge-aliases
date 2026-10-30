# AGENTS.md

## Project overview

`merge-aliases` is a library crate for merging ticker CSV data after symbol renames. Its public API is in `src/lib.rs`; this package has no CLI executable.

## Public API

- `plan_json` returns a read-only JSON plan.
- `merge_folder` processes one data directory with parallel file work.

## Behavior

- Alias lines use `OLD --> NEW`; comments start with `#`.
- Only files matching the old ticker prefix and ending in `.csv` are processed.
- Colliding files merge by timestamp, with the target row winning duplicate timestamps.
- Source files are removed after successful merge; empty old ticker folders are removed.
- Non-dry-run merges lock the data directory.

## Validation

- `cargo fmt --check`
- `cargo build --release`
- `cargo test` when the user asks for tests or verification
