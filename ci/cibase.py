#!/usr/bin/env python
import datetime
import os
import itertools
import multiprocessing
import typing as t
import sys

features = {"mutex", "rwlock", "std"}


Steps = t.Callable[[], t.Iterable]


current_step: t.Optional[str] = None


class ScriptError(Exception):
    pass


def on_script_error(exctype, value, traceback):
    if exctype == ScriptError:
        print(
            "\033[91m"
            + (
                f'Script Error: Step "{current_step}" has failed. '
                if current_step is not None
                else "Script Error. "
            )
            + "Check for detailed error messages above."
            + "\033[0m",
            file=sys.stderr,
            flush=True,
        )
    else:
        sys.__excepthook__(exctype, value, traceback)


sys.excepthook = on_script_error


def parallel_entry(target: t.Callable[..., t.Any], *args, **kwargs):
    target(*args, **kwargs)


def permute_features_parallel(
    target: t.Callable[[t.Iterable[str]], None],
    with_empty=False,
    with_full=False,
):
    global features
    calculated_range = range(
        0 if with_empty else 1, len(features) + (1 if with_full else 0)
    )
    parallel_params(
        target,
        itertools.chain(
            *[itertools.combinations(features, i) for i in calculated_range]
        ),
    )


T = t.TypeVar("T")


def parallel_params(target: t.Callable[[T], None], params: t.Iterable[T]):
    pool = multiprocessing.Pool()
    results = [
        pool.apply_async(parallel_entry, args=(target, param))
        for param in params
    ]
    [result.wait() for result in results]

    for result in results:
        if not result.successful():
            raise ScriptError()


def step(name: str):
    global current_step
    echo() if current_step is not None else ()
    current_step = name
    echo(f"[{datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S')}]: {name}")


def run(command: str):
    if os.system(command) != 0:
        raise ScriptError()


def echo(*args, **kwargs):
    print(*args, **kwargs, flush=True)
