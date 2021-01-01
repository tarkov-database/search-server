-include .env

rest-server:
	cargo build --bin search-rest --release

test:
	cargo test

run:
	cargo run

run-release:
	cargo run --release
