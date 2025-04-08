#!/usr/bin/env python
from cibase import step, run


if __name__ == "__main__":
    step("Clean")
    run("cargo clean")

    step("Lint - Cargo Check")
    run("cargo check --all-features")

    step("Lint - Clippy")
    run("cargo clippy --all-features -- -D warnings")
