# ACS Makefile — Auto Consulting Service development workflow
#
# Self-evolve dev loop:
#   1. Run `make install` to build and install the acs binary
#   2. Run `acs init --auto` to bootstrap ticket backlog from the repo
#   3. Run `make evolve` to start the self-evolve loop (bounded manager + workers)
#      Workers pick up tickets, execute them in isolated git worktrees, and
#      commit changes to feature branches. The manager reviews and merges.
#   4. Run `make status` at any time to check ticket progress
#   5. Run `make test` / `make check` to validate changes
#   6. Run `make clean` to reset the .acs/ workspace (drops DB and worktrees)

.PHONY: all ci test coverage coverage-html check clippy fmt-check evolve status install clean help install-nextest install-coverage fmt clean-build

RUST_HOST := $(shell rustc -vV | sed -n 's/^host: //p')
RUSTUP_TOOLCHAIN_DIR := $(shell rustup which rustc 2>/dev/null | sed 's|/bin/rustc$$||')
LLVM_BIN_DIR := $(RUSTUP_TOOLCHAIN_DIR)/lib/rustlib/$(RUST_HOST)/bin
LLVM_COV_BIN := $(LLVM_BIN_DIR)/llvm-cov
LLVM_PROFDATA_BIN := $(LLVM_BIN_DIR)/llvm-profdata

# Default target
all: check test

# CI gate: run quality checks and strict coverage in parallel.
ci:
	@echo "Running check and coverage in parallel..."
	@$(MAKE) check & CHECK_PID=$$!; \
	$(MAKE) coverage & COV_PID=$$!; \
	wait $$CHECK_PID; CHECK_STATUS=$$?; \
	wait $$COV_PID; COV_STATUS=$$?; \
	if [ $$CHECK_STATUS -ne 0 ] || [ $$COV_STATUS -ne 0 ]; then \
		echo "CI failed (check=$$CHECK_STATUS, coverage=$$COV_STATUS)"; \
		exit 1; \
	fi

# ---------------------------------------------------------------------------
# Testing
# ---------------------------------------------------------------------------

# Run tests in parallel using cargo-nextest (faster than cargo test).
# Install nextest first: cargo install cargo-nextest
# Falls back to cargo test if nextest is not installed.
test:
	@if command -v cargo-nextest > /dev/null 2>&1 || cargo nextest --version > /dev/null 2>&1; then \
		echo "Running tests with cargo nextest (parallel)..."; \
		cargo nextest run; \
	else \
		echo "cargo-nextest not found, falling back to cargo test."; \
		echo "Install with: cargo install cargo-nextest"; \
		cargo test; \
	fi

# Run a strict coverage gate and emit a machine-readable report.
coverage:
	@if command -v cargo-llvm-cov > /dev/null 2>&1 || cargo llvm-cov --version > /dev/null 2>&1; then \
		mkdir -p coverage; \
		LLVM_COV="$(LLVM_COV_BIN)" LLVM_PROFDATA="$(LLVM_PROFDATA_BIN)" cargo llvm-cov --workspace --all-features --json --output-path coverage/coverage.json --fail-under-lines 100; \
	else \
		echo "cargo-llvm-cov not found."; \
		echo "Install with: cargo install cargo-llvm-cov --locked"; \
		exit 1; \
	fi

# Generate an HTML coverage report with the same strict threshold.
coverage-html:
	@if command -v cargo-llvm-cov > /dev/null 2>&1 || cargo llvm-cov --version > /dev/null 2>&1; then \
		mkdir -p coverage; \
		LLVM_COV="$(LLVM_COV_BIN)" LLVM_PROFDATA="$(LLVM_PROFDATA_BIN)" cargo llvm-cov --workspace --all-features --html --output-dir coverage/html --fail-under-lines 100; \
	else \
		echo "cargo-llvm-cov not found."; \
		echo "Install with: cargo install cargo-llvm-cov --locked"; \
		exit 1; \
	fi

# ---------------------------------------------------------------------------
# Code quality
# ---------------------------------------------------------------------------

# Run clippy and rustfmt checks in parallel.
check:
	@echo "Running clippy and fmt-check in parallel..."
	@$(MAKE) clippy & CLIPPY_PID=$$!; \
	$(MAKE) fmt-check & FMT_PID=$$!; \
	wait $$CLIPPY_PID; CLIPPY_STATUS=$$?; \
	wait $$FMT_PID; FMT_STATUS=$$?; \
	if [ $$CLIPPY_STATUS -ne 0 ] || [ $$FMT_STATUS -ne 0 ]; then \
		echo "Check failed (clippy=$$CLIPPY_STATUS, fmt=$$FMT_STATUS)"; \
		exit 1; \
	fi

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt-check:
	cargo fmt --all -- --check

# Auto-fix formatting in place (does not fail on unformatted code).
fmt:
	cargo fmt --all

# ---------------------------------------------------------------------------
# ACS self-evolve loop
# ---------------------------------------------------------------------------

# Start the ACS self-evolve loop with default worker count.
# Set WORKERS env var to control parallelism, e.g.: make evolve WORKERS=4
# The evolve command iterates manager/worker runs and ticket generation.
WORKERS ?= 2
evolve:
	acs evolve --workers $(WORKERS)

# Show current ticket progress and agent status.
status:
	acs status

# ---------------------------------------------------------------------------
# Installation
# ---------------------------------------------------------------------------

# Build and install the acs binary into ~/.cargo/bin/.
# Also installs cargo-nextest and cargo-llvm-cov for local quality gates.
install:
	cargo install --path .
	cargo install cargo-nextest --locked
	cargo install cargo-llvm-cov --locked

# Install cargo-nextest for parallel test execution.
install-nextest:
	cargo install cargo-nextest --locked

# Install cargo-llvm-cov for coverage reporting.
install-coverage:
	cargo install cargo-llvm-cov --locked

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

# Remove the .acs/ workspace directory (database, worktrees, logs).
# WARNING: This destroys all ticket state and in-progress work.
clean:
	@echo "Removing .acs workspace and pruning worktrees..."
	git worktree prune
	rm -rf .acs/
	@echo "Done. Run 'acs init' to start fresh."

# Remove Rust build artifacts.
clean-build:
	cargo clean

# ---------------------------------------------------------------------------
# Help
# ---------------------------------------------------------------------------

help:
	@echo "ACS development targets:"
	@echo ""
	@echo "  make test          Run tests in parallel with cargo-nextest"
	@echo "  make coverage      Run coverage and fail if line coverage is below 100%"
	@echo "  make coverage-html Generate HTML coverage report in coverage/html"
	@echo "  make check         Run clippy + rustfmt check"
	@echo "  make ci            Run check + 100% coverage"
	@echo "  make fmt           Auto-format source code"
	@echo "  make install       Install acs binary via cargo install"
	@echo "  make install-nextest  Install cargo-nextest for parallel tests"
	@echo "  make install-coverage Install cargo-llvm-cov for coverage reports"
	@echo "  make evolve        Start the ACS self-evolve loop (acs evolve)"
	@echo "  make status        Show ticket progress (acs status)"
	@echo "  make clean         Remove .acs/ workspace (destructive)"
	@echo "  make clean-build   Remove Rust build artifacts (cargo clean)"
	@echo ""
	@echo "Self-evolve dev loop:"
	@echo "  1. make install    # build and install acs"
	@echo "  2. acs init --auto # bootstrap ticket backlog"
	@echo "  3. make evolve     # start bounded manager + workers"
	@echo "  4. make status     # monitor progress"
