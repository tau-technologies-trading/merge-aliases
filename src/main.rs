fn main() {
    if let Err(err) = merge_aliases::run_cli() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
