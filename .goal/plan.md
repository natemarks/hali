# Plan for #4 `hali generate` and `make install` (approved)

The approved contract is the "Groomed contract (approved)" section of issue #4. All seven open questions took the planner's default (strip hidden subcommands from generated completions; zsh `~/.local/share/zsh/site-functions/_hali` with an fpath note, fish `~/.config/fish/completions/hali.fish`; warn only when BINDIR was just created; no lasting snapshot of the man page or completions; no uninstall target; no CI install job; absorb the `HALI_TEST_` assertion of #81 for `make install`).

Not cross-cutting: implementer on its default model. Touches `Cargo.toml`/`Cargo.lock` (guarded): security-reviewer required.

## Seams
- The real binary (`assert_cmd`, `Command::arg0`): output, exit codes, hidden listing, determinism, no store access. `hali::cli::command()`: `generate` exists with `is_hide_set()`.
- `make install` with `HOME` or `PREFIX` pointed at a temp dir (CARGO_HOME and RUSTUP_HOME exported).
- `src/cli/generate.rs` (new): `Generate { Man, Completions { shell: GenShell } }` and render functions; in `src/cli/mod.rs` one variant at the end of `Command` and one early-dispatch line beside `Command::Schema` in `run()`, before `injected()`.

## Goal map
New tests go in `tests/cli_generate.rs` under a goal-tagged `mod goal`.
| # | Criterion | Check |
|---|---|---|
| 1 | `generate man`: roff `.TH hali 1`, lists `describe`/`find`, not `generate`; `groff -man -ww -z` clean | `cli_generate::man_page`; `.goal/checks/02-groff.sh` |
| 2 | `completions bash\|zsh\|fish` register `hali` (`complete -F _hali ... hali`, `#compdef hali`, `complete -c hali`); `bash -n` parses; hidden `generate` not offered | `cli_generate::completions_each_shell` (`bash -n` in `#[ignore]`d `completions_parse`) |
| 3 | Unknown or hostile shell, missing shell: exit 2, escaped, lists values; bare `generate` gives help on stderr, exit 2 | `cli_generate::usage_errors` |
| 4 | Hidden from `--help`, `hali help` and the man page; present in the tree | `cli_generate::hidden` |
| 5 | Name is `hali` under hostile `arg0` or a symlink; no `$0`; src reads no program name | `cli_generate::argv0_independent`; `.goal/checks/01-no-argv0.sh` (grep `args_os\|current_exe\|arg0\|env::args\|/proc/self` over `src/`) |
| 6 | Deterministic, no paths; needs no store or config (a broken `HALI_DATA_DIR`/config still exits 0, creates nothing) | `cli_generate::deterministic_and_storeless` |
| 7 | `--json`/`--dry-run`/`--yes` give identical output | `cli_generate::global_flags_ignored` |
| 8 | Schema list unchanged; `schema generate man` exits 2; hidden leaves skipped by `every_command_has_a_schema` | edited `core_schema.rs`; `.goal/checks/03-snaps.sh` (three `*schema_list_json.snap` unchanged vs merge-base) |
| 9 | Dry-run guard covers both generate leaves | `dry_run_guard.rs` CASES `generate man`, `generate completions bash` |
| 10 | `make install` puts the 5 files at the §4 paths, modes 0755/0644, content = `generate` output; nothing else under HOME | `.goal/checks/04-install.sh` (`HOME=$tmp make install`) |
| 11 | Re-login warning only when BINDIR was created; a second run has none, overwrites, replaces a symlink without writing its target | `04-install.sh` |
| 12 | Release build, `--locked`, no test-hooks; aborts on `HALI_TEST_` | `.goal/checks/05-release.sh` (`make -n install` has `--release --locked`, no `--features`; installed binary has no `HALI_TEST_`) |
| 13 | Exact pins; Cargo.lock adds only `clap_complete`, `clap_mangen`, `roff`; `cargo audit` clean | `.goal/checks/06-deps.sh` (`cargo tree -e normal` diff vs merge-base) |
| 14 | Spec §4/§13.2 paths, AGENTS.md rows, `make help` lists install | `.goal/checks/07-docs.sh` |

No existing snapshot changes (help tests compare against `render_help()`; no help `.snap` files). Tests clear HOME, XDG_* and HALI_* and point them at temp dirs.
