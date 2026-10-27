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

shellcheck: ## check shell scripts
	git ls-files 'scripts/*.sh' | xargs -r shellcheck --severity=error --format=gcc

unit: ## run unit and CLI tests that need no outside software (with test-hooks)
	cargo test --features test-hooks

unit-update-golden: ## accept changed insta snapshots (review them first)
	INSTA_UPDATE=always cargo test --features test-hooks

test-external: ## run the #[ignore]d tests that need outside software (shared-mime-info)
	cargo test --features test-hooks -- --ignored

integration: ## run integration tests
	cargo test --features test-hooks --test '*'

static-check: fmt-check clippy dead-code unit audit shellcheck ## run all static checks, check-only (CI)

static: fmt clippy dead-code unit audit shellcheck ## run all static checks with auto-format

clean-cache: ## clean Rust build artifacts
	cargo clean

.PHONY: help fmt fmt-check clippy dead-code audit shellcheck unit unit-update-golden test-external integration static static-check clean-cache

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
