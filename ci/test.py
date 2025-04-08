#!/usr/bin/env python
from cibase import step, run


if __name__ == "__main__":
    step("Clean")
    run("cargo clean")

    step("Test - All Features")
    run("cargo test --all-features --no-fail-fast")
