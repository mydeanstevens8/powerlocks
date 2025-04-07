#!/usr/bin/env python
import typing as t;

import common;
from common import run, done;

def build_features(features: t.Iterable[str]):
    run(f'cargo build --features "{",".join(features)}"')
    run(f'cargo build --features "{",".join(features)}" --release')

if __name__ == "__main__":
    print("Clean", flush=True)
    run("cargo clean")

    print()
    print("Build - No features", flush=True)
    run("cargo build")
    run("cargo build --release")

    print()
    print("Build - All features", flush=True)
    run("cargo build --all-features")
    run("cargo build --all-features --release")

    print()
    print("Build - Feature permutations (parallel)", flush=True)
    common.permute_features_parallel(build_features)

    done()
