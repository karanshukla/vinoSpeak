import io

import pytest

from vino_speak import cli


def test_parser_defaults_to_npu():
    arguments = cli.build_parser().parse_args(["Hello"])
    assert arguments.device == "NPU"
    assert arguments.voice == "af_heart"
    assert arguments.model_dir is None


def test_main_reports_errors_through_the_parser(monkeypatch, capsys):
    def refuse(*args, **kwargs):
        raise ValueError("Speed must be between 0.5 and 2.0.")

    monkeypatch.setattr(cli, "synthesize", refuse)
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["Hello", "--speed", "9"])
    assert exit_info.value.code == 2
    assert "Speed must be between" in capsys.readouterr().err


def test_main_prints_where_the_speech_went(monkeypatch, capsys, tmp_path):
    monkeypatch.setattr(cli, "synthesize", lambda *args, **kwargs: None)
    cli.main(["Hello", "--output", str(tmp_path / "out.wav")])
    assert str(tmp_path / "out.wav") in capsys.readouterr().out


def test_main_speaks_when_there_is_no_output(monkeypatch, capsys):
    spoken = []
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: spoken.append(text))
    monkeypatch.setattr(cli, "synthesize", lambda *args, **kwargs: pytest.fail("wrote a file"))
    cli.main(["Hello"])
    assert spoken == ["Hello"]
    assert capsys.readouterr().out == ""


def test_saving_to_a_file_does_not_speak(monkeypatch, tmp_path):
    monkeypatch.setattr(cli, "synthesize", lambda *args, **kwargs: None)
    monkeypatch.setattr(cli, "speak", lambda *args, **kwargs: pytest.fail("spoke a saved file"))
    cli.main(["Hello", "--output", str(tmp_path / "out.wav")])


def test_ctrl_c_exits_quietly_with_130(monkeypatch):
    def interrupted(*args, **kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr(cli, "speak", interrupted)
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["Hello"])
    assert exit_info.value.code == 130


def test_file_is_read_as_markdown(monkeypatch, tmp_path):
    spoken = []
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: spoken.append(text))
    (tmp_path / "notes.md").write_text("# Notes\n\nSee [the docs](https://x.dev/docs).\n")
    cli.main(["--file", str(tmp_path / "notes.md")])
    assert " ".join(spoken[0].split()) == "Notes. See the docs."


def test_print_text_prints_without_speaking(monkeypatch, capsys, tmp_path):
    monkeypatch.setattr(cli, "speak", lambda *args, **kwargs: pytest.fail("spoke"))
    (tmp_path / "a.txt").write_text("Plain words.")
    cli.main(["--file", str(tmp_path / "a.txt"), "--print-text"])
    assert capsys.readouterr().out == "Plain words.\n"


@pytest.mark.parametrize("argv", [[], ["Hello", "--file", "a.md"]])
def test_text_or_file_but_not_both(argv):
    with pytest.raises(SystemExit) as exit_info:
        cli.main(argv)
    assert exit_info.value.code == 2


def test_a_binary_file_is_refused(capsys, tmp_path):
    (tmp_path / "a.bin").write_bytes(b"\xff\xfe\x00\x81")
    with pytest.raises(SystemExit):
        cli.main(["--file", str(tmp_path / "a.bin")])
    assert "not a UTF-8 text file" in capsys.readouterr().err


def test_file_dash_reads_plain_text_from_stdin(monkeypatch, capsys):
    monkeypatch.setattr(
        "sys.stdin", io.StringIO("Page one, see https://x.dev/a.\n\n# not a heading")
    )
    cli.main(["--file", "-", "--print-text"])
    assert capsys.readouterr().out == "Page one, see x.dev.\n\n# not a heading\n"


def test_audio_device_reaches_speak(monkeypatch):
    devices = []
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: devices.append(kwargs["audio_device"]))
    cli.main(["Hello", "--audio-device", "hdmi_sink"])
    assert devices == ["hdmi_sink"]


def test_audio_device_and_output_dont_mix(tmp_path):
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["Hello", "--audio-device", "x", "--output", str(tmp_path / "a.wav")])
    assert exit_info.value.code == 2
