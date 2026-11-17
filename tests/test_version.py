import json

from vino_speak.version import version_of

DIRECTORY = json.dumps({"url": "file:///src/vinoSpeak", "dir_info": {}})
EDITABLE = json.dumps({"url": "file:///src/vinoSpeak", "dir_info": {"editable": True}})
GIT = json.dumps({"url": "https://github.com/x/y", "vcs_info": {"vcs": "git", "commit_id": "a"}})
WHEEL = json.dumps(
    {"url": "file:///tmp/vinospeak-0.8.0-py3-none-any.whl", "archive_info": {"hash": "sha256=a"}}
)


def test_a_release_build_reports_its_version():
    assert version_of("0.8.0", WHEEL) == "0.8.0"
    assert version_of("0.8.0", None) == "0.8.0"  # PyPI writes no direct_url.json
    assert version_of("0.8.0", "not json") == "0.8.0"


def test_an_unstamped_build_reports_dev():
    for direct_url in (DIRECTORY, EDITABLE, GIT):
        assert version_of("0.8.0", direct_url) == "dev"
