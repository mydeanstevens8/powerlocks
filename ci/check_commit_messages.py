import subprocess
import typing as t
import collections.abc as c
import itertools
import sys
import enum
import re


# Build in a color class to limit external dependencies required for the GitHub
# actions check.
class AnsiColor(enum.Enum):
    Black = 0
    Red = 1
    Green = 2
    Yellow = 3
    Blue = 4
    Magenta = 5
    Cyan = 6
    White = 7
    Custom = 8
    Default = 9


def ansiesc(
    text: str = "",
    fg: t.Optional[AnsiColor] = None,
    bg: t.Optional[AnsiColor] = None,
    reset: bool = False,
    bold: bool = False,
    dim: bool = False,
    italic: bool = False,
    underline: bool = False,
    blinking: bool = False,
    invert: bool = False,
    hidden: bool = False,
    end_reset: bool = True,
) -> str:
    return (
        "\x1b["
        + ";".join(
            (["0"] if reset else [])
            + (["1"] if bold else [])
            + (["2"] if dim else [])
            + (["3"] if italic else [])
            + (["4"] if underline else [])
            + (["5"] if blinking else [])
            + (["7"] if invert else [])
            + (["8"] if hidden else [])
            + ([f"3{fg.value}"] if fg is not None else [])
            + ([f"4{bg.value}"] if bg is not None else [])
        )
        + "m"
        + text
        + ("\x1b[0m" if end_reset else "")
    )


class Diagnostic(t.NamedTuple):
    description: str = ""
    code: t.Optional[str] = None
    hint: t.Optional[str] = None
    location: t.Optional[tuple[int, range]] = None
    help: t.Optional[str] = None
    note: t.Optional[str] = None


type DiagnosticCheck = c.Callable[[str], list[Diagnostic]]


def empty(_) -> list[Diagnostic]:
    return []


def format_help_suggestion(suggestion: str) -> str:
    return "\n\n" + ansiesc(
        "\n".join(["| ".rjust(8) + line for line in suggestion.splitlines()]),
        fg=AnsiColor.Green,
    )


def check_line_too_long(message: str) -> list[Diagnostic]:
    LINE_LIMIT = 72
    message_lines = message.splitlines()

    violations = filter(
        lambda row_violation: row_violation[1],
        [
            (row, len(line) > LINE_LIMIT, line)
            for (row, line) in enumerate(message_lines)
        ],
    )
    return [
        Diagnostic(
            code="LINE_TOO_LONG",
            description=f"Lines must not be over {LINE_LIMIT} characters long",
            location=(row, range(LINE_LIMIT, len(line))),
            hint=("subject" if row == 0 else "body") + " line too long",
            help=(
                "Split this long line into two or more shorter lines."
                if row > 0
                else (
                    "Shorten this subject line."
                    "\n\n"
                    "If neccessary, move extra details in this long subject "
                    "line to the body."
                )
            ),
        )
        for (row, _, line) in violations
    ]


def get_subject(message: str) -> t.Optional[str]:
    message_lines = message.splitlines()
    return message_lines[0] if len(message_lines) > 0 else None


def check_subject_end_in_punctuation(message: str) -> list[Diagnostic]:
    subject = get_subject(message)
    if subject is not None:
        if subject.endswith((".", ",", "!", "?", ":", ";")):
            invalid_ending = subject[-1]
            return [
                Diagnostic(
                    code="INVALID_ENDING_PUNCTUATION",
                    description="Subject must not end in "
                    f'a "{invalid_ending}" character',
                    location=(0, range(len(subject) - 1, len(subject))),
                    hint=f'ends in a "{invalid_ending}"',
                    help=(
                        "Remove this character:"
                        + format_help_suggestion(subject[:-1])
                        + "\n\n"
                        + "Alternatively, if this is part of a code snippet, "
                        'surround the snippet with quotes or "`".'
                    ),
                )
            ]
        else:
            return []


def check_non_empty_second_line(message: str) -> list[Diagnostic]:
    lines = message.splitlines()
    if len(lines) > 1 and len(lines[1]) > 0:
        subject = lines[0]
        offender = lines[1]
        return [
            Diagnostic(
                code="NON_EMPTY_SECOND_LINE",
                description="There must be an empty line between the subject \n"
                "and the body",
                location=(1, range(len(offender))),
                hint="non-empty second line"
                + (
                    " (these characters are whitespace)"
                    if offender.isspace()
                    else ""
                ),
                help=(
                    "Remove these extra whitespace characters."
                    if offender.isspace()
                    else "Add a space between the subject and this line:"
                    + format_help_suggestion(subject + "\n\n" + offender)
                ),
            )
        ]
    else:
        return []


def check_wrong_case_subject(message: str):
    BAN_UPPER_CASE = True
    BAN_TITLE_CASE = True

    subject = get_subject(message)
    # Remove anything that's in quotes or code markers (or anything likely to
    # constitute code).
    valid_word_subject = re.sub(
        "(?:`.*?`|\".*?\"|'.*?'|\\[.*?\\]|{.*?})", "", subject.strip()
    )

    # As an exception, capitalize the "I" word as required in English.
    suggested_subject = re.sub(
        "(^|\\s)i(,|\\.|;|:|\\s|$)", "\\1I\\2", subject.strip().capitalize()
    )

    if subject is not None:
        subject_words = [word.strip() for word in valid_word_subject.split()]

        if BAN_UPPER_CASE:
            upper_cased_words = sum(
                [
                    int(
                        word == word.upper()
                        and word[0].isalpha()
                        and len(word) > 1
                    )
                    for word in subject_words
                ]
            )

            if (
                (
                    upper_cased_words >= 3
                    and (upper_cased_words / len(subject_words) >= 0.67)
                )
                or (upper_cased_words == len(subject_words))
            ) and any([len(word) > 2 for word in subject_words]):
                return [
                    Diagnostic(
                        code="WRONG_CASED_SUBJECT",
                        description="Subject line must not be in upper-case",
                        location=(0, range(len(subject))),
                        hint="subject in upper case",
                        help="Convert the subject to sentence-case:"
                        + format_help_suggestion(suggested_subject),
                    )
                ]

        if BAN_TITLE_CASE:
            title_cased_words = sum(
                [
                    int(word[0] == word.capitalize()[0] and word[0].isalpha())
                    for word in subject_words
                ]
            )

            if (
                (
                    title_cased_words >= 5
                    and (title_cased_words / len(subject_words) >= 0.75)
                )
                or (
                    title_cased_words == len(subject_words)
                    and len(subject_words) >= 3
                )
            ) and any([len(word) > 2 for word in subject_words]):
                return [
                    Diagnostic(
                        code="WRONG_CASED_SUBJECT",
                        description="Subject line must not be in title-case",
                        location=(0, range(len(subject))),
                        hint="subject in title case",
                        help="Convert the subject to sentence-case:"
                        + format_help_suggestion(suggested_subject),
                        note="This check can be pedantic in some cases. "
                        'Words in quotes (e.g. "\'", "`") and words '
                        "\n"
                        'inside code brackets (e.g. "[]", "{}" etc.) are '
                        "ignored for this check for the purposes of"
                        "\n"
                        "representing code, quotes, or other terminology.",
                    )
                ]

        if not subject[0].isupper():
            return [
                Diagnostic(
                    code="WRONG_CASED_SUBJECT",
                    description="Subject lines must start with an upper-case "
                    "character",
                    location=(0, range(1)),
                    hint="non-uppercase starting character",
                    help="Start the subject with an uppercase character"
                    + format_help_suggestion(subject[0].upper() + subject[1:]),
                )
            ]

    return []


def check_untrimmed_line(message: str):
    lines = message.splitlines()
    return list(
        itertools.chain(
            *[
                (
                    [
                        Diagnostic(
                            code="UNTRIMMED_LINE",
                            description="Lines must not have leading "
                            "or trailing whitespace",
                            location=(
                                row,
                                range(len(line) - len(line.lstrip())),
                            ),
                            hint="leading whitespace",
                            help="Remove this leading whitespace",
                        )
                    ]
                    if len(line) - len(line.lstrip()) > 0 and not line.isspace()
                    else []
                )
                + (
                    [
                        Diagnostic(
                            code="UNTRIMMED_LINE",
                            description="Lines must not have leading "
                            "or trailing whitespace",
                            location=(
                                row,
                                range(len(line.rstrip()), len(line)),
                            ),
                            hint="trailing whitespace",
                            help="Remove this trailing whitespace",
                        )
                    ]
                    if len(line) - len(line.rstrip()) > 0 and not line.isspace()
                    else []
                )
                for row, line in filter(
                    # Ignore if it's the second row - there's the
                    # `check_non_empty_second_line` check which requires it to
                    # be a blank line.
                    lambda line: line[1].strip() != line[1] and line[0] != 1,
                    enumerate(lines),
                )
            ]
        )
    )


def check_empty_line(message: str):
    lines = message.splitlines()
    return list(
        itertools.chain(
            *[
                (
                    [
                        Diagnostic(
                            code="EMPTY_LINE",
                            description="Lines must not be entirely whitespace",
                            location=(
                                row,
                                range(0, len(line)),
                            ),
                            hint="entirely whitespace",
                            help="Remove this whitespace",
                        )
                    ]
                    if line.isspace()
                    else []
                )
                for row, line in filter(
                    # Ignore if it's the second row - there's the
                    # `check_non_empty_second_line` check which requires it to
                    # be a blank line.
                    lambda line: (line[1].isspace()) and line[0] != 1,
                    enumerate(lines),
                )
            ]
        )
    )


diagnostic_checks = [
    check_line_too_long,
    check_subject_end_in_punctuation,
    check_non_empty_second_line,
    check_wrong_case_subject,
    check_untrimmed_line,
    check_empty_line,
]


def check_commit_message(message: str) -> list[Diagnostic]:
    return list(itertools.chain(*[diag(message) for diag in diagnostic_checks]))


def emit_diagnostics(diags: list[Diagnostic], message: str):
    def emit_location(
        message: str, location: tuple[int, range], hint: t.Optional[str] = None
    ):
        lines = message.splitlines()
        # Lines before
        if location[0] - 1 > 0:
            print(
                " ".rjust(8),
                ansiesc("...", dim=True),
                sep="",
                file=sys.stderr,
            )
        if location[0] > 0:
            print(
                " ".rjust(8),
                ansiesc(lines[location[0] - 1], dim=True),
                sep="",
                file=sys.stderr,
            )
        locrange = location[1]

        # Actual line + highlight
        line = lines[location[0]]
        print(
            ansiesc(
                f"L{location[0] + 1}: ".rjust(8),
                fg=AnsiColor.Cyan,
                bold=True,
            ),
            line[: locrange.start],
            ansiesc(
                line[locrange.start : locrange.stop],
                fg=AnsiColor.Red,
                bold=True,
            ),
            line[locrange.stop :],
            sep="",
            file=sys.stderr,
        )

        # Diagnostic markers
        print(
            " ".rjust(8),
            "".join([" " for _ in range(0, locrange.start)]),
            ansiesc(
                "".join(["^" for _ in locrange]), fg=AnsiColor.Red, bold=True
            ),
            sep="",
            file=sys.stderr,
        )

        # Diagnostic message
        if hint is not None:
            print(
                ansiesc(
                    hint.rjust(8 + locrange.stop),
                    fg=AnsiColor.Yellow,
                    bold=True,
                ),
                sep="",
                file=sys.stderr,
            )
        print(file=sys.stderr, flush=True)

    def emit_help(help: str):
        print(
            ansiesc("Help:", fg=AnsiColor.Green, bold=True),
            help,
            file=sys.stderr,
        )
        print(file=sys.stderr, flush=True)

    def emit_note(note: str):
        print(
            ansiesc("Note:", fg=AnsiColor.Magenta, bold=True),
            note,
            file=sys.stderr,
        )

    grouped_diags = itertools.groupby(
        iterable=sorted(diags, key=lambda diag: (diag.code, diag.description)),
        key=lambda diag: (diag.code, diag.description),
    )

    for (code, description), diag_group in grouped_diags:
        print(
            ansiesc(
                "Error"
                + (f" [{code}]" if code is not None else "")
                + " - "
                + f"{description}:",
                fg=AnsiColor.Red,
                bold=True,
            ),
            file=sys.stderr,
        )
        print(
            file=sys.stderr,
        )

        for diag in diag_group:
            if diag.location is not None:
                emit_location(message, diag.location, diag.hint)
            if diag.help is not None:
                emit_help(diag.help)
            if diag.note is not None:
                emit_note(diag.note)

        print(file=sys.stderr, flush=True)

    # Summary
    if len(diags) > 0:
        print(
            ansiesc(
                f"{len(diags)} error{'s' if len(diags) != 1 else ''}",
                fg=AnsiColor.Red,
                bold=True,
            ),
            "in this commit message.",
            file=sys.stderr,
        )


def get_commit(steps_back: int = 0) -> t.Optional[tuple[str, str]]:
    hash_process = subprocess.run(
        f"git show HEAD~{steps_back} -s --pretty=%H",
        capture_output=True,
        shell=True,
        text=True,
    )

    message_process = subprocess.run(
        f"git show HEAD~{steps_back} -s --pretty=%B",
        capture_output=True,
        shell=True,
        text=True,
    )
    # Non-zero return code most likely means that we've stepped past the initial
    # commit.
    return (
        (hash_process.stdout, message_process.stdout)
        if hash_process.returncode == 0 and message_process.returncode == 0
        else None
    )


def get_merge_base() -> t.Optional[str]:
    merge_base_process = subprocess.run(
        "git merge-base HEAD main", capture_output=True, shell=True, text=True
    )
    return (
        merge_base_process.stdout
        if merge_base_process.returncode == 0
        else None
    )


if __name__ == "__main__":
    full_mode = len(sys.argv) > 1 and sys.argv[1] == "--full"
    full_mode_limit = int(sys.argv[2]) if len(sys.argv) > 2 else 500

    base = get_merge_base()
    for i in range(full_mode_limit):
        commit = get_commit(i)
        if commit is None:
            break

        hash, message = commit

        if not full_mode and hash == base:
            break

        subject = get_subject(message)
        print(
            ansiesc("Checking", fg=AnsiColor.Cyan, bold=True)
            + " message "
            + ansiesc(f'"{subject}"', fg=AnsiColor.White, bold=True)
            + f" (commit {ansiesc(hash[:10], fg=AnsiColor.Green, bold=True)})",
            flush=True,
        )

        diags = check_commit_message(message)
        emit_diagnostics(diags, message)
        if len(diags) > 0:
            exit(len(diags))

    print(
        ansiesc("Finished", fg=AnsiColor.Green, bold=True),
        "checking commit messages",
        flush=True,
    )
