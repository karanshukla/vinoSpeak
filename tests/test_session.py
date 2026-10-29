import json
import zipfile
from pathlib import Path

import pytest

from vino_speak import cli, session

# Copies of vinoWhisper's tests/fixtures (2026-10-29), as vinoSearch keeps them
FIXTURES = Path(__file__).parent / "fixtures"
SESSION = FIXTURES / "session_v1.vwsession"
NO_SUMMARY = FIXTURES / "session_v1_nosummary.vwsession"


def bundle(path: Path, *, mimetype=session.MIMETYPE, stored=True, fmt=session.FORMAT, parts=None):
    """A bundle laid out as vinoWhisper writes one, with one thing changed."""
    parts = (
        {
            "transcript.txt": "vinoWhisper transcript, 2026-01-02 09:00, microphone, NPU\n\n[00:00] Hi."
        }
        if parts is None
        else parts
    )
    with zipfile.ZipFile(path, "w") as archive:
        method = zipfile.ZIP_STORED if stored else zipfile.ZIP_DEFLATED
        archive.writestr("mimetype", mimetype, compress_type=method)
        for name, text in parts.items():
            archive.writestr(name, text, compress_type=zipfile.ZIP_DEFLATED)
        archive.writestr("attestation.json", json.dumps({"format": fmt}))
    return path


def test_the_summary_is_read_by_default_and_said_to_be_machine_written():
    text = session.read(SESSION)
    assert text == (FIXTURES / "session_v1.summary.expected").read_text().rstrip("\n")
    assert text.startswith("This summary was machine-written.")
    assert "[" not in text and "check it at the time stamps" not in text


def test_the_transcript_says_the_day_and_drops_the_stamps():
    assert session.read(SESSION, "transcript") == (
        "Recorded on 27 October 2026.\n\n"
        "Welcome to the talk about local captioning on the NPU.\n\n"
        "Café, naïve, “quoted” — and an ellipsis…\n\n"
        "Past an hour the minutes keep counting."
    )


def test_without_a_summary_the_transcript_is_read():
    assert session.read(NO_SUMMARY).startswith("Recorded on 27 October 2026.")
    with pytest.raises(ValueError, match="isn't summarised yet"):
        session.read(NO_SUMMARY, "summary")


def test_summary_stamps_of_both_shapes_go_and_a_paragraph_ends_a_sentence():
    text = "# Title\n\n- one [00:10] [01:20]\n- two (1:10)\n- keeps [this] link"
    assert session.summary(text) == "Title.\n\none.\ntwo.\nkeeps [this] link."
    assert session.transcript("[00:00] no stop\n\n[61:05] past an hour") == (
        "no stop.\n\npast an hour."
    )


def test_a_header_with_no_real_month_is_dropped_unsaid():
    assert (
        session.transcript("vinoWhisper transcript, 2026-13-01 10:00, x, CPU\n\n[00:00] A.") == "A."
    )


@pytest.mark.parametrize(
    ("changed", "error"),
    [
        ({"mimetype": b"application/zip"}, "not a vinoWhisper session"),
        ({"stored": False}, "not a vinoWhisper session"),
        ({"fmt": "vinowhisper-session/2"}, "'vinowhisper-session/2' session"),
        ({"parts": {}}, "has no transcript.txt"),
        ({"parts": {"transcript.txt": b"\xff"}}, "codec can't decode"),
        ({"parts": {f"extra{i}": "" for i in range(16)}}, "too many entries"),
    ],
)
def test_a_bundle_this_version_cant_read_is_refused(tmp_path, changed, error):
    with pytest.raises(ValueError, match=error):
        session.load(bundle(tmp_path / "x.vwsession", **changed))


def test_mimetype_must_come_first(tmp_path):
    path = tmp_path / "x.vwsession"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("transcript.txt", "x")
        archive.writestr("mimetype", session.MIMETYPE)
    with pytest.raises(ValueError, match="no mimetype first"):
        session.load(path)


def test_not_a_zip_is_refused(tmp_path):
    (tmp_path / "x.vwsession").write_text("plain text")
    with pytest.raises(ValueError, match="not a vinoWhisper session"):
        session.load(tmp_path / "x.vwsession")


def test_an_oversized_part_is_refused_whatever_its_header_claims(tmp_path, monkeypatch):
    monkeypatch.setattr(session, "MAX_PART_BYTES", 10)
    path = bundle(tmp_path / "x.vwsession", parts={"transcript.txt": "x" * 11})
    with pytest.raises(ValueError, match="transcript.txt is too large"):
        session.load(path)
    monkeypatch.setattr(session, "MAX_TOTAL_BYTES", 10)
    with pytest.raises(ValueError, match="too large for a session"):
        session.load(path)


def test_cli_prints_the_part_asked_for(capsys):
    cli.main(["--file", str(SESSION), "--part", "summary", "--print-text"])
    assert capsys.readouterr().out == (FIXTURES / "session_v1.summary.expected").read_text()
    cli.main(["--file", str(SESSION), "--part", "transcript", "--print-text"])
    assert capsys.readouterr().out.startswith("Recorded on 27 October 2026.")


@pytest.mark.parametrize(
    ("argv", "error"),
    [
        (["--file", "a.md", "--part", "summary"], "--part is for a vinoWhisper session"),
        (["--file", str(SESSION), "--kind", "plain"], "--kind doesn't apply"),
        (["--file", str(NO_SUMMARY), "--part", "summary"], "isn't summarised yet"),
    ],
)
def test_cli_refuses_part_misuse(capsys, argv, error):
    with pytest.raises(SystemExit) as exit_info:
        cli.main([*argv, "--print-text"])
    assert exit_info.value.code == 2
    assert error in capsys.readouterr().err
