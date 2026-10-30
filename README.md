# merge-aliases

Library for merging CSV data folders after a ticker rename.

## Use

Add the crate to your Rust project:

```toml
merge-aliases = "0.2.1"
```

The aliases file uses one `OLD --> NEW` mapping per line. Call `plan_json` for a read-only JSON plan, then `merge_folder` to process one data directory:

```rust,ignore
let plan = merge_aliases::plan_json(
    std::path::Path::new("aliases.txt"),
    std::path::Path::new("data"),
)?;
merge_aliases::merge_folder(
    std::path::Path::new("aliases.txt"),
    std::path::Path::new("data"),
    4,
)?;
```

The data directory contains one folder per ticker. Matching `OLD-*.csv` files are moved into the new ticker folder with the prefix changed. If a target file already exists, rows are merged by timestamp and the target's row wins on duplicate timestamps. Source files are removed only after a successful merge. Empty old ticker folders are removed after processing.

Alias lines beginning with `#` are comments. Empty lines are ignored.
