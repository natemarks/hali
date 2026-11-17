# Plan for #26 Latency benchmark `make bench` (approved)

The approved contract is the "Groomed contract (approved)" section of issue #26. All eight open questions took the planner's default (#48 stays separate; env guard `HALI_BENCH=1` keeps the benchmark out of `test-external`; warm cache after one untimed warm-up per size; nearest-rank percentiles; text output only; never fails on the verdict; empty `.lock` in the synthetic store; `bench-run.sh` runs a real `make bench` in the goal).

Not cross-cutting, not security-sensitive (tests, Makefile, AGENTS.md, spec): implementer on its default model; regular reviewer only. If the implementer must touch `Cargo.toml` (a guarded path), it stops and reports.

## Seams
- `make bench` / `make -n bench`; `cargo test --test bench_find` (`-- --list`, `--ignored`); `tests/bench_support/mod.rs` (`build_store`, `percentile`, `verdict`); the real binary (`hali validate --json`, `hali find --json`).

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | `make bench` exists: release build, no test-hooks, `HALI_BENCH=1`, `--ignored`; `make help` lists `bench` | `.goal/checks/01-bench-target.sh` (`make -n bench` contains `cargo test --release`, `HALI_BENCH=1`, `--ignored`, not `test-hooks`) |
| 2 | Ignored: not in unit, static or CI | `.goal/checks/02-bench-ignored.sh` (`cargo test --features test-hooks --test bench_find` reports `1 ignored`; `-- --list --ignored` shows `bench_find_latency`) |
| 3 | Not run by `test-external` | `.goal/checks/03-bench-skip.sh` (`-- --ignored --nocapture` without `HALI_BENCH` prints `skipped: run with make bench` in under 60 s) |
| 4 | Refuses a debug or test-hooks build | `.goal/checks/04-bench-refuse.sh` (`HALI_BENCH=1` with `--features test-hooks -- --ignored` exits non-zero with the message) |
| 5 | Store is valid, realistic, N Resources | `tests/bench_contract.rs::store_is_conformant_and_realistic` (N=24: `validate --json` conforms; `find --json --limit 100` has `total` 24, >= 3 media types, >= 2 Types, some tagged and some untagged, all `missing` and `stale` false; dir 0700) |
| 6 | Store is deterministic | `bench_contract::store_is_deterministic` (two roots give identical `resources.ttl` and `vocabulary.ttl` once each root path is replaced) |
| 7 | Nearest-rank percentiles | `bench_contract::percentile_nearest_rank` (1..=30 ms: p50 = 15, p95 = 29; unsorted input; n = 1) |
| 8 | Verdict line, never fails | `bench_contract::verdict_threshold` (200.0 ms says "stays"; 200.1 ms says "v0.1 task ... file a follow-up issue") |
| 9 | Reports p50/p95 per size; writes nothing in the repo | `.goal/checks/05-bench-run.sh` (runs `make bench`; header line, `1000` and `10000` rows with two numbers each, a `verdict:` line; `git status --porcelain` identical before and after; `tests/bench_find.rs` uses `env_clear`) |
| 10 | Docs | `.goal/checks/06-bench-docs.sh` (AGENTS.md `make bench` row and the sentence no longer names `make bench`; spec §14.5 names `HALI_BENCH`, the warm-up, nearest-rank, the verdict; §14.7 sentence) |

Locked: `tests/bench_contract.rs` (goal-tagged). The implementer owns `tests/bench_support/mod.rs` and `tests/bench_find.rs`: the contract tests include `bench_support` through `#[path]`/`mod` and fail to compile today (`E0583`), which is the intended red. Tests clear HOME, XDG_* and HALI_*.
