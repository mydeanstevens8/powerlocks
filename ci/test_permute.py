#!/usr/bin/env python
import collections.abc as c
import cibase
from cibase import step, run


def build_test_features(features: c.Iterable[str]):
    run(f'cargo test --features "{",".join(features)}" --no-run')


def test_test_features(features: c.Iterable[str]):
    run(f'cargo test --features "{",".join(features)}" --no-fail-fast')


if __name__ == "__main__":
    step("Clean")
    run("cargo clean")

    step("Build - Feature permutations (parallel)")
    cibase.permute_features_parallel(
        build_test_features, with_empty=True, with_full=True
    )

    step("Test - Feature permutations (parallel)")
    cibase.permute_features_parallel(
        test_test_features, with_empty=True, with_full=True
    )
