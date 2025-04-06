#!/usr/bin/env python
import typing as t;

import common;
from common import run, done;

def test_features(features: t.Iterable[str]):
    run(f'cargo test --features "{','.join(features)}"')

if __name__ == "__main__":
    print("Clean", flush=True)
    run("cargo clean")

    print()
    print("Test - No features", flush=True)
    run("cargo test")

    print()
    print("Test - All features", flush=True)
    run("cargo test --all-features")

    print()
    print("Test - Feature permutations (parallel)", flush=True)
    common.permute_features_parallel(test_features)

    done()
