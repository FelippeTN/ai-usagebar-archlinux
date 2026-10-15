PREFIX ?= $(HOME)/.local
BINS := claude-usagebar claude-usagebar-tui

.PHONY: build test lint install uninstall

build:
	cargo build --release --locked

test:
	cargo test --all-targets

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

install: build
	@for b in $(BINS); do install -Dm755 target/release/$$b $(PREFIX)/bin/$$b; done
	@echo "instalado em $(PREFIX)/bin"

uninstall:
	@for b in $(BINS); do rm -f $(PREFIX)/bin/$$b; done
