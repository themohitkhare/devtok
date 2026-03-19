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

.PHONY: all test check evolve status install clean help

# Default target
all: check test

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

# ---------------------------------------------------------------------------
# Code quality
# ---------------------------------------------------------------------------

# Run clippy (lints) and rustfmt (formatting check).
check:
	cargo clippy --all-targets --all-features -- -D warnings
	cargo fmt --all -- --check

# Auto-fix formatting in place (does not fail on unformatted code).
fmt:
	cargo fmt --all

# ---------------------------------------------------------------------------
# ACS self-evolve loop
# ---------------------------------------------------------------------------

# Start the ACS self-evolve loop with default worker count.
# Set WORKERS env var to control parallelism, e.g.: make evolve WORKERS=4
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
install:
	cargo install --path .

# Install cargo-nextest for parallel test execution.
install-nextest:
	cargo install cargo-nextest --locked

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

# Remove the .acs/ workspace directory (database, worktrees, logs).
# WARNING: This destroys all ticket state and in-progress work.
clean:
	@echo "Removing .acs/ workspace (tickets, worktrees, logs)..."
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
	@echo "  make check         Run clippy + rustfmt check"
	@echo "  make fmt           Auto-format source code"
	@echo "  make install       Install acs binary via cargo install"
	@echo "  make install-nextest  Install cargo-nextest for parallel tests"
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
