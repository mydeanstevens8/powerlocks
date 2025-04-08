#!/usr/bin/env python
import typing as t
import cibase
from cibase import step, run


def build_features(features: t.Iterable[str]):
    (run(f'cargo build --features "{",".join(features)}"'),)
    run(f'cargo build --features "{",".join(features)}" --release')


if __name__ == "__main__":
    step("Clean")
    run("cargo clean")

    step("Build - No features")
    run("cargo build")
    run("cargo build --release")

    step("Build - All features")
    run("cargo build --all-features")
    run("cargo build --all-features --release")

    step("Build - Feature permutations (parallel)")
    cibase.permute_features_parallel(
        build_features, with_full=False, with_empty=False
    )
