fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--probe") {
        println!("{}", forgentex::lsp::server_info_json());
        return;
    }
    if let Err(err) = forgentex::lsp::run_stdio() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
