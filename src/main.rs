fn main() {
    if let Err(err) = forgentex::cli::run(std::env::args().collect()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
