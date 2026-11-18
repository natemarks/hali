.DEFAULT_GOAL := help
SHELL := $(shell which bash)

help: ## Show this help
	@fgrep -h "##" $(MAKEFILE_LIST) | fgrep -v fgrep | sed -e 's/\\$$//' | sed -e 's/##//'

fmt: ## format code
	cargo fmt

fmt-check: ## check code formatting
	cargo fmt -- --check

clippy: ## run clippy linter
	cargo clippy --all-targets -- -D warnings

dead-code: ## check for dead code
	RUSTFLAGS="-D dead_code" cargo check --all-targets

audit: ## scan for vulnerable dependencies
	cargo audit

RELEASE_SCRIPTS := scripts/install.sh scripts/release-lib.sh scripts/dist.sh scripts/release-prepare.sh scripts/release-publish.sh

shellcheck: ## check shell scripts (the release scripts and install.sh at default severity)
	git ls-files 'scripts/*.sh' | xargs -r shellcheck --severity=error --format=gcc
	git ls-files -- $(RELEASE_SCRIPTS) | xargs -r shellcheck --external-sources --format=gcc

unit: ## run unit and CLI tests that need no outside software (with test-hooks)
	cargo test --features test-hooks

unit-update-golden: ## accept changed insta snapshots (review them first)
	INSTA_UPDATE=always cargo test --features test-hooks

test-external: ## run the #[ignore]d tests that need outside software (shared-mime-info; bash, git, tar, jq, shellcheck, cargo-edit)
	cargo test --features test-hooks -- --ignored

integration: ## run integration tests
	cargo test --features test-hooks --test '*'

bench: ## latency benchmark: hali find --json over 1k and 10k Resources (release build)
	HALI_BENCH=1 cargo test --release --locked --test bench_find -- --ignored --nocapture --test-threads=1

.PHONY: bench

static-check: fmt-check clippy dead-code unit audit shellcheck ## run all static checks, check-only (CI)

static: fmt clippy dead-code unit audit shellcheck ## run all static checks with auto-format

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
MANDIR ?= $(PREFIX)/share/man/man1
BASH_COMPDIR ?= $(PREFIX)/share/bash-completion/completions
ZSH_COMPDIR ?= $(PREFIX)/share/zsh/site-functions
FISH_COMPDIR ?= $(HOME)/.config/fish/completions
export BINDIR MANDIR BASH_COMPDIR ZSH_COMPDIR FISH_COMPDIR

install: ## developer-local install: release build to $(BINDIR), plus man page and bash/zsh/fish completions
	cargo build --release --locked
	@set -eu; umask 022; \
	bin="$${CARGO_TARGET_DIR:-target}/release/hali"; \
	if grep -aq 'HALI_TEST_' "$$bin"; then echo "error: $$bin contains HALI_TEST_; refusing to install a test-hooks build" >&2; exit 1; fi; \
	stage=$$(mktemp -d); cur=; \
	trap 'rm -rf -- "$$stage"; if [ -n "$$cur" ]; then rm -f -- "$$cur"; fi' EXIT; \
	trap 'exit 1' HUP INT TERM; \
	env -i "$$bin" generate man >"$$stage/hali.1"; \
	env -i "$$bin" generate completions bash >"$$stage/bash"; \
	env -i "$$bin" generate completions zsh >"$$stage/zsh"; \
	env -i "$$bin" generate completions fish >"$$stage/fish"; \
	bin_new=false; [ -d "$$BINDIR" ] || bin_new=true; \
	zsh_new=false; [ -d "$$ZSH_COMPDIR" ] || zsh_new=true; \
	put() { mode=$$1; src=$$2; dest=$$3; mkdir -p -- "$$(dirname -- "$$dest")"; \
		cur="$$dest.tmp.$$$$"; install -m "$$mode" -- "$$src" "$$cur"; mv -f -T -- "$$cur" "$$dest"; cur=; }; \
	put 0755 "$$bin" "$$BINDIR/hali"; \
	put 0644 "$$stage/hali.1" "$$MANDIR/hali.1"; \
	put 0644 "$$stage/bash" "$$BASH_COMPDIR/hali"; \
	put 0644 "$$stage/zsh" "$$ZSH_COMPDIR/_hali"; \
	put 0644 "$$stage/fish" "$$FISH_COMPDIR/hali.fish"; \
	if $$bin_new; then echo "warning: created $$BINDIR; log out and back in (or start a new login shell) so it is on PATH" >&2; fi; \
	if $$zsh_new; then echo "note: add $$ZSH_COMPDIR to fpath before compinit in ~/.zshrc for zsh completions" >&2; fi

# Release (spec §13.3-13.5): thin wrappers over scripts/. GH and RELEASE_STATIC are test seams.
GH ?= gh
RELEASE_STATIC ?= static-check
CARGO_EDIT_VERSION := 0.13.14
RELEASE_ENV = BUMP='$(BUMP)' DRY_RUN='$(DRY_RUN)' GH='$(GH)' RELEASE_STATIC='$(RELEASE_STATIC)' CARGO_EDIT_VERSION='$(CARGO_EDIT_VERSION)'

dist: ## release artifacts in dist/: hali-vX.Y.Z-x86_64-linux.tar.gz and SHA256SUMS (DRY_RUN=1 to preview)
	@$(RELEASE_ENV) scripts/dist.sh

release-prepare: ## release step 1: open the version-bump PR (BUMP=patch|minor|major, DRY_RUN=1 to preview)
	@$(RELEASE_ENV) scripts/release-prepare.sh

release-publish: ## release step 2, after the bump PR is merged: dist, tag, GitHub release (DRY_RUN=1 to preview)
	@$(RELEASE_ENV) scripts/release-publish.sh

.PHONY: dist release-prepare release-publish

clean-cache: ## clean Rust build artifacts
	cargo clean

.PHONY: install help fmt fmt-check clippy dead-code audit shellcheck unit unit-update-golden test-external integration static static-check clean-cache

# AI loop: see "AI loop" in AGENTS.md
GOAL_STATIC ?= static-check
GOAL_TAG_PATTERN := feature[[:space:]]*=[[:space:]]*"goal"|(^|[[:space:]])@goal([[:space:]]|$$)
GOAL_TAG_PATHS :=
GOAL_SETTLE_SED := /^[[:space:]]*\#\[cfg\(feature = "goal"\)\][[:space:]]*$$/d; /^[[:space:]]*@goal[[:space:]]*$$/d; s/[[:space:]]+@goal([[:space:]]|$$)/\1/g
GOAL_SETTLE_MV :=
LOOP_REQUIRED_CHECKS := static-check goal-settled
LOOP_READY_LABEL := ready-for-agent
GOAL_ENV = GOAL_STATIC=$(GOAL_STATIC) GOAL_TAG_PATTERN='$(GOAL_TAG_PATTERN)' GOAL_TAG_PATHS='$(GOAL_TAG_PATHS)' GOAL_SETTLE_SED='$(GOAL_SETTLE_SED)' GOAL_SETTLE_MV='$(GOAL_SETTLE_MV)'
LOOP_ENV = GOAL_STATIC=$(GOAL_STATIC) LOOP_REQUIRED_CHECKS='$(LOOP_REQUIRED_CHECKS)' LOOP_READY_LABEL='$(LOOP_READY_LABEL)'

goal: ## Run the PR goal: static gate, goal guard, goal-tagged tests, .goal/checks (red until the PR is done)
	@$(GOAL_ENV) scripts/goal.sh

goal-tests: ## Run goal-tagged tests only (never --all-features: it would pull goal tests into the static gate)
	cargo test --features test-hooks,goal

goal-settle: ## Retire the goal: untag goal tests, delete .goal/, commit "goal: settle"
	@$(GOAL_ENV) scripts/goal-settle.sh

goal-settled: ## Fail while goal scaffolding (.goal/, goal tags) remains; required check in CI
	@$(GOAL_ENV) scripts/goal-settled.sh

goal-selftest: ## Prove the loop's mechanics end to end in a throwaway worktree
	@$(GOAL_ENV) scripts/goal-selftest.sh

loop-doctor: ## Check everything the AI loop depends on, with a fix for each miss
	@$(LOOP_ENV) scripts/loop-doctor.sh

loop-protect: ## Require the loop's checks on the default branch (dry run; APPLY=1 to apply)
	@$(LOOP_ENV) scripts/loop-protect.sh $(if $(filter 1,$(APPLY)),--apply)

loop-status: ## Show what needs you: stopped loops, PRs to merge, issues to groom, worktrees to remove
	@$(LOOP_ENV) scripts/loop-status.sh

.PHONY: goal goal-tests goal-settle goal-settled goal-selftest loop-doctor loop-protect loop-status
