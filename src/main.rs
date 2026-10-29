use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
struct Config {
    aliases_path: PathBuf,
    data_path: PathBuf,
    dry_run: bool,
    keep_old: bool,
    jobs: usize,
    plan: bool,
}

#[derive(Debug)]
struct Alias {
    old: String,
    new: String,
}

#[derive(Debug)]
struct TickerFile {
    source: PathBuf,
    target: PathBuf,
}

fn main() {
    if env::args_os()
        .skip(1)
        .any(|arg| arg == "-V" || arg == "--version")
    {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args(env::args().skip(1))?;
    let aliases = read_aliases(&config.aliases_path)?;

    if config.plan {
        validate_aliases(&aliases)?;
        return print_plan(&config, &aliases);
    }

    if aliases.is_empty() {
        println!("no aliases found in {}", config.aliases_path.display());
        return Ok(());
    }

    validate_aliases(&aliases)?;
    let _lock = if config.dry_run {
        None
    } else {
        Some(lock_data_directory(&config.data_path)?)
    };
    // Check every input before moving or replacing any file.
    for alias in &aliases {
        let old_dir = config.data_path.join(&alias.old);
        let new_dir = config.data_path.join(&alias.new);
        if old_dir.exists() && !old_dir.is_dir() {
            return Err(
                format!("old ticker path is not a directory: {}", old_dir.display()).into(),
            );
        }
        if new_dir.exists() && !new_dir.is_dir() {
            return Err(
                format!("new ticker path is not a directory: {}", new_dir.display()).into(),
            );
        }
        if old_dir.is_dir() {
            let files = collect_ticker_files(&old_dir, &new_dir, &alias.old, &alias.new)?;
            parallel_files(&files, config.jobs, |file| {
                let mut old = BufReader::new(File::open(&file.source)?);
                if file.target.exists() {
                    let mut new = BufReader::new(File::open(&file.target)?);
                    merge_readers_by_timestamp(&mut old, &mut new, &mut io::sink())?;
                } else {
                    merge_readers_by_timestamp(&mut old, &mut io::empty(), &mut io::sink())?;
                }
                Ok(())
            })?;
        }
    }

    for alias in aliases {
        process_alias(&config, &alias)?;
    }

    Ok(())
}

fn print_plan(config: &Config, aliases: &[Alias]) -> Result<(), Box<dyn std::error::Error>> {
    let operations = plan_operations(config, aliases)?;
    println!("{}", serde_json::json!({ "operations": operations }));
    Ok(())
}

fn plan_operations(
    config: &Config,
    aliases: &[Alias],
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let mut operations = Vec::new();
    for alias in aliases {
        let old_dir = config.data_path.join(&alias.old);
        let new_dir = config.data_path.join(&alias.new);
        if !old_dir.exists() {
            continue;
        }
        if !old_dir.is_dir() {
            return Err(
                format!("old ticker path is not a directory: {}", old_dir.display()).into(),
            );
        }
        if new_dir.exists() && !new_dir.is_dir() {
            return Err(
                format!("new ticker path is not a directory: {}", new_dir.display()).into(),
            );
        }
        for file in collect_ticker_files(&old_dir, &new_dir, &alias.old, &alias.new)? {
            let source = file
                .source
                .strip_prefix(&config.data_path)?
                .to_str()
                .ok_or("planned source path is not UTF-8")?;
            let target = file
                .target
                .strip_prefix(&config.data_path)?
                .to_str()
                .ok_or("planned target path is not UTF-8")?;
            operations.push(serde_json::json!({
                "source": source,
                "target": target,
                "merge": file.target.exists(),
            }));
        }
    }
    Ok(operations)
}

fn lock_data_directory(data_path: &Path) -> Result<File, Box<dyn std::error::Error>> {
    let lock_path = data_path.join(".merge-aliases.lock");
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|err| format!("cannot open lock file {}: {err}", lock_path.display()))?;
    file.try_lock().map_err(|err| {
        format!(
            "cannot lock {}: {err}; another merge may be running",
            lock_path.display()
        )
    })?;
    Ok(file)
}

fn validate_aliases(aliases: &[Alias]) -> Result<(), Box<dyn std::error::Error>> {
    let mut sources = std::collections::HashSet::new();
    let mut targets = std::collections::HashSet::new();
    for alias in aliases {
        for ticker in [&alias.old, &alias.new] {
            if ticker.is_empty()
                || !ticker
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(format!("invalid ticker name: {ticker}").into());
            }
        }
        if !sources.insert(&alias.old) {
            return Err(format!("duplicate alias source: {}", alias.old).into());
        }
        if !targets.insert(&alias.new) {
            return Err(format!("duplicate alias target: {}", alias.new).into());
        }
    }
    for alias in aliases {
        if sources.contains(&alias.new) {
            return Err(format!(
                "alias chains or cycles are not supported: {} -> {}",
                alias.old, alias.new
            )
            .into());
        }
    }
    Ok(())
}

fn parse_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let mut aliases_path = PathBuf::from("../aliases.txt");
    let mut data_path = PathBuf::from("../data");
    let mut dry_run = false;
    let mut keep_old = false;
    let mut jobs = 4usize;
    let mut plan = false;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--aliases" | "-a" => {
                aliases_path = PathBuf::from(
                    args.next()
                        .ok_or_else(|| format!("{arg} requires a path argument"))?,
                );
            }
            "--data" | "-d" => {
                data_path = PathBuf::from(
                    args.next()
                        .ok_or_else(|| format!("{arg} requires a path argument"))?,
                );
            }
            "--jobs" | "-j" => {
                jobs = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a number"))?
                    .parse()
                    .map_err(|_| format!("{arg} requires a positive integer"))?;
                if jobs == 0 {
                    return Err(format!("{arg} must be greater than zero"));
                }
            }
            "--dry-run" => dry_run = true,
            "--plan" => plan = true,
            "--keep-old" => keep_old = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Config {
        aliases_path,
        data_path,
        dry_run,
        keep_old,
        jobs,
        plan,
    })
}

fn print_usage() {
    println!(
        "Usage: merge-aliases [OPTIONS]\n\
\n\
Options:\n\
  -a, --aliases <PATH>  Alias file path [default: ../aliases.txt]\n\
  -d, --data <PATH>     Data directory path [default: ../data]\n\
  -j, --jobs <N>        Maximum files processed in parallel [default: 4]\n\
      --dry-run         Print actions without modifying files\n\
      --plan            Print planned operations as JSON without modifying files\n\
      --keep-old        Do not remove empty old ticker folders\n\
  -V, --version         Print version\n\
  -h, --help            Print this help"
    );
}

fn read_aliases(path: &Path) -> Result<Vec<Alias>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut aliases = Vec::new();

    for (line_number, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((old, new)) = line.split_once("-->") else {
            return Err(format!(
                "{}:{}: expected alias format `OLD --> NEW`",
                path.display(),
                line_number + 1
            )
            .into());
        };

        let old = old.trim();
        let new = new.trim();
        if old.is_empty() || new.is_empty() {
            return Err(format!(
                "{}:{}: old and new ticker names must be non-empty",
                path.display(),
                line_number + 1
            )
            .into());
        }
        if old == new {
            return Err(format!(
                "{}:{}: old and new ticker names are identical: {old}",
                path.display(),
                line_number + 1
            )
            .into());
        }

        aliases.push(Alias {
            old: old.to_string(),
            new: new.to_string(),
        });
    }

    Ok(aliases)
}

fn process_alias(config: &Config, alias: &Alias) -> Result<(), Box<dyn std::error::Error>> {
    let old_dir = config.data_path.join(&alias.old);
    let new_dir = config.data_path.join(&alias.new);

    if !old_dir.exists() {
        println!("skip {} -> {}: old folder missing", alias.old, alias.new);
        return Ok(());
    }
    if !old_dir.is_dir() {
        return Err(format!("old ticker path is not a directory: {}", old_dir.display()).into());
    }

    if config.dry_run {
        if !new_dir.exists() {
            println!("would create {}", new_dir.display());
        }
    } else {
        fs::create_dir_all(&new_dir)?;
    }

    let files = collect_ticker_files(&old_dir, &new_dir, &alias.old, &alias.new)?;
    if files.is_empty() {
        println!("no CSV files found for {}", alias.old);
    }

    parallel_files(&files, config.jobs, |file| {
        process_ticker_file(config, file)
    })?;

    if config.keep_old {
        return Ok(());
    }

    if config.dry_run {
        println!("would remove {} if empty", old_dir.display());
        return Ok(());
    }

    match fs::remove_dir(&old_dir) {
        Ok(()) => println!("removed empty folder {}", old_dir.display()),
        Err(err) if err.kind() == io::ErrorKind::DirectoryNotEmpty => {
            println!("kept non-empty folder {}", old_dir.display());
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    Ok(())
}

fn parallel_files<F>(
    files: &[TickerFile],
    jobs: usize,
    work: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(&TickerFile) -> Result<(), Box<dyn std::error::Error>> + Sync,
{
    let next = std::sync::atomic::AtomicUsize::new(0);
    let error = std::sync::Mutex::new(None::<String>);
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(files.len()) {
            let next = &next;
            let error = &error;
            let work = &work;
            scope.spawn(move || {
                loop {
                    if error.lock().unwrap().is_some() {
                        break;
                    }
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(file) = files.get(index) else {
                        break;
                    };
                    if let Err(err) = work(file) {
                        let mut first = error.lock().unwrap();
                        if first.is_none() {
                            *first = Some(format!("{}: {err}", file.source.display()));
                        }
                        break;
                    }
                }
            });
        }
    });
    if let Some(error) = error.into_inner().unwrap() {
        return Err(error.into());
    }
    Ok(())
}

fn process_ticker_file(
    config: &Config,
    file: &TickerFile,
) -> Result<(), Box<dyn std::error::Error>> {
    if file.target.exists() {
        println!(
            "merge rows {} into {}",
            file.source.display(),
            file.target.display()
        );
        if !config.dry_run {
            merge_by_timestamp(&file.source, &file.target)?;
            fs::remove_file(&file.source)?;
        }
    } else {
        println!(
            "move {} to {}",
            file.source.display(),
            file.target.display()
        );
        if !config.dry_run {
            fs::rename(&file.source, &file.target)?;
        }
    }
    Ok(())
}

fn collect_ticker_files(
    old_dir: &Path,
    new_dir: &Path,
    old: &str,
    new: &str,
) -> Result<Vec<TickerFile>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    let prefix = format!("{old}-");

    for entry in fs::read_dir(old_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let file_name = file_name.to_string();

        if !file_name.ends_with(".csv") {
            continue;
        }
        let Some(rest) = file_name.strip_prefix(&prefix) else {
            eprintln!(
                "skip unexpected filename in {}: {file_name}",
                old_dir.display()
            );
            continue;
        };

        let target = new_dir.join(format!("{new}-{rest}"));
        files.insert(
            rest.to_string(),
            TickerFile {
                source: path,
                target,
            },
        );
    }

    Ok(files.into_values().collect())
}

fn merge_by_timestamp(old_path: &Path, new_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let old_file = File::open(old_path)?;
    let new_file = File::open(new_path)?;

    let mut old_reader = BufReader::new(old_file);
    let mut new_reader = BufReader::new(new_file);
    let temp_path = temp_path_for(new_path)?;
    let temp_file = File::create(&temp_path)?;
    let mut writer = BufWriter::new(temp_file);

    let merge_result = merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut writer);
    if let Err(err) = merge_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }

    if let Err(err) = writer.flush() {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }

    if let Err(err) = fs::rename(&temp_path, new_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }

    Ok(())
}

fn merge_readers_by_timestamp(
    old_reader: &mut impl BufRead,
    new_reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> io::Result<()> {
    fn next(
        reader: &mut impl BufRead,
        previous: &mut Option<i64>,
    ) -> io::Result<Option<(i64, String)>> {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let timestamp = line
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i64>()
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid open_time in alias input",
                )
            })?;
        if previous.is_some_and(|last| timestamp <= last) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "alias input timestamps must be strictly increasing",
            ));
        }
        *previous = Some(timestamp);
        Ok(Some((timestamp, line)))
    }
    let (mut old_prev, mut new_prev) = (None, None);
    let mut old = next(old_reader, &mut old_prev)?;
    let mut new = next(new_reader, &mut new_prev)?;
    while old.is_some() || new.is_some() {
        match (&old, &new) {
            (Some((a, line)), Some((b, _))) if a < b => {
                write_line(writer, line)?;
                old = next(old_reader, &mut old_prev)?;
            }
            (Some((a, _)), Some((b, line))) if a == b => {
                write_line(writer, line)?;
                old = next(old_reader, &mut old_prev)?;
                new = next(new_reader, &mut new_prev)?;
            }
            (_, Some((_, line))) => {
                write_line(writer, line)?;
                new = next(new_reader, &mut new_prev)?;
            }
            (Some((_, line)), None) => {
                write_line(writer, line)?;
                old = next(old_reader, &mut old_prev)?;
            }
            (None, None) => break,
        }
    }

    Ok(())
}

fn write_line(writer: &mut impl Write, line: &str) -> io::Result<()> {
    writer.write_all(line.as_bytes())?;
    if !line.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn temp_path_for(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid file path: {}", path.display()))?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(path.with_file_name(format!(
        ".{file_name}.merge-aliases-{}-{nanos}.tmp",
        std::process::id()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn merge_interleaves_sorted_inputs() {
        let old = b"100,old-a\n200,old-b\n";
        let new = b"150,new-c\n250,new-d\n";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        assert_eq!(output, b"100,old-a\n150,new-c\n200,old-b\n250,new-d\n");
    }

    #[test]
    fn merge_new_wins_on_conflict() {
        let old = b"100,old-data\n200,old-data\n";
        let new = b"100,new-data\n";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        let expected = b"100,new-data\n200,old-data\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn merge_with_one_empty_file() {
        let old = b"100,a\n200,b\n";
        let new = b"";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        assert_eq!(output, b"100,a\n200,b\n");
    }

    #[test]
    fn rejects_unordered_or_duplicate_timestamps() {
        for old in [b"2,a\n1,b\n".as_slice(), b"1,a\n1,b\n".as_slice()] {
            let mut source = Cursor::new(old);
            let mut target = Cursor::new(b"");
            assert!(merge_readers_by_timestamp(&mut source, &mut target, &mut Vec::new()).is_err());
        }
    }

    #[test]
    fn rejects_conflicting_or_chained_aliases() {
        let alias = |old: &str, new: &str| Alias {
            old: old.into(),
            new: new.into(),
        };
        assert!(validate_aliases(&[alias("A", "B"), alias("A", "C")]).is_err());
        assert!(validate_aliases(&[alias("A", "B"), alias("B", "C")]).is_err());
        assert!(validate_aliases(&[alias("../A", "B")]).is_err());
    }

    #[test]
    fn lock_prevents_concurrent_merges_and_releases_on_drop() {
        let root = env::temp_dir().join(format!(
            "merge-aliases-lock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let first = lock_data_directory(&root).unwrap();
        assert!(lock_data_directory(&root).is_err());
        drop(first);
        drop(lock_data_directory(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn processes_multiple_files_in_one_alias_folder() {
        let root = env::temp_dir().join(format!(
            "merge-aliases-files-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = root.join("OLD");
        let new = root.join("NEW");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        for month in ["01", "02", "03"] {
            fs::write(
                old.join(format!("OLD-1s-2024-{month}.csv")),
                "1,old\n3,old\n",
            )
            .unwrap();
            fs::write(
                new.join(format!("NEW-1s-2024-{month}.csv")),
                "2,new\n3,new\n",
            )
            .unwrap();
        }
        let config = Config {
            aliases_path: root.join("aliases"),
            data_path: root.clone(),
            dry_run: false,
            keep_old: false,
            jobs: 3,
            plan: false,
        };
        process_alias(
            &config,
            &Alias {
                old: "OLD".into(),
                new: "NEW".into(),
            },
        )
        .unwrap();
        for month in ["01", "02", "03"] {
            assert_eq!(
                fs::read_to_string(new.join(format!("NEW-1s-2024-{month}.csv"))).unwrap(),
                "1,old\n2,new\n3,new\n"
            );
        }
        assert!(!old.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_reports_merges_and_moves_without_modifying_files() {
        let root = env::temp_dir().join(format!(
            "merge-aliases-plan-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old = root.join("OLD");
        let new = root.join("NEW");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        for month in ["01", "02"] {
            fs::write(old.join(format!("OLD-1s-2024-{month}.csv")), "original\n").unwrap();
        }
        fs::write(new.join("NEW-1s-2024-01.csv"), "target\n").unwrap();
        let config = Config {
            aliases_path: root.join("aliases"),
            data_path: root.clone(),
            dry_run: false,
            keep_old: false,
            jobs: 2,
            plan: true,
        };
        let plan = plan_operations(
            &config,
            &[Alias {
                old: "OLD".into(),
                new: "NEW".into(),
            }],
        )
        .unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0]["merge"], true);
        assert_eq!(plan[1]["merge"], false);
        assert_eq!(
            Path::new(plan[0]["source"].as_str().unwrap()),
            Path::new("OLD").join("OLD-1s-2024-01.csv")
        );
        assert_eq!(
            fs::read_to_string(old.join("OLD-1s-2024-01.csv")).unwrap(),
            "original\n"
        );
        assert!(!root.join(".merge-aliases.lock").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merge_both_empty() {
        let old = b"";
        let new = b"";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        assert_eq!(output, b"");
    }
}
