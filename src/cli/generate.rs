//! `hali generate`: the man page and shell completions, from the clap definitions (spec §9.1,
//! §13.2). The name is always the literal `hali`; nothing here reads the environment, the
//! program name, the config or the store.

use clap::Subcommand;
use std::io::Write;
use std::process::ExitCode;

/// The shells hali ships completions for.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum GenShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
pub enum Generate {
    /// Print the man page (roff) for hali(1)
    Man,
    /// Print the completion script for a shell
    Completions {
        /// The shell to generate for
        #[arg(value_enum, value_name = "SHELL")]
        shell: GenShell,
    },
}

/// The command with its hidden subcommands removed, so completions never offer them.
fn without_hidden(command: clap::Command) -> clap::Command {
    let visible: Vec<clap::Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .cloned()
        .collect();
    let mut rebuilt = clap::Command::new("hali")
        .bin_name("hali")
        .args(command.get_arguments().cloned())
        .subcommands(visible);
    if let Some(about) = command.get_about() {
        rebuilt = rebuilt.about(about.clone());
    }
    rebuilt
}

pub fn render_man() -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    clap_mangen::Man::new(super::command()).render(&mut out)?;
    Ok(out)
}

pub fn render_completions(shell: GenShell) -> Vec<u8> {
    let shell = match shell {
        GenShell::Bash => clap_complete::Shell::Bash,
        GenShell::Zsh => clap_complete::Shell::Zsh,
        GenShell::Fish => clap_complete::Shell::Fish,
    };
    let mut out = Vec::new();
    let mut command = without_hidden(super::command());
    clap_complete::generate(shell, &mut command, "hali", &mut out);
    out
}

pub fn run(what: &Generate) -> ExitCode {
    let bytes = match what {
        Generate::Man => match render_man() {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(1);
            }
        },
        Generate::Completions { shell } => render_completions(*shell),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
