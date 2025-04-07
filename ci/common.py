#!/usr/bin/env python
import os
import itertools
import threading
import typing as t

features = {"mutex", "rwlock", "std"}


def permute_features_parallel(target: t.Callable[[t.Iterable[str]], None]):
    features_permuted = []
    for i in range(1, len(features) + 1):
        features_permuted.extend(itertools.combinations(features, i))

    threads: t.List[threading.Thread] = []
    for selected_features in features_permuted:
        thread = threading.Thread(target=target, args=(selected_features,))
        thread.start()
        threads.append(thread)

    for thread in threads:
        thread.join()


script_error = threading.Event()


def run(command: str) -> bool:
    global script_error
    if os.system(command) != 0:
        script_error.set()
        return False
    else:
        return True


def done():
    global script_error
    exit(1 if script_error.is_set() else 0)
