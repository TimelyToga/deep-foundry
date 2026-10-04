# Deep Foundry build commands.
#   make          build the game (optimized, no debug info)
#   make run      build and run the game
#   make test     run all tests (dev profile)
#   make clean-old  delete old build files and cached crate sources (DAYS=3 by default)
#   make clean    delete all build files

DAYS ?= 3

.PHONY: build run headless test clean-old clean

build:
	cargo build --profile fast -p deep_foundry

run: build
	./target/fast/deep-foundry $(ARGS)

headless:
	cargo build --profile fast -p foundry_headless

test:
	cargo test --workspace

# Removes:
#  - target/release (the Makefile uses target/fast)
#  - incremental caches (cargo makes them again when needed)
#  - files in target/*/deps that were not changed in $(DAYS) days (old versions)
#  - unpacked crate sources in ~/.cargo (cargo unpacks them again when needed)
clean-old:
	@du -sh target 2>/dev/null || true
	rm -rf target/release
	rm -rf target/*/incremental
	find target/*/deps -type f -mtime +$(DAYS) -delete 2>/dev/null || true
	rm -rf $(HOME)/.cargo/registry/src
	@du -sh target 2>/dev/null || true

clean:
	cargo clean
