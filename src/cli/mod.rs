use clap::Parser;

#[derive(Parser)]
#[command(name = "hali", version, about)]
struct Cli {}

pub fn run() {
    Cli::parse();
}
