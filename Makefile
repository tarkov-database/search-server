-include .env

rest-server:
	cargo build --bin search-rest --release

test:
	cargo test

run-debug:
	cargo run

run-release:
	cargo run --release
