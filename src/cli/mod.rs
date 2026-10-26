//! The CLI adapter: clap definitions, wiring of the seams, text/JSON rendering, exit codes.

use crate::core::{self, Clock, Context, Error, IdGen, concept, validate};
use crate::{ontology, paths, storage};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::process::ExitCode;

/// The JSON contract version (spec §9.3).
const SCHEMA_VERSION: u32 = 1;

#[derive(Parser)]
#[command(name = "hali", version, about)]
struct Cli {
    /// Print JSON instead of text
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage the Concepts of the topics scheme
    #[command(subcommand)]
    Concept(ConceptCommand),
    /// Check the store against the SHACL shapes; writes nothing
    Validate {
        /// Maximum number of results
        #[arg(long, default_value_t = 50, allow_hyphen_values = true, value_parser = parse_limit)]
        limit: usize,
        /// Results to skip
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        offset: usize,
    },
}

#[derive(Subcommand)]
enum ConceptCommand {
    /// Add a Concept
    Add {
        /// Its preferred label
        #[arg(value_parser = parse_label)]
        label: String,
    },
    /// List Concepts, sorted by label
    List {
        /// Maximum number of results
        #[arg(long, default_value_t = 50, allow_hyphen_values = true, value_parser = parse_limit)]
        limit: usize,
        /// Results to skip
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        offset: usize,
    },
}

/// Rejects bad labels at parse time, so they are usage errors in clap's format.
fn parse_label(label: &str) -> Result<String, String> {
    core::normalise_label(label).map_err(|e| e.message)
}

/// A limit below 1 would page forever (`next_offset` would never advance).
fn parse_limit(limit: &str) -> Result<usize, String> {
    match limit.parse::<usize>() {
        Ok(0) => Err("the limit must be at least 1".to_string()),
        Ok(n) => Ok(n),
        Err(e) => Err(e.to_string()),
    }
}

/// Makes stored text safe to print: control and bidi/format characters become a visible
/// `\u{..}` escape, so a hand-edited store cannot drive the terminal or forge rows.
fn printable(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let unsafe_char = c.is_control()
            || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        if unsafe_char {
            write!(out, "\\u{{{:x}}}", u32::from(c)).expect("writing to a String");
        } else {
            out.push(c);
        }
    }
    out
}

/// An IRI (bare or in N-Triples form) as a prefixed name from the fixed prefix table, when its
/// local part is a plain name; anything else is returned as given.
fn compact(term: &str) -> String {
    let iri = term
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(term);
    for (name, ns) in ontology::PREFIXES {
        if let Some(local) = iri.strip_prefix(ns) {
            let plain = !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'));
            if plain {
                return format!("{name}:{local}");
            }
        }
    }
    term.to_string()
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let (clock, ids) = match injected() {
        Ok(injected) => injected,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(3);
        }
    };
    match execute(&cli, clock, ids) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&err, cli.json);
            ExitCode::from(err.exit_code())
        }
    }
}

fn execute(cli: &Cli, clock: Box<dyn Clock>, ids: Box<dyn IdGen>) -> Result<(), Error> {
    let config = match paths::config_file(env) {
        Ok(path) => match storage::read_text(&path) {
            Ok(Some(text)) => core::config::parse(&text, &path)?,
            Ok(None) => core::config::Config::default(),
            Err(_) => return Err(Error::config_invalid(&path, "cannot read it")),
        },
        Err(paths::Unresolved) => core::config::Config::default(),
    };
    let ctx = Context::new(&config.language, clock, ids);
    let dir = paths::data_dir(env).map_err(|_| Error::store_io(None))?;
    let mut store = storage::load(&dir)?;

    match &cli.command {
        Command::Concept(ConceptCommand::Add { label }) => {
            let added = concept::add(&ctx, &mut store, label)?;
            storage::save(&dir, &store, storage::File::Vocabulary)?;
            if cli.json {
                print_json(&AddOutput {
                    schema_version: SCHEMA_VERSION,
                    concept: &added,
                });
            } else {
                println!(
                    "added concept {} \"{}\"",
                    printable(&added.id),
                    printable(&added.label)
                );
            }
        }
        Command::Concept(ConceptCommand::List { limit, offset }) => {
            let page = concept::list(&ctx, &store, *offset, *limit);
            if cli.json {
                print_json(&ListOutput {
                    schema_version: SCHEMA_VERSION,
                    page: &page,
                });
            } else {
                for c in &page.results {
                    println!("{}\t{}", printable(&c.id), printable(&c.label));
                }
                if let Some(next) = page.next_offset {
                    eprintln!(
                        "note: showing {} of {}; next: --offset {next}",
                        page.results.len(),
                        page.total
                    );
                }
            }
        }
        Command::Validate { limit, offset } => {
            let report = validate::validate(&store, *offset, *limit)?;
            if cli.json {
                print_json(&ValidateOutput {
                    schema_version: SCHEMA_VERSION,
                    report: &report,
                });
            } else if report.conforms {
                println!("the store conforms to the SHACL shapes");
            } else {
                let dash = || "-".to_string();
                for v in &report.results {
                    println!(
                        "{}\t{}\t{}\t{}",
                        printable(&compact(&v.focus_node)),
                        printable(&v.shape.as_deref().map_or_else(dash, compact)),
                        printable(&v.value.as_deref().map_or_else(dash, compact)),
                        printable(&v.message)
                    );
                }
                if let Some(next) = report.next_offset {
                    eprintln!(
                        "note: showing {} of {}; next: --offset {next}",
                        report.results.len(),
                        report.total
                    );
                }
            }
            if let Some(err) = report.error() {
                return Err(err);
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct AddOutput<'a> {
    schema_version: u32,
    concept: &'a concept::Concept,
}

#[derive(Serialize)]
struct ListOutput<'a> {
    schema_version: u32,
    #[serde(flatten)]
    page: &'a concept::Page,
}

#[derive(Serialize)]
struct ValidateOutput<'a> {
    schema_version: u32,
    #[serde(flatten)]
    report: &'a validate::Report,
}

#[derive(Serialize)]
struct ErrorOutput<'a> {
    schema_version: u32,
    error: &'a Error,
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string(value).expect("output serializes")
    );
}

/// Writes an error to stderr: the JSON envelope, or `error:` and `hint:` lines.
fn report(err: &Error, json: bool) {
    if json {
        let envelope = ErrorOutput {
            schema_version: SCHEMA_VERSION,
            error: err,
        };
        eprintln!(
            "{}",
            serde_json::to_string(&envelope).expect("errors serialize")
        );
        return;
    }
    eprintln!("error: {}", err.message);
    if let Some(fix) = &err.suggested_fix {
        match &err.suggested_command {
            Some(cmd) => eprintln!("hint: {fix} Try: {cmd}"),
            None => eprintln!("hint: {fix}"),
        }
    }
}

type Injected = (Box<dyn Clock>, Box<dyn IdGen>);

/// The clock and id generator: the system ones, unless a test-hooks build is steered by
/// `HALI_TEST_NOW` / `HALI_TEST_ID_SEED` (spec §14.2).
#[cfg(not(feature = "test-hooks"))]
fn injected() -> Result<Injected, String> {
    Ok((Box::new(core::SystemClock), Box::new(core::RandomIdGen)))
}

#[cfg(feature = "test-hooks")]
fn injected() -> Result<Injected, String> {
    let clock: Box<dyn Clock> = match env("HALI_TEST_NOW") {
        Some(now) if is_rfc3339_utc(&now) => Box::new(core::FixedClock::new(&now)),
        Some(_) => return Err("HALI_TEST_NOW must look like 2026-10-24T09:30:00Z".to_string()),
        None => Box::new(core::SystemClock),
    };
    let ids: Box<dyn IdGen> = match env("HALI_TEST_ID_SEED") {
        Some(seed) => match seed.parse::<u64>() {
            Ok(seed) => Box::new(core::SeededIdGen::new(seed)),
            Err(_) => return Err("HALI_TEST_ID_SEED must be a decimal u64".to_string()),
        },
        None => Box::new(core::RandomIdGen),
    };
    Ok((clock, ids))
}

/// `YYYY-MM-DDTHH:MM:SSZ` with in-range fields.
#[cfg(feature = "test-hooks")]
fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    let num = |from: usize, to: usize| -> Option<u32> {
        b.get(from..to)?.iter().try_fold(0u32, |n, &c| {
            c.is_ascii_digit().then(|| n * 10 + u32::from(c - b'0'))
        })
    };
    let fields = (|| {
        Some((
            num(0, 4)?,
            num(5, 7)?,
            num(8, 10)?,
            num(11, 13)?,
            num(14, 16)?,
            num(17, 19)?,
        ))
    })();
    b.len() == 20
        && [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'Z'),
        ]
        .iter()
        .all(|&(i, c)| b[i] == c)
        && matches!(fields, Some((_, mo, d, h, mi, sec))
            if (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && sec < 60)
}

#[cfg(test)]
mod tests {
    use super::{compact, printable};

    #[test]
    fn compact_uses_the_prefix_table_for_plain_local_names() {
        assert_eq!(compact("<https://w3id.org/hali#ml>"), "hali:ml");
        assert_eq!(
            compact("https://w3id.org/hali#LabelUniqueShape"),
            "hali:LabelUniqueShape"
        );
        assert_eq!(
            compact("<https://w3id.org/hali#a/b>"),
            "<https://w3id.org/hali#a/b>"
        );
        assert_eq!(compact("<http://example.org/x>"), "<http://example.org/x>");
        assert_eq!(compact("\"x\"@en"), "\"x\"@en");
    }

    #[test]
    fn printable_escapes_controls_and_keeps_letters() {
        assert_eq!(
            printable("a\u{1b}b\u{7f}\u{85}\u{202e}\n"),
            "a\\u{1b}b\\u{7f}\\u{85}\\u{202e}\\u{a}"
        );
        assert_eq!(printable("café 日本"), "café 日本");
    }
}
