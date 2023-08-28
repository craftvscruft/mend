.phony: build env watch commit-fmt commit-lint

build:
	cargo build
env:
	export $(cat .env | xargs)

watch:
	@echo "Watching for changes..."
	cargo watch -s 'mold -run cargo run'

commit-fmt:
	cargo fmt && git commit -am "r - cargo fmt"

commit-lint:
	cargo clippy --fix && cargo test && git commit -am "r - cargo clippy --fix"

test:
	cargo test

install:
	cargo install --path .
