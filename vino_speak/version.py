"""Whether this install is a release or a source build."""

import json
from importlib.metadata import PackageNotFoundError, distribution

DEV = "dev"


def version_of(release_version: str, direct_url: str | None) -> str:
    """The release version, or `dev` for a build from a directory or a VCS checkout.

    `direct_url` is the dist's PEP 610 `direct_url.json`. Only a local directory
    (`dir_info`, editable or not) or a VCS URL (`vcs_info`) is a source build.
    PyPI installs have no file, and the release installer's wheel has
    `archive_info`, so both keep the version.
    """
    if direct_url is None:
        return release_version
    try:
        origin = json.loads(direct_url)
    except ValueError:
        return release_version
    if isinstance(origin, dict) and ("dir_info" in origin or "vcs_info" in origin):
        return DEV
    return release_version


def installed_version() -> str:
    """`version_of` for the installed `vinospeak`; `dev` when none is installed."""
    try:
        dist = distribution("vinospeak")
    except PackageNotFoundError:
        return DEV
    return version_of(dist.version, dist.read_text("direct_url.json"))
