"""Every `test_file.py::test_name` a comment or docstring cites must be a real
test, so a rule that names its test fails CI when the test is renamed or
deleted. See docs/comment-style.md; gui/tests/doc_links.rs checks the Rust."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TESTS = ROOT / "tests"
CITED = re.compile(r"`(test_\w+\.py)::(test_\w+)`")


def cited_tests(text: str) -> list[tuple[str, str]]:
    return CITED.findall(text)


def resolves(test_file: str, name: str, tests: Path = TESTS) -> bool:
    try:
        text = (tests / test_file).read_text(encoding="utf-8")
    except OSError:
        return False
    return re.search(rf"^def {name}\(", text, re.M) is not None


def test_every_cited_test_exists():
    sources = [*(ROOT / "vino_speak").rglob("*.py"), *(ROOT / "scripts").glob("*.py")]
    broken = [
        f"{source.relative_to(ROOT)}:{line_no}: `{test_file}::{name}`"
        for source in sources
        for line_no, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1)
        for test_file, name in cited_tests(line)
        if not resolves(test_file, name)
    ]
    assert not broken, "no such test:\n" + "\n".join(broken)


def test_the_check_finds_a_cited_test_and_rejects_a_missing_one(tmp_path):
    (tmp_path / "test_x.py").write_text("def test_real():\n    pass\n\n\ndef helper():\n    pass\n")
    line = "# (pinned by `test_x.py::test_real`, `test_x.py::test_gone`)"
    assert cited_tests(line) == [("test_x.py", "test_real"), ("test_x.py", "test_gone")]
    assert resolves("test_x.py", "test_real", tmp_path)
    assert not resolves("test_x.py", "test_gone", tmp_path)
    assert not resolves("test_y.py", "test_real", tmp_path)
