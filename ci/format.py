#!/usr/bin/env python
from cibase import step, run


if __name__ == "__main__":
    step("Format")
    run("cargo fmt --all -- --check")
