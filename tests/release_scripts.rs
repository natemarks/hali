//! Goal of #33 (criterion 8, safety): the release scripts never delete or force tags, never
//! use sudo, and never pipe a download into a shell. This reads the files; it needs no outside
//! software, so it is not ignored.

#[cfg(feature = "goal")]
mod release_support;

#[cfg(feature = "goal")]
mod scripts {
    use crate::release_support::read_script;

    const FILES: [&str; 5] = [
        "release-lib.sh",
        "release-prepare.sh",
        "release-publish.sh",
        "dist.sh",
        "install.sh",
    ];

    /// What is wrong with one line of shell, if anything. Comment lines are ignored.
    fn forbidden(line: &str) -> Option<&'static str> {
        let t = line.trim();
        if t.starts_with('#') {
            return None;
        }
        let words: Vec<&str> = t
            .split(|c: char| c.is_whitespace() || ";|&()".contains(c))
            .filter(|w| !w.is_empty())
            .collect();
        let has = |w: &str| words.contains(&w);
        if has("sudo") {
            return Some("sudo");
        }
        if t.contains("tag -d") || t.contains("tag --delete") || t.contains("tag -D") {
            return Some("tag delete");
        }
        if t.contains("--delete") || t.contains(":refs/tags") || t.contains("update-ref -d") {
            return Some("ref delete");
        }
        if words.iter().any(|w| w.starts_with("--force")) {
            return Some("--force");
        }
        if has("push")
            && words
                .iter()
                .any(|w| *w == "-f" || (w.len() > 1 && w.starts_with('+')))
        {
            return Some("force push");
        }
        if (has("curl") || has("wget")) && (t.contains("| sh") || t.contains("| bash")) {
            return Some("download piped into a shell");
        }
        None
    }

    #[test]
    fn never_delete_or_force_tags() {
        // The detector first: each of these must be flagged, and ordinary lines must not be.
        for bad in [
            "git tag -d v1.0.0",
            "git tag --delete v1.0.0",
            "git push origin :refs/tags/v1.0.0",
            "git push origin --delete v1.0.0",
            "git push --force origin v1.0.0",
            "git push --force-with-lease origin main",
            "git push -f origin v1.0.0",
            "git push origin +refs/tags/v1.0.0",
            "sudo make install",
            "curl -sSf https://example.invalid/i.sh | sh",
        ] {
            assert!(forbidden(bad).is_some(), "detector misses: {bad}");
        }
        for fine in [
            "git push origin refs/tags/v1.0.0",
            "mv -f -T -- \"$tmp\" \"$dest\"",
            "rm -f -- \"$tmp\"",
            "[ -f \"$file\" ] || exit 1",
            "# never use sudo, git tag -d or git push --force",
            "git tag -a \"v$version\" -m \"hali v$version\"",
        ] {
            assert_eq!(forbidden(fine), None, "detector flags: {fine}");
        }

        let mut found = Vec::new();
        for name in FILES {
            let text = read_script(name);
            for (n, line) in text.lines().enumerate() {
                if let Some(why) = forbidden(line) {
                    found.push(format!("scripts/{name}:{}: {why}: {}", n + 1, line.trim()));
                }
            }
        }
        assert!(found.is_empty(), "forbidden forms:\n{}", found.join("\n"));
    }
}
