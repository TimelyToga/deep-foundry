# TimTech build commands.
#   make          build the game (optimized, no debug info)
#   make run      build and run the game
#   make test     run all tests (dev profile)
#   make screenshots  make the README pictures again (needs sips, macOS)
#   make clean-old  delete old build files and cached crate sources (DAYS=3 by default)
#   make clean    delete all build files

DAYS ?= 3

.PHONY: build run headless test screenshots clean-old clean

build:
	cargo build --profile fast -p timtech

run: build
	./target/fast/timtech $(ARGS)

headless:
	cargo build --profile fast -p foundry_headless

test:
	cargo test --workspace

SHOT = ./target/fast/timtech --screenshot
SHOTS = out/shots
# Pictures made at 2560x1440 are made smaller to 1280x720.
SMALL = sips -s format jpeg -s formatOptions 85 -z 720 1280
JPEG = sips -s format jpeg -s formatOptions 85

screenshots: build
	mkdir -p $(SHOTS)
	$(SHOT) $(SHOTS)/world_wide.png --world gen --mode normal --ui-state playing --no-ui --zoom 1 --size 2560x1440
	$(SHOT) $(SHOTS)/world_gen.png --world gen --mode normal --ui-state playing --size 2560x1440 --ui-scale 1.35
	$(SHOT) $(SHOTS)/kiln.png --ui-state kiln --size 2560x1440 --ui-scale 1.35
	$(SHOT) $(SHOTS)/steam_line.png --ui-state steam-line --size 2560x1440 --ui-scale 1.35
	$(SHOT) $(SHOTS)/smelter.png --ui-state smelter --size 1600x900 --zoom 7 --center 62,1001
	$(SHOT) $(SHOTS)/automation.png --ui-state automation --size 1600x900 --center 150,1008
	$(SHOT) $(SHOTS)/cave.png --world gen --ui-state cave --no-ui --size 1280x560 --center 300,1355
	for f in world_wide world_gen kiln steam_line; do $(SMALL) $(SHOTS)/$$f.png --out docs/screenshots/$$f.jpg >/dev/null; done
	for f in smelter automation cave; do $(JPEG) $(SHOTS)/$$f.png --out docs/screenshots/$$f.jpg >/dev/null; done

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
