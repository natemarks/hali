//! The agent usage guide (spec §11.3): `docs/agent-usage.md`, embedded at build time for
//! `hali help agents`, and its marked snippet for `hali help agents --snippet`.

const GUIDE: &str = include_str!("../../docs/agent-usage.md");
const START: &str = "<!-- hali:snippet:start -->";
const END: &str = "<!-- hali:snippet:end -->";

/// The guide, byte for byte.
pub fn text() -> &'static str {
    GUIDE
}

/// The snippet: the lines strictly between the start and the end marker lines, joined by LF,
/// with no LF after the last one.
pub fn snippet() -> &'static str {
    marked(GUIDE).expect("docs/agent-usage.md holds the snippet markers")
}

/// The marked section of `text`; `None` when a marker line is missing or out of order.
fn marked(text: &str) -> Option<&str> {
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        match start {
            None if bare == START => start = Some(offset + line.len()),
            Some(from) if bare == END => {
                // The section ends before the LF that precedes the end marker.
                return Some(text[from..offset].strip_suffix('\n').unwrap_or(""));
            }
            _ => {}
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::marked;

    #[test]
    fn marked_section() {
        let text = "a\n<!-- hali:snippet:start -->\nb\nc\n<!-- hali:snippet:end -->\nd\n";
        assert_eq!(marked(text), Some("b\nc"));
        let empty = "<!-- hali:snippet:start -->\n<!-- hali:snippet:end -->\n";
        assert_eq!(marked(empty), Some(""));
        assert_eq!(marked("<!-- hali:snippet:end -->\n"), None);
        assert_eq!(
            marked("<!-- hali:snippet:end -->\n<!-- hali:snippet:start -->\n"),
            None
        );
    }
}
