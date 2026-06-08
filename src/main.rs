use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
struct Config {
    aliases_path: PathBuf,
    data_path: PathBuf,
    dry_run: bool,
    keep_old: bool,
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
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args(env::args().skip(1))?;
    let aliases = read_aliases(&config.aliases_path)?;

    if aliases.is_empty() {
        println!("no aliases found in {}", config.aliases_path.display());
        return Ok(());
    }

    for alias in aliases {
        process_alias(&config, &alias)?;
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
            "--dry-run" => dry_run = true,
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
    })
}

fn print_usage() {
    println!(
        "Usage: merge-aliases [OPTIONS]\n\
\n\
Options:\n\
  -a, --aliases <PATH>  Alias file path [default: ../aliases.txt]\n\
  -d, --data <PATH>     Data directory path [default: ../data]\n\
      --dry-run         Print actions without modifying files\n\
      --keep-old        Do not remove empty old ticker folders\n\
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

    for file in files {
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
    }

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
            println!(
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
    let mut line = String::new();

    while old_reader.read_line(&mut line)? > 0 {
        write_line(writer, &line)?;
        line.clear();
    }

    while new_reader.read_line(&mut line)? > 0 {
        write_line(writer, &line)?;
        line.clear();
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
    fn merge_appends_new_to_old_without_sorting() {
        let old = b"100,old-a\n200,old-b\n";
        let new = b"150,new-c\n250,new-d\n";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        assert_eq!(output, b"100,old-a\n200,old-b\n150,new-c\n250,new-d\n");
    }

    #[test]
    fn merge_new_wins_on_conflict() {
        let old = b"100,old-data\n200,old-data\n";
        let new = b"100,new-data\n";
        let mut old_reader = Cursor::new(old);
        let mut new_reader = Cursor::new(new);
        let mut output = Vec::new();

        merge_readers_by_timestamp(&mut old_reader, &mut new_reader, &mut output).unwrap();

        let expected = b"100,old-data\n200,old-data\n100,new-data\n";
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
