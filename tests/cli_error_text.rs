//! Goal tests for text-mode errors through the real binary (#73, spec §9.3 "Escaped for the
//! terminal", §9.4 text bullet): whatever outside text an error repeats (an argv reference, a
//! stored id, an env or config path), stderr holds no escape-set character, exactly two lines
//! (`error:` and `hint:`) and the `\u{..}` form of each character. Clap usage errors are out of
//! scope and not tested. The unit side is tests/error_text.rs; `refresh`'s own three escape tests
//! in tests/cli_refresh.rs are not goal-owned and must keep passing.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs, clears the rest of the
//! environment (so the real store and config are never read) and sets HALI_TEST_TTY=0 (no
//! terminal, so a destructive command is not confirmed). The store is tests/fixtures/relations
//! plus Turtle written inline below: a Concept, a Type and a relation kind whose ids hold U+202E
//! (valid in an IRI, not in a prefixed name, so they are written as full IRIs). Nothing here is
//! mode based, so no case is skipped for root.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the hostile reference is the unit tests' H (ESC ] 0 ; x BEL, a newline then `error: y`, bidi
//!   and invisible characters); the `refresh` argument is a real uncataloged file named with ESC
//!   and a newline, given by its absolute path, which text mode shows under `/TMP`;
//! - an error that repeats a path shows it exactly as the message built it, escaped:
//!   `cannot read or write /TMP/d\u{1b}[31mX`;
//! - the `--json` envelope of H is checked field by field, not against a golden snapshot.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000701";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000702";
    const H: &str = "a\u{1b}]0;x\u{7}b\nerror: y\u{202e}\u{2028}\u{61c}\u{feff}\u{200b}c";
    /// H as text mode shows it.
    const EH: &str =
        "a\\u{1b}]0;x\\u{7}b\\u{a}error: y\\u{202e}\\u{2028}\\u{61c}\\u{feff}\\u{200b}c";
    /// U+202E as text mode shows it.
    const RLO: &str = "\\u{202e}";

    struct Env {
        _root: TempDir,
        root: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    /// Whether text mode may not print `c` raw (the contract's set, without the structural LF).
    fn is_escaped(c: char) -> bool {
        let u = u32::from(c);
        c.is_control()
            || matches!(u,
                0x2028 | 0x2029
                | 0x061C | 0x200E | 0x200F
                | 0x202A..=0x202E | 0x2066..=0x2069
                | 0x00AD | 0x180E | 0x200B
                | 0x2060..=0x2064 | 0x206A..=0x206F
                | 0xFEFF | 0xFFF9..=0xFFFB
                | 0xE0000..=0xE007F)
    }

    impl Env {
        /// The `relations` fixture in the data dir.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), env.data().join(entry.file_name())).unwrap();
            }
            env
        }

        /// Hand-edited Turtle added to vocabulary.ttl: a Concept `bi<U+202E>di` (label bidi) that
        /// `uses` is under, an unused Concept `lo<U+202E>ne` (label lone), a Type `Bi<U+202E>di`
        /// (label Bidi) with a subtype `Sub`, and a relation kind `bi<U+202E>di` (label bidi)
        /// with a sub-relation `sub`.
        fn with_bidi_ids() -> Env {
            let env = Env::new();
            env.append(
                "vocabulary.ttl",
                "\n\
                 <https://w3id.org/hali#bi\u{202e}di> a skos:Concept ;\n\
                 \tskos:inScheme hali:topics ;\n\
                 \tskos:prefLabel \"bidi\"@en .\n\
                 hali:uses a skos:Concept ;\n\
                 \tskos:broader <https://w3id.org/hali#bi\u{202e}di> ;\n\
                 \tskos:inScheme hali:topics ;\n\
                 \tskos:prefLabel \"uses\"@en .\n\
                 <https://w3id.org/hali#lo\u{202e}ne> a skos:Concept ;\n\
                 \tskos:inScheme hali:topics ;\n\
                 \tskos:prefLabel \"lone\"@en .\n\
                 <https://w3id.org/hali#Bi\u{202e}di> a owl:Class ;\n\
                 \trdfs:label \"Bidi\"@en ;\n\
                 \trdfs:subClassOf hali:Resource .\n\
                 hali:Sub a owl:Class ;\n\
                 \trdfs:label \"Sub\"@en ;\n\
                 \trdfs:subClassOf <https://w3id.org/hali#Bi\u{202e}di> .\n\
                 <https://w3id.org/hali#bi\u{202e}di> a owl:ObjectProperty ;\n\
                 \trdfs:label \"bidi\"@en ;\n\
                 \trdfs:subPropertyOf dcterms:relation .\n\
                 hali:sub a owl:ObjectProperty ;\n\
                 \trdfs:label \"sub\"@en ;\n\
                 \trdfs:subPropertyOf <https://w3id.org/hali#bi\u{202e}di> .\n",
            );
            env
        }

        fn append(&self, file: &str, text: &str) {
            let path = self.data().join(file);
            let mut old = std::fs::read_to_string(&path).unwrap();
            old.push_str(text);
            std::fs::write(path, old).unwrap();
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        fn exec<I, S>(&self, extra_env: &[(&str, &OsStr)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", "1")
                .env("HALI_TEST_TTY", "0")
                .timeout(Duration::from_secs(30));
            for (k, v) in extra_env {
                cmd.env(k, v);
            }
            let out = cmd
                .write_stdin(b"y\n".to_vec())
                .args(args)
                .output()
                .unwrap();
            let root = self.root.to_str().unwrap();
            let clean = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).replace(root, "/TMP");
            Out {
                code: out.status.code(),
                stdout: clean(out.stdout),
                stderr: clean(out.stderr),
            }
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(&[], args)
        }

        /// The bytes of every file in the data dir, to prove a refused command wrote nothing.
        fn state(&self) -> Vec<(String, Vec<u8>)> {
            let mut files: Vec<_> = std::fs::read_dir(self.data())
                .map(|rd| {
                    rd.map(|e| {
                        let e = e.unwrap();
                        (
                            e.file_name().into_string().unwrap(),
                            std::fs::read(e.path()).unwrap(),
                        )
                    })
                    .collect()
                })
                .unwrap_or_default();
            files.sort();
            files
        }
    }

    /// A failed text-mode run: `code`, nothing on stdout, no escape-set character on stderr, two
    /// lines (`error:`, `hint:`), and `expected` exactly.
    fn assert_escaped(label: &str, out: &Out, code: i32, expected: &str) {
        assert_eq!(out.code, Some(code), "{label}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{label}");
        for c in out.stderr.chars().filter(|&c| c != '\n') {
            assert!(
                !is_escaped(c),
                "{label}: raw {c:?} on stderr: {:?}",
                out.stderr
            );
        }
        assert_eq!(out.stderr.lines().count(), 2, "{label}: {:?}", out.stderr);
        assert!(
            out.stderr.starts_with("error: "),
            "{label}: {:?}",
            out.stderr
        );
        assert_eq!(
            out.stderr
                .lines()
                .filter(|l| l.starts_with("hint: "))
                .count(),
            1,
            "{label}: {:?}",
            out.stderr
        );
        assert_eq!(out.stderr, expected, "{label}");
    }

    // ---- a typed reference that matches nothing -------------------------------------------------

    #[test]
    fn unknown_concept_text_escapes() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["find", "--tag", H]);
        assert_escaped(
            "find --tag H",
            &out,
            1,
            &format!(
                "error: no Concept matches {EH}\nhint: Use the slug or a label of an existing Concept, or add the Concept first. Try: hali --dry-run concept add '{EH}'\n"
            ),
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn unknown_type_text_escapes() {
        let env = Env::new();
        let out = env.run(["find", "--type", H]);
        assert_escaped(
            "find --type H",
            &out,
            1,
            &format!(
                "error: no Type matches {EH}\nhint: Use the name of an existing Type, or add the Type first. Try: hali type list\n"
            ),
        );
    }

    #[test]
    fn unknown_resource_text_escapes() {
        let env = Env::new();
        let out = env.run(["show", H]);
        assert_escaped(
            "show H",
            &out,
            1,
            &format!(
                "error: no cataloged Resource matches {EH}\nhint: Give the path of a cataloged file or the full UUID of a Resource; UUID prefixes are not accepted.\n"
            ),
        );
    }

    #[test]
    fn unknown_relation_text_escapes() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["relate", R1, H, R2]);
        assert_escaped(
            "relate R1 H R2",
            &out,
            1,
            &format!(
                "error: no relation kind matches {EH}\nhint: Use an existing relation kind (hali relation list), or add one first. Try: hali relation list\n"
            ),
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn refresh_unknown_ref_two_lines() {
        // An uncataloged file named like the forged-line attack on a `find ... -exec` run.
        let env = Env::new();
        let name = "zz\u{1b}[31mX\nerror: y";
        let path = env.root.join("files").join(name);
        std::fs::write(&path, b"hello\n").unwrap();
        let before = env.state();
        let out = env.run([OsStr::new("refresh"), path.as_os_str()]);
        assert_escaped(
            "refresh",
            &out,
            1,
            "error: no cataloged Resource matches /TMP/files/zz\\u{1b}[31mX\\u{a}error: y\nhint: Give the path of a cataloged file or the full UUID of a Resource; UUID prefixes are not accepted.\n",
        );
        assert_eq!(env.state(), before, "nothing written");
    }

    // ---- ids taken from a hand-edited store -----------------------------------------------------

    #[test]
    fn concept_stored_id_escapes() {
        let env = Env::with_bidi_ids();
        let before = env.state();
        let id = format!("bi{RLO}di");
        let lone = format!("lo{RLO}ne");
        let fix_confirm =
            "Run it again with --yes to confirm, or with --dry-run to see what it would change.";
        let cases: Vec<(&str, Vec<&str>, String)> = vec![
            (
                "concept remove (IN_USE)",
                vec!["concept", "remove", "bidi"],
                format!(
                    "error: Concept {id} is in use: 0 tagged, broader of 1 Concepts; nothing was written\nhint: Untag the Resources (hali edit <ref> --remove-tag '{id}') and unlink the narrower Concepts (hali concept edit <C> --remove-broader '{id}'), then retry. Try: hali concept show '{id}'\n"
                ),
            ),
            (
                "concept remove (CONFIRMATION_REQUIRED)",
                vec!["concept", "remove", "lone"],
                format!(
                    "error: concept remove needs confirmation, but stdin or stderr is not a terminal; nothing was written\nhint: {fix_confirm} Try: hali concept remove '{lone}' --yes\n"
                ),
            ),
            (
                "concept add (LABEL_TAKEN)",
                vec!["concept", "add", "bidi"],
                format!(
                    "error: label \"bidi\"@en is already used by Concept {id}\nhint: Use the existing Concept, or choose a different label. Try: hali concept list\n"
                ),
            ),
            (
                "concept rename (LABEL_TAKEN)",
                vec!["concept", "rename", "ml", "bidi"],
                format!(
                    "error: label \"bidi\"@en is already used by Concept {id}\nhint: Use the existing Concept, or choose a different label. Try: hali concept list\n"
                ),
            ),
            (
                "concept edit --add-alt (LABEL_TAKEN)",
                vec!["concept", "edit", "ml", "--add-alt", "bidi"],
                format!(
                    "error: label \"bidi\"@en is already used by Concept {id}\nhint: Use the existing Concept, or choose a different label. Try: hali concept list\n"
                ),
            ),
        ];
        for (label, args, expected) in cases {
            let out = env.run(args);
            assert_escaped(label, &out, 1, &expected);
        }
        assert_eq!(env.state(), before, "nothing written");
    }

    #[test]
    fn type_stored_id_escapes() {
        let env = Env::with_bidi_ids();
        let before = env.state();
        let id = format!("Bi{RLO}di");
        let out = env.run(["type", "remove", "Bidi"]);
        assert_escaped(
            "type remove (IN_USE)",
            &out,
            1,
            &format!(
                "error: Type {id} is in use: 0 typed, parent of 1 Types; nothing was written\nhint: Remove the Type from the Resources (hali edit <ref> --remove-type '{id}') and remove its subtypes first (hali type remove <S>), then retry. Try: hali type list\n"
            ),
        );
        let out = env.run(["type", "add", "Bidi"]);
        assert_escaped(
            "type add (LABEL_TAKEN)",
            &out,
            1,
            &format!(
                "error: label \"Bidi\"@en is already used by Type {id}\nhint: Use the existing Type, or choose a different name. Try: hali type list\n"
            ),
        );
        assert_eq!(env.state(), before, "nothing written");
    }

    #[test]
    fn relation_stored_id_escapes() {
        let env = Env::with_bidi_ids();
        let before = env.state();
        let id = format!("bi{RLO}di");
        let out = env.run(["relation", "remove", "bi\u{202e}di"]);
        assert_escaped(
            "relation remove (IN_USE)",
            &out,
            1,
            &format!(
                "error: relation kind {id} is in use: 0 relations, parent of 1 relation kinds; nothing was written\nhint: Remove the relations (hali unrelate <ref> '{id}' <ref>) and remove its sub-relations first (hali relation remove <S>), then retry. Try: hali relation list\n"
            ),
        );
        let out = env.run(["relation", "add", "bidi", "--parent", "dcterms:relation"]);
        assert_escaped(
            "relation add (LABEL_TAKEN)",
            &out,
            1,
            &format!(
                "error: label \"bidi\"@en is already used by relation kind {id}\nhint: Use the existing relation kind, or choose a different name. Try: hali relation list\n"
            ),
        );
        assert_eq!(env.state(), before, "nothing written");
    }

    // ---- paths from the environment and the filesystem ------------------------------------------

    #[test]
    fn outside_paths_escape() {
        // STORE_IO: HALI_DATA_DIR names a file, whose name holds ESC.
        let env = Env::new();
        let file = env.root.join("d\u{1b}[31mX");
        std::fs::write(&file, b"not a directory").unwrap();
        let out = env.exec(&[("HALI_DATA_DIR", file.as_os_str())], ["concept", "list"]);
        assert_escaped(
            "STORE_IO",
            &out,
            3,
            "error: cannot read or write /TMP/d\\u{1b}[31mX\nhint: Check that the data dir is a directory you can read and write.\n",
        );

        // CONFIG_INVALID: XDG_CONFIG_HOME holds ESC and a config file that is not TOML.
        let env = Env::new();
        let cfg = env.root.join("c\u{1b}[31mX");
        std::fs::create_dir_all(cfg.join("hali")).unwrap();
        std::fs::write(cfg.join("hali/config.toml"), "x = [").unwrap();
        let out = env.exec(&[("XDG_CONFIG_HOME", cfg.as_os_str())], ["concept", "list"]);
        assert_escaped(
            "CONFIG_INVALID",
            &out,
            3,
            "error: invalid config file /TMP/c\\u{1b}[31mX/hali/config.toml: not valid TOML, or a key has the wrong type\nhint: Fix the config file; only [user] name and [vocabulary] language are read.\n",
        );

        // NOT_A_REGULAR_FILE: `add` of a path that does not exist.
        let env = Env::new();
        let missing = env.root.join("files").join("m\u{1b}[31mX");
        let out = env.run([OsStr::new("add"), missing.as_os_str()]);
        assert_escaped(
            "NOT_A_REGULAR_FILE",
            &out,
            1,
            "error: /TMP/files/m\\u{1b}[31mX does not exist; only regular files can be cataloged\nhint: Give the path of an existing regular file.\n",
        );

        // NON_UTF8_PATH: `add` of a file whose name holds ESC and a byte that is not UTF-8. The
        // message shows the lossy form: U+FFFD is printed as is.
        let env = Env::new();
        let mut bytes = env.root.join("files").into_os_string().into_vec();
        bytes.extend_from_slice(b"/n\x1b[31m\xff");
        let odd = OsString::from_vec(bytes);
        std::fs::write(&odd, b"hello\n").unwrap();
        let before = env.state();
        let out = env.run([OsStr::new("add"), odd.as_os_str()]);
        assert_escaped(
            "NON_UTF8_PATH",
            &out,
            1,
            "error: /TMP/files/n\\u{1b}[31m\u{fffd} is not valid UTF-8; hali catalogs UTF-8 paths only\nhint: Rename the file, or the directory on its path, to a valid UTF-8 name.\n",
        );
        assert_eq!(env.state(), before, "nothing written");
    }

    // ---- --json is never escaped this way -------------------------------------------------------

    fn json_error(out: &Out) -> Value {
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(
            out.stderr.matches('\n').count(),
            1,
            "one line of JSON: {:?}",
            out.stderr
        );
        assert!(!out.stderr.contains('\u{1b}'), "serde escapes ESC");
        serde_json::from_str(&out.stderr).unwrap()
    }

    #[test]
    fn json_envelope_keeps_raw_values() {
        let env = Env::with_bidi_ids();

        let v = json_error(&env.run(["--json", "find", "--tag", H]));
        let e = &v["error"];
        assert_eq!(e["rule"], "UNKNOWN_CONCEPT");
        assert_eq!(e["message"], format!("no Concept matches {H}"));
        assert_eq!(e["details"]["concept"], H);
        assert_eq!(
            e["suggested_command"],
            format!("hali --dry-run concept add '{H}'")
        );
        assert!(!e["suggested_fix"].as_str().unwrap().contains("\\u{"));

        let v = json_error(&env.run(["--json", "show", H]));
        assert_eq!(
            v["error"]["message"],
            format!("no cataloged Resource matches {H}")
        );
        assert_eq!(v["error"]["details"]["ref"], H);

        let v = json_error(&env.run(["--json", "concept", "remove", "bidi"]));
        let e = &v["error"];
        assert_eq!(e["rule"], "IN_USE");
        assert_eq!(e["details"]["concept"], "bi\u{202e}di");
        assert!(e["message"].as_str().unwrap().contains("bi\u{202e}di"));
        assert!(!e["message"].as_str().unwrap().contains("\\u{"));
        assert_eq!(e["suggested_command"], "hali concept show 'bi\u{202e}di'");
    }
}
