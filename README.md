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
    --dry-run         Print actions without modifying files
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

- If the target file does not exist, the old file is renamed into the new ticker folder.
- If the target file exists, rows are merged by row number.
- Existing rows in the target file are preferred.
- Extra rows from the old file are appended when the old file has more rows than the target file.
- After a successful merge, the old source CSV is removed.
- The old ticker folder is removed if it is empty, unless `--keep-old` is used.

## Testing

```sh
cargo test
```
