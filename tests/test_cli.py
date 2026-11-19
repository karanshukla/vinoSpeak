import io
import json
from pathlib import Path

import pytest

from vino_speak import cli, model, tts


def test_parser_defaults_to_npu():
    arguments = cli.build_parser().parse_args(["Hello"])
    assert arguments.device == "NPU"
    assert arguments.voice is None
    assert arguments.model_dir is None


def test_without_a_voice_heart_reads_it(monkeypatch):
    asked = {}
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: asked.update(kwargs))
    cli.main(["Hello."])
    assert asked["voice"] == "af_heart"


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


def test_kind_document_reads_stdin_line_by_line(monkeypatch, capsys):
    monkeypatch.setattr("sys.stdin", io.StringIO("Results\nAlice\n90"))
    cli.main(["--file", "-", "--kind", "document", "--print-text"])
    assert capsys.readouterr().out == "Results.\nAlice.\n90.\n"


def test_kind_needs_a_file():
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["Hello", "--kind", "document"])
    assert exit_info.value.code == 2


DOCUMENTS = Path(__file__).parent / "fixtures" / "documents"


# Each .txt is what vinoSearch's FileText gave for the document beside it
# (2026-10-29), so CI checks the GUI's path without vinoSearch.
@pytest.mark.parametrize("name", ["report.pdf", "stations.docx", "meeting.pptx"])
def test_extracted_documents_read_as_expected(monkeypatch, capsys, name):
    monkeypatch.setattr("sys.stdin", io.StringIO((DOCUMENTS / f"{name}.txt").read_text()))
    cli.main(["--file", "-", "--kind", "document", "--print-text"])
    assert capsys.readouterr().out == (DOCUMENTS / f"{name}.expected").read_text()


@pytest.fixture
def cache(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path))
    model_dir = tmp_path / "vinospeak" / "kokoro-82m-int8-ov"
    model_dir.mkdir(parents=True)
    (model_dir / "openvino_model.bin").write_bytes(b"x" * 2_500_000)
    return tmp_path / "vinospeak"


def test_model_status_lists_what_is_on_disk(cache, capsys):
    cli.main(["--model-status"])
    assert capsys.readouterr().out == f"    2 MB  {cache / 'kokoro-82m-int8-ov'}\n"


def test_model_status_without_a_model(monkeypatch, tmp_path, capsys):
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path))
    cli.main(["--model-status"])
    assert "No model downloaded" in capsys.readouterr().out


def test_delete_model_deletes_the_cache_and_says_how_much(cache, capsys):
    cli.main(["--delete-model"])
    assert "Deleted the model and its compiled cache (2 MB)" in capsys.readouterr().out
    assert not cache.exists()
    cli.main(["--delete-model"])
    assert "Nothing to delete" in capsys.readouterr().out


def test_delete_model_leaves_a_model_dir_alone(cache, tmp_path):
    mine = tmp_path / "mine"
    mine.mkdir()
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--delete-model", "--model-dir", str(mine)])
    assert exit_info.value.code == 2
    assert mine.is_dir() and cache.is_dir()


def test_delete_model_reports_a_download_in_progress(cache, monkeypatch, capsys):
    def busy():
        raise model.ModelBusyError("The model is downloading")

    monkeypatch.setattr(model, "delete_cached", busy)
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--delete-model"])
    assert exit_info.value.code == 2
    assert "downloading" in capsys.readouterr().err


@pytest.mark.parametrize(
    ("n", "shown"), [(999, "1 kB"), (163_000_000, "163 MB"), (1_250_000_000, "1.2 GB")]
)
def test_sizes_read_as_people_say_them(n, shown):
    assert cli.size(n) == shown


def test_sizes_are_shown_as_the_window_shows_them():
    """The shared table gui/src/model_cache.rs is tested against too."""
    rules = json.loads((Path(__file__).parent / "fixtures/shared_with_window.json").read_text())
    assert [cli.size(n) for n, _ in rules["sizes"]] == [shown for _, shown in rules["sizes"]]


def test_version_prints_the_program_and_its_version(monkeypatch, capsys):
    monkeypatch.setattr(cli, "__version__", "1.2.3")
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--version"])
    assert exit_info.value.code == 0
    assert capsys.readouterr().out == "vinospeak 1.2.3\n"


def test_list_voices_prints_each_voice_and_its_language_without_a_model(monkeypatch, capsys):
    monkeypatch.setattr(model, "ensure_model", lambda *a, **k: pytest.fail("loaded the model"))
    cli.main(["--list-voices"])
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 41
    assert lines[0].split() == ["af_alloy", "en-us", "female", "American", "English"]
    assert "(default)" in next(line for line in lines if line.startswith("af_heart"))


def test_without_language_the_voice_chooses_it(monkeypatch):
    asked = {}
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: asked.update(kwargs))
    cli.main(["Hola.", "--voice", "ef_dora"])
    assert asked["language"] is None
    assert asked["voice"] == "ef_dora"


def test_subtitles_need_an_output(capsys):
    with pytest.raises(SystemExit):
        cli.main(["Hello", "--subtitles", "a.srt"])
    assert "give --output too" in capsys.readouterr().err


def test_subtitles_reach_the_save_and_are_reported(monkeypatch, capsys, tmp_path):
    asked = {}
    monkeypatch.setattr(cli, "synthesize", lambda text, output, **kwargs: asked.update(kwargs))
    cli.main(["Hello", "--output", str(tmp_path / "a.mp3"), "--subtitles", str(tmp_path / "a.vtt")])
    assert asked["subtitles"] == tmp_path / "a.vtt"
    assert f"Subtitles saved to {tmp_path / 'a.vtt'}" in capsys.readouterr().out


def test_progress_is_shown_only_to_a_terminal(monkeypatch, capsys):
    monkeypatch.setattr(cli.sys.stderr, "isatty", lambda: False, raising=False)
    assert cli.progress_reporter() is None


def test_progress_rewrites_one_line_and_ends_it_when_done(monkeypatch, capsys):
    monkeypatch.setattr(
        cli.sys, "stderr", type("Tty", (io.StringIO,), {"isatty": lambda s: True})()
    )
    report = cli.progress_reporter()
    report(1, 2)
    report(2, 2)
    assert cli.sys.stderr.getvalue() == "\rSaving: 1 of 2 chunks\rSaving: 2 of 2 chunks\n"


def test_an_unknown_output_format_is_refused_before_the_model_loads(monkeypatch, capsys, tmp_path):
    monkeypatch.setattr(tts, "Synthesizer", lambda *a: pytest.fail("loaded the model"))
    with pytest.raises(SystemExit):
        cli.main(["Hello", "--output", str(tmp_path / "a.aiff")])
    assert "not '.aiff'" in capsys.readouterr().err


@pytest.fixture
def config_home(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "config"))
    return tmp_path / "config"


def test_print_text_respells_with_the_default_lexicon(config_home, capsys):
    (config_home / "vinospeak").mkdir(parents=True)
    (config_home / "vinospeak" / "lexicon.toml").write_text('[words]\nsql = "sequel"\n')
    cli.main(["Write sql.", "--print-text"])
    assert capsys.readouterr().out == "Write sequel.\n"


def test_a_lexicon_given_replaces_the_default(config_home, tmp_path, capsys):
    mine = tmp_path / "mine.toml"
    mine.write_text('[words]\nsql = "ess queue ell"\n')
    cli.main(["Write sql.", "--print-text", "--lexicon", str(mine)])
    assert capsys.readouterr().out == "Write ess queue ell.\n"


def test_a_broken_lexicon_is_a_usage_error(config_home, tmp_path, capsys):
    (tmp_path / "bad.toml").write_text("[words")
    with pytest.raises(SystemExit):
        cli.main(["Hi.", "--lexicon", str(tmp_path / "bad.toml")])
    assert "isn't valid TOML" in capsys.readouterr().err


def test_the_lexicon_reaches_speak(config_home, monkeypatch, tmp_path):
    mine = tmp_path / "mine.toml"
    mine.write_text('[words]\nsql = "sequel"\n')
    asked = {}
    monkeypatch.setattr(cli, "speak", lambda text, **kwargs: asked.update(kwargs))
    cli.main(["Hi.", "--lexicon", str(mine)])
    assert asked["lexicon"].words == {"sql": "sequel"}


def test_an_ssml_file_prints_its_text_and_pauses(config_home, tmp_path, capsys):
    path = tmp_path / "script.ssml"
    path.write_text('<speak>\n  <p>Hello</p>\n  <break time="2s"/>\n  World.\n</speak>\n')
    cli.main(["--file", str(path), "--print-text"])
    assert capsys.readouterr().out == "Hello.\n[pause 2 s]\nWorld.\n"


def test_bad_ssml_is_a_usage_error(config_home, capsys):
    with pytest.raises(SystemExit):
        cli.main(["<speak>open", "--print-text"])
    assert "well-formed" in capsys.readouterr().err


class FakeTranslator:
    made = []

    def __init__(self, device):
        self.device = device
        FakeTranslator.made.append(self)

    def translate(self, text, language):
        return f"[{language}] {text}"


@pytest.fixture
def translation(monkeypatch):
    FakeTranslator.made.clear()
    monkeypatch.setattr(cli.translate, "Translator", FakeTranslator)
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: True)
    return FakeTranslator.made


def test_translate_without_the_model_says_how_to_get_it_and_downloads_nothing(monkeypatch, capsys):
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: False)
    monkeypatch.setattr(cli.translate, "download", lambda: pytest.fail("downloaded"))
    with pytest.raises(SystemExit):
        cli.main(["Hello.", "--translate", "es"])
    assert "--download-translation-model" in capsys.readouterr().err


def test_print_text_prints_the_translation(translation, capsys):
    cli.main(["Hello.", "--translate", "fr-fr", "--print-text", "--device", "CPU"])
    assert capsys.readouterr().out == "[fr-fr] Hello.\n"
    assert translation[0].device == "CPU"


def test_a_saved_translation_is_read_in_its_language_by_its_voice(
    translation, monkeypatch, tmp_path
):
    asked = {}
    monkeypatch.setattr(cli, "synthesize", lambda text, output, **k: asked.update(k, text=text))
    cli.main(["Hello.", "--translate", "it", "--output", str(tmp_path / "a.mp3")])
    assert asked["text"] == "[it] Hello."
    assert (asked["language"], asked["voice"]) == ("it", "if_sara")


def test_a_spoken_translation_is_handed_to_speak_untranslated(translation, monkeypatch):
    asked = {}
    monkeypatch.setattr(cli, "speak", lambda text, **k: asked.update(k, text=text))
    cli.main(["Hello.", "--translate", "es", "--voice", "bm_george"])
    assert asked["text"] == "Hello."
    assert (asked["translate_to"], asked["voice"]) == ("es", "bm_george")
    assert asked["translator"] is translation[0]


@pytest.mark.parametrize(
    ("argv", "complaint"),
    [
        (["Hi.", "--translate", "es", "--language", "es"], "drop --language"),
        (["<speak>Hi.</speak>", "--translate", "es"], "SSML can't be translated"),
    ],
)
def test_what_translate_cant_take(translation, capsys, argv, complaint):
    with pytest.raises(SystemExit):
        cli.main(argv)
    assert complaint in capsys.readouterr().err
    assert translation == []


def test_hindi_is_not_a_translation_target(capsys):
    with pytest.raises(SystemExit):
        cli.main(["Hi.", "--translate", "hi"])
    assert "invalid choice: 'hi'" in capsys.readouterr().err


def test_download_translation_model_downloads_once(monkeypatch, capsys):
    downloaded = []
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: bool(downloaded))
    monkeypatch.setattr(cli.translate, "download", lambda: downloaded.append(1))
    cli.main(["--download-translation-model"])
    cli.main(["--download-translation-model"])
    out = capsys.readouterr()
    assert downloaded == [1]
    assert "Downloading SmolLM3-3B (1.7 GB)" in out.err
    assert "already in" in out.out


def test_a_failed_translation_download_is_a_usage_error(monkeypatch, capsys):
    def fail():
        raise RuntimeError("The download failed verification.")

    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: False)
    monkeypatch.setattr(cli.translate, "download", fail)
    with pytest.raises(SystemExit):
        cli.main(["--download-translation-model"])
    assert "failed verification" in capsys.readouterr().err


def test_delete_translation_model_says_what_it_freed(monkeypatch, capsys):
    freed = [2_100_000_000, 0]
    monkeypatch.setattr(cli.translate, "delete", lambda: freed.pop(0))
    cli.main(["--delete-translation-model"])
    cli.main(["--delete-translation-model"])
    assert (
        capsys.readouterr().out == "Deleted the translation model (2.1 GB).\nNothing to delete.\n"
    )


def test_the_translation_model_commands_take_no_text():
    with pytest.raises(SystemExit):
        cli.main(["Hi.", "--download-translation-model"])


def test_model_status_lists_the_translation_model_when_downloaded(cache, monkeypatch, capsys):
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: True)
    monkeypatch.setattr(cli.translate, "disk_usage", lambda: 1_702_000_000)
    cli.main(["--model-status"])
    assert "1.7 GB" in capsys.readouterr().out.splitlines()[-1]
