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
