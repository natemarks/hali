//! The agent skill (spec §11.2, §11.3): `skill/SKILL.md`, embedded at build time with its
//! `hali-version` filled in, the files `hali skill install` writes, and what `describe` reports
//! about an installed copy. No I/O.

use super::guide;
use serde::Serialize;

const SKILL_MD: &str = include_str!("../../skill/SKILL.md");
const PLACEHOLDER: &str = "{{HALI_VERSION}}";

/// The most characters of an installed `hali-version`.
const MAX_VERSION: usize = 64;

/// The version of this binary.
fn binary() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `skill/SKILL.md` with the binary version in its frontmatter.
pub fn skill_md() -> String {
    SKILL_MD.replace(PLACEHOLDER, binary())
}

/// The files an install writes below a skill directory, in order: `SKILL.md`, then the usage
/// guide as `references/usage.md`, byte for byte.
pub fn files() -> Vec<(&'static str, String)> {
    vec![
        ("SKILL.md", skill_md()),
        ("references/usage.md", guide::text().to_string()),
    ]
}

/// The `hali-version` of the frontmatter of `text` (the lines between a first `---` line and
/// the next one), double-quoted or bare; `None` unless it matches `^[0-9A-Za-z.+-]{1,64}$`.
pub fn installed_version(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return None;
    }
    // The first `hali-version` line counts, valid or not.
    let mut found: Option<Option<String>> = None;
    for line in lines {
        if line == "---" {
            return found.flatten();
        }
        if found.is_none()
            && let Some(value) = line.trim_start().strip_prefix("hali-version:")
        {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            found = Some(Some(value).filter(|v| valid_version(v)).map(str::to_string));
        }
    }
    // An unterminated frontmatter has no version.
    None
}

fn valid_version(v: &str) -> bool {
    (1..=MAX_VERSION).contains(&v.len())
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// The `skill` key of `describe`: the installed version (`None` when there is no installed
/// file or it has no readable version), the binary version, and whether the installed file
/// differs from the binary's version.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct State {
    pub installed: Option<String>,
    pub binary: String,
    pub stale: bool,
}

/// The state of an installed `SKILL.md` whose text is `installed_text` (`None`: no file).
pub fn state(installed_text: Option<&str>) -> State {
    let binary = binary().to_string();
    match installed_text {
        None => State {
            installed: None,
            binary,
            stale: false,
        },
        Some(text) => {
            let installed = installed_version(text);
            let stale = installed.as_deref() != Some(binary.as_str());
            State {
                installed,
                binary,
                stale,
            }
        }
    }
}

/// What an install does to a file whose bytes are `current` (`None`: no file) to make it `new`.
pub fn action(current: Option<&[u8]>, new: &[u8]) -> &'static str {
    match current {
        None => "created",
        Some(bytes) if bytes == new => "unchanged",
        Some(_) => "updated",
    }
}
