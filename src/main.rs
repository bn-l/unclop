use clap::Parser;

fn main() {
    let cli = unclop::cli::Cli::parse();
    match unclop::commands::dispatch(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("unclop: {err:#}");
            std::process::exit(70);
        }
    }
}
