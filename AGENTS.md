# AGENTS.md

## Project Overview

`merge-aliases` is a small Rust CLI that merges ticker data folders after ticker symbol renames. It reads alias mappings formatted as `OLD --> NEW`, moves CSV files from the old ticker directory to the new ticker directory, and merges row-by-row when the destination CSV already exists.

## Repository Layout

- `src/main.rs`: CLI argument parsing, alias parsing, file collection, merge behavior, and unit tests.
- `Cargo.toml`: Rust package metadata. This project currently has no external dependencies.
- `README.md`: User-facing build, usage, data layout, and merge behavior documentation.
- `.github/workflows/tests.yaml`: CI build and test workflow for stable, beta, and nightly Rust.
- `.github/workflows/release.yaml`: Manual release workflow for Linux, macOS, and Windows binaries.

## Development Commands

- `cargo build`: Build the debug binary.
- `cargo build --release`: Build the optimized release binary at `target/release/merge-aliases`.
- `cargo test`: Run the test suite.
- `cargo run -- --dry-run`: Run with default paths and print planned actions without modifying files.
- `cargo run -- --aliases ../aliases.txt --data ../data --dry-run`: Run against explicit alias and data paths without modifying files.

Run `cargo test` before considering behavior changes complete. Use `cargo build` when changing CLI flow or filesystem behavior.

## Coding Guidelines

- Keep the implementation dependency-free unless there is a strong reason to add a crate.
- Prefer small, direct functions and standard library APIs.
- Preserve the current CLI defaults: `../aliases.txt` for aliases and `../data` for data.
- Preserve dry-run behavior: `--dry-run` must not create, move, merge, delete, or rename files.
- Treat filesystem operations carefully. Avoid destructive changes unless the code has already validated paths and intended actions.
- Keep error messages specific enough to identify the problematic path, argument, or alias line.
- Keep output human-readable and consistent with the existing style.

## Behavior Notes

- Alias lines are comments if they start with `#` after trimming.
- Alias lines must contain `-->`; both sides must be non-empty and different.
- Only files in the old ticker folder ending in `.csv` and starting with `{OLD}-` are processed.
- Target filenames replace the old ticker prefix with the new ticker prefix.
- Existing target CSV rows win by row number; extra old rows are appended.
- Temporary files are written next to the target file during merges.
- Old source CSV files are removed after successful non-dry-run merges.
- Empty old ticker folders are removed unless `--keep-old` is passed.

## Testing Guidance

- Add focused unit tests near the existing tests in `src/main.rs` for pure parsing or merge behavior.
- Prefer temporary directories only when testing filesystem behavior; ensure tests clean up after themselves.
- Include dry-run coverage when changing code that performs filesystem mutations.
- Keep tests deterministic and independent of external `../aliases.txt` or `../data` files.

## Documentation

- Update `README.md` when changing CLI options, defaults, alias format, data layout, or merge semantics.
- Keep examples aligned with the actual binary options printed by `--help`.

## Agent Notes

- Do not edit generated build artifacts under `target/`.
- Do not assume real ticker data exists in the workspace.
- Do not commit or tag releases unless explicitly asked.
