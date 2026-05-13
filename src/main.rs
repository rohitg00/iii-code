mod app;
mod cli;
mod events;
mod iii;
mod payload;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    if let Err(err) = app::run_with_input(cli, iii::ProcessRunner::new(), stdin.lock(), &mut stdout)
    {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
