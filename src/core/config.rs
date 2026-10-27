//! The config file (spec §4.1). Only `[user] name` and `[vocabulary] language` are read so far.

use super::Error;
use oxigraph::model::Literal;
use serde::Deserialize;
use std::path::Path;

pub struct Config {
    /// The language tag of labels, lowercase.
    pub language: String,
    /// The `foaf:name` of the user Person, if configured.
    pub user_name: Option<String>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            language: "en".to_string(),
            user_name: None,
        }
    }
}

#[derive(Deserialize)]
struct Raw {
    user: Option<User>,
    vocabulary: Option<Vocabulary>,
}

#[derive(Deserialize)]
struct User {
    name: Option<String>,
}

#[derive(Deserialize)]
struct Vocabulary {
    language: Option<String>,
}

/// Parses the text of the config file at `path`.
pub fn parse(text: &str, path: &Path) -> Result<Config, Error> {
    let raw: Raw = toml::from_str(text)
        .map_err(|_| Error::config_invalid(path, "not valid TOML, or a key has the wrong type"))?;
    let user_name = raw.user.and_then(|u| u.name);
    if let Some(name) = &user_name
        && (name.trim().is_empty() || name.chars().any(char::is_control))
    {
        return Err(Error::config_invalid(
            path,
            "[user] name must be non-empty, without control characters",
        ));
    }
    let language = match raw.vocabulary.and_then(|v| v.language) {
        // oxrdf checks BCP 47 well-formedness and normalises the tag to lowercase.
        Some(language) => Literal::new_language_tagged_literal("", &language)
            .map_err(|_| Error::config_invalid(path, "[vocabulary] language is not a BCP 47 tag"))?
            .language()
            .unwrap_or("en")
            .to_string(),
        None => "en".to_string(),
    };
    Ok(Config {
        language,
        user_name,
    })
}

/// The name a newly minted user Person gets: config `[user] name`, else `$USER`, else
/// `$LOGNAME`, else `user`. Empty variables, and ones with control characters, are skipped.
pub fn user_name(config: &Config, env: impl Fn(&str) -> Option<String>) -> String {
    let usable = |v: &String| !v.trim().is_empty() && !v.chars().any(char::is_control);
    config
        .user_name
        .clone()
        .or_else(|| env("USER").filter(usable))
        .or_else(|| env("LOGNAME").filter(usable))
        .unwrap_or_else(|| "user".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn user_name_precedence() {
        let path = Path::new("config.toml");
        let named = parse("[user]\nname = \"Nate\"\n", path).unwrap();
        let env = env_of(&[("USER", "alice"), ("LOGNAME", "carol")]);
        assert_eq!(user_name(&named, &env), "Nate");
        let plain = Config::default();
        assert_eq!(user_name(&plain, &env), "alice");
        assert_eq!(
            user_name(&plain, env_of(&[("USER", ""), ("LOGNAME", "carol")])),
            "carol"
        );
        assert_eq!(user_name(&plain, env_of(&[])), "user");
    }

    #[test]
    fn bad_user_names_are_config_invalid() {
        for text in ["[user]\nname = \"\"\n", "[user]\nname = \"a\\u0001b\"\n"] {
            let err = parse(text, Path::new("c.toml")).err().unwrap();
            assert_eq!(err.rule, "CONFIG_INVALID", "{text:?}");
        }
    }
}
