mod app;
mod cli;
mod events;
mod iii;
mod payload;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    let mut stdout = std::io::stdout();
    if let Err(err) = app::run(cli, iii::ProcessRunner::new(), &mut stdout) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
