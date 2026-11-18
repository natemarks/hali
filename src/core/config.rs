//! The config file (spec §4.1). Only `[user] name`, `[vocabulary] language`, `[files] dir` and
//! the `[scan]` keys `exclude`, `include_hidden` and `max_size` are read so far.

use super::Error;
use oxigraph::model::Literal;
use serde::Deserialize;
use std::ffi::OsStr;
use std::path::Path;

pub struct Config {
    /// The language tag of labels, lowercase.
    pub language: String,
    /// The `foaf:name` of the user Person, if configured.
    pub user_name: Option<String>,
    /// The `[scan]` table.
    pub scan: ScanConfig,
    /// `[files] dir`, as written (a leading `~` is not expanded here).
    pub files_dir: Option<String>,
}

/// What `hali scan` reads from the config file (spec §7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanConfig {
    /// Excludes in gitignore syntax, relative to the Scan root.
    pub exclude: Vec<String>,
    /// Walk hidden names too.
    pub include_hidden: bool,
    /// The largest file registered, in bytes.
    pub max_size: Option<u64>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            language: "en".to_string(),
            user_name: None,
            scan: ScanConfig::default(),
            files_dir: None,
        }
    }
}

#[derive(Deserialize)]
struct Raw {
    user: Option<User>,
    vocabulary: Option<Vocabulary>,
    scan: Option<Scan>,
    files: Option<Files>,
}

#[derive(Deserialize)]
struct Files {
    dir: Option<String>,
}

#[derive(Deserialize)]
struct Scan {
    exclude: Option<Vec<String>>,
    include_hidden: Option<bool>,
    max_size: Option<String>,
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
    let scan = match raw.scan {
        None => ScanConfig::default(),
        Some(scan) => ScanConfig {
            exclude: scan.exclude.unwrap_or_default(),
            include_hidden: scan.include_hidden.unwrap_or(false),
            max_size: scan
                .max_size
                .map(|size| {
                    super::parse_size(&size).ok_or_else(|| {
                        Error::config_invalid(
                            path,
                            "[scan] max_size is not a size such as 512MiB or 1GiB",
                        )
                    })
                })
                .transpose()?,
        },
    };
    Ok(Config {
        language,
        user_name,
        scan,
        files_dir: raw.files.and_then(|f| f.dir),
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

/// The acting AI agent's name (spec §9.6): `flag` (`--agent`) wins over `env` (`HALI_AGENT`, as
/// read with `var_os`); an empty or whitespace-only `env` counts as unset. The name is trimmed.
/// An empty flag, a control character in either trimmed name, or a non-UTF-8 `env` is a usage
/// error; `env` is checked even when the flag wins.
pub fn agent_name(flag: Option<&str>, env: Option<&OsStr>) -> Result<Option<String>, Error> {
    let env = match env {
        None => None,
        Some(value) => {
            let value = value
                .to_str()
                .ok_or_else(|| Error::usage("HALI_AGENT is not valid UTF-8"))?
                .trim();
            if value.chars().any(char::is_control) {
                return Err(Error::usage(
                    "HALI_AGENT must not contain control characters",
                ));
            }
            Some(value).filter(|name| !name.is_empty())
        }
    };
    match flag {
        Some(flag) => check_agent_flag(flag)
            .map(Some)
            .map_err(|message| Error::usage(&message)),
        None => Ok(env.map(str::to_string)),
    }
}

/// Checks an `--agent` value and returns it trimmed: it must not be empty or whitespace-only
/// and must not contain control characters. The argument parser uses it too.
pub fn check_agent_flag(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("an agent name must not be empty".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("an agent name must not contain control characters".to_string());
    }
    Ok(name.to_string())
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
