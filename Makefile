build:
	@cargo build

test:
	@cargo nextest run --all-features --examples

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin main
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test release update-submodule
