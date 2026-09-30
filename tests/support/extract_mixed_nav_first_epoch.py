"""Extract byte-preserving first-epoch OBS and competing RINEX 4 NAV EPH.

Usage: python3 tests/support/extract_mixed_nav_first_epoch.py OBS NAV
Run from the repository root. The input files are never modified.
"""

import datetime as dt
import hashlib
import json
import pathlib
import re
import sys


OUT = pathlib.Path("tests/fixtures")
QUERY = dt.datetime(2024, 5, 10, 3, 0, 0)
WINDOW_S = {"G": 7200, "R": 900, "E": 10800, "C": 21600,
            "J": 3600, "I": 7200, "S": 360}
EXPECTED_SHA = {
    "obs": "9d9bd52a22be147b6c75de89ccb2ca6f852413736ed42e95f2690bdf126c4f0a",
    "nav": "9cffb1b1f2978c46e8352d78f603dd089ecd7f32312c1a6c4ae585b5d0e538b6",
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def header_end(lines):
    return next(i + 1 for i, line in enumerate(lines) if b"END OF HEADER" in line)


def write_fixture(name, chunks):
    data = b"".join(chunks)
    (OUT / name).write_bytes(data)
    return {"file": name, "sha256": digest(data), "bytes": len(data)}


def main(obs_path, nav_path):
    obs_bytes, nav_bytes = obs_path.read_bytes(), nav_path.read_bytes()
    assert digest(obs_bytes) == EXPECTED_SHA["obs"], "unexpected OBS original"
    assert digest(nav_bytes) == EXPECTED_SHA["nav"], "unexpected NAV original"
    obs, nav = obs_bytes.splitlines(keepends=True), nav_bytes.splitlines(keepends=True)
    obs_header, nav_header = header_end(obs), header_end(nav)
    first = obs_header
    assert obs[first].startswith(b"> 2024 05 10 03 00  0.0000000  0 56")
    obs_stop = next(i for i in range(first + 1, len(obs)) if obs[i].startswith(b">"))
    svs = [line[:3].decode("ascii") for line in obs[first + 1:obs_stop]]
    assert len(svs) == len(set(svs)) == 56 and all(re.fullmatch(r"[A-Z][0-9]{2}", sv) for sv in svs)

    obs_info = write_fixture("obs_mixed_2024131_first_epoch.rnx", obs[:obs_header] + obs[first:obs_stop])
    starts = [i for i in range(nav_header, len(nav)) if nav[i].startswith(b"> ")]
    starts.append(len(nav))
    kept, spans = [], []
    for start, stop in zip(starts, starts[1:]):
        descriptor = nav[start].decode("ascii").split()
        if len(descriptor) < 4 or descriptor[1] != "EPH" or descriptor[2] not in svs:
            continue
        sv = descriptor[2]
        label = nav[start + 1][:23].decode("ascii")
        match = re.match(r"^([A-Z][0-9]{2})\s+(\d{4})\s+(\d{1,2})\s+(\d{1,2})\s+(\d{1,2})\s+(\d{1,2})\s+(\d{1,2})", label)
        assert match and match.group(1) == sv, (start + 1, sv)
        toc = dt.datetime(*map(int, match.groups()[1:]))
        # RINEX NAV ToC uses constellation time; 60 s covers their offset
        # from the GPST query. The library applies its exact epoch rules.
        if abs((toc - QUERY).total_seconds()) > WINDOW_S[sv[0]] + 60:
            continue
        kept.extend(nav[start:stop])
        spans.append({"sv": sv, "message": descriptor[3], "lines": [start + 1, stop],
                      "toc_label": toc.isoformat(), "sha256": digest(b"".join(nav[start:stop]))})

    nav_info = write_fixture("nav_mixed_2024131_first_epoch.rnx", nav[:nav_header] + kept)
    manifest = {
        "rule": "OBS original header and first 56-SV epoch; NAV original header and all EPH for those SVs with |ToC label - 2024-05-10 03:00:00| <= selection half-window + 60 s time-scale margin; original record order and bytes retained",
        "query": "2024-05-10T03:00:00 GPST",
        "originals": {"obs": {"name": obs_path.name, "sha256": digest(obs_bytes)},
                      "nav": {"name": nav_path.name, "sha256": digest(nav_bytes)}},
        "obs": {**obs_info, "original_header_lines": [1, obs_header],
                "original_epoch_lines": [first + 1, obs_stop], "svs": svs},
        "nav": {**nav_info, "original_header_lines": [1, nav_header],
                "record_count": len(spans), "records": spans},
    }
    (OUT / "mixed_2024131_first_epoch_manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"OBS {obs_info['bytes']} bytes, 56 SV; NAV {nav_info['bytes']} bytes, {len(spans)} EPH")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: python3 tests/support/extract_mixed_nav_first_epoch.py OBS NAV")
    main(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]))
