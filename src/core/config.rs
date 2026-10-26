//! The config file (spec §4.1). Only `[vocabulary] language` is read so far.

use super::Error;
use oxigraph::model::Literal;
use serde::Deserialize;
use std::path::Path;

pub struct Config {
    /// The language tag of labels, lowercase.
    pub language: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            language: "en".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct Raw {
    vocabulary: Option<Vocabulary>,
}

#[derive(Deserialize)]
struct Vocabulary {
    language: Option<String>,
}

/// Parses the text of the config file at `path`.
pub fn parse(text: &str, path: &Path) -> Result<Config, Error> {
    let raw: Raw = toml::from_str(text)
        .map_err(|_| Error::config_invalid(path, "not valid TOML, or a key has the wrong type"))?;
    let Some(language) = raw.vocabulary.and_then(|v| v.language) else {
        return Ok(Config::default());
    };
    // oxrdf checks BCP 47 well-formedness and normalises the tag to lowercase.
    let literal = Literal::new_language_tagged_literal("", &language)
        .map_err(|_| Error::config_invalid(path, "[vocabulary] language is not a BCP 47 tag"))?;
    Ok(Config {
        language: literal.language().unwrap_or("en").to_string(),
    })
}
