# merge-aliases

`merge-aliases` is a small (and 🌩️ fast) Rust CLI for merging ticker data folders when one ticker symbol has been renamed to another.

It reads alias mappings in the format `OLD --> NEW`, moves matching CSV files from the old ticker folder into the new ticker folder, and merges files that already exist under the new ticker seamlessly.

## Requirements

- Rust 2024 edition toolchain
- Cargo

## Build

```sh
cargo build --release
```

The compiled binary will be available at `target/release/merge-aliases`.

## Usage

```sh
merge-aliases [OPTIONS]
```

Options:

```text
-a, --aliases <PATH>  Alias file path [default: ../aliases.txt]
-d, --data <PATH>     Data directory path [default: ../data]
-j, --jobs <N>        Maximum files processed in parallel [default: 4]
    --dry-run         Print actions without modifying files
    --plan            Print planned operations as JSON without modifying files
    --keep-old        Do not remove empty old ticker folders
-h, --help            Print help
```

Run from this repository with Cargo:

```sh
cargo run -- --aliases ../aliases.txt --data ../data
```

Preview changes without modifying files:

```sh
cargo run -- --dry-run
```

## Alias File Format

Each non-empty, non-comment line must use this format:

```text
OLD --> NEW
```

Example:

```text
# Ticker renames
FB --> META
TWTR --> X
```

## Data Layout

The data directory is expected to contain one folder per ticker:

```text
data/
  FB/
    FB-prices.csv
    FB-dividends.csv
  META/
    META-prices.csv
```

For an alias like `FB --> META`, the tool looks for CSV files in `data/FB/` whose names start with `FB-`. Each file is moved or merged into `data/META/` with the prefix changed to `META-`.

## Merge Behavior

`--plan` reports each operation as JSON with `source` and `target` paths relative to the data directory and a `merge` boolean indicating whether the target exists. It does not create or alter files.

Aliases are handled one at a time. Files within each ticker are validated and merged concurrently, up to `--jobs`.

Non-dry runs hold an exclusive lock on `data/.merge-aliases.lock` for the duration of validation and merging. A concurrent `merge-aliases` run against the same data directory fails instead of changing files at the same time. The lock file stays in place after the run. Dry runs do not create or acquire it, and other programs must coordinate separately.

- If the target file does not exist, the old file is renamed into the new ticker folder.
- If the target file exists, both files are merged in ascending `open_time` order.
- If both files contain the same timestamp, the new ticker row wins.
- Every input file must have strictly increasing numeric timestamps. Inputs and alias mappings are checked before any files are moved. Alias chains, cycles, repeated sources, and repeated targets are rejected; resolve those mappings separately.
- After a successful merge, the old source CSV is removed.
- The old ticker folder is removed if it is empty, unless `--keep-old` is used.

## Testing

```sh
cargo test
```
