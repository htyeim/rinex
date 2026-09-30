> Historical F1/F2 snapshot. For the current F4 diagnostic behavior and validation, see `nav_f4_frame_paths.md`.

# 2024-05-10 mixed NAV first-epoch diagnostic (F1)

Run from this repository root. The fixture is made from the original Septentrio OBS and DLR mixed NAV files named in `tests/fixtures/mixed_2024131_first_epoch_manifest.json`. The manifest records original SHA-256 values, fixture SHA-256 values, original one-based line spans, per-record hashes, and all 56 OBS SV labels. The extractor copies the original header and records as bytes. It does not normalize RINEX field widths, values, or line endings.

The OBS slice contains the header and the complete 2024-05-10 03:00:00 GPST epoch. The NAV slice contains the header and 879 EPH blocks for those 56 SVs with the raw ToC label within the constellation selection half-window plus 60 s of the query label. The extra minute covers differences between the raw constellation clock labels and GPST; the Rust selector makes the actual time and validity decision. The 879 raw blocks have 822 distinct (SV, message, ToC label) triples; the parsed NAV has 822 EPH keys and reports zero rejected or unsupported parse records. Do not interpret the 57 repeated triples as 57 independent selected candidates.

## Run

```sh
cargo test --offline --features nav --test nav_mixed_first_epoch
cargo run --offline --features nav --example nav_first_epoch -- \
  tests/fixtures/obs_mixed_2024131_first_epoch.rnx \
  tests/fixtures/nav_mixed_2024131_first_epoch.rnx
```

Append `--candidates` to print each candidate's message, ToC, ToE, raw health value, and rejection, plus any parse diagnostics. To check fixture selection against the original NAV, append `--compare-nav /path/to/BRD400DLR_S_20241310000_01D_MN.rnx`; this reads the entire original once. Re-create the fixtures only when the original hashes match:

```sh
python3 tests/support/extract_mixed_nav_first_epoch.py /path/to/test_obs_read-SDUZ00ATA_R_20241310300_01H_01S_MO.rnx /path/to/BRD400DLR_S_20241310000_01D_MN.rnx
```

The 56-row CLI uses `UnknownHealthPolicy::Reject`, `FrameRequest::Wgs84`, and `TransformOptions::default()`. It displays the original OBS SV labels, selected message/ToC/ToE, health field, candidate rejection counts, native XYZ in km, source realization and evidence, output or the original `FrameError`, catalog, ordered edges, and any CAUTION note. `selection=none` has no invented native state. `propagation_error` and `point_error` have separate categories.

For a single selected record and a concrete target, use:

```sh
cargo run --offline --features nav --example nav_frame -- \
  tests/fixtures/nav_mixed_2024131_first_epoch.rnx G04 \
  '2024-05-10T03:00:00 GPST' --target itrf2014
```

`--target` accepts `wgs84`, `g2296`, `itrf2020`, or `itrf2014`. Frame failure prints the native state and `frame_error` to identify the stage that failed.

## Observed F1 result

This is the F1 baseline before the verified R02 and subsequent mixed-frame
extensions. See `nav_glonass_r02_mixed_frame.md` and
`nav_mixed_remaining_frames.md` for the current 56-row result.

The original and fixture selected the same key for all 56 SVs. With the default request, 11 were `NativeIdentity`; 9 had no selected record; the remainder were 17 `NoPath`, 15 `UnknownSourceRealization`, 3 `UnsupportedSource(NavicBroadcastWgs84)`, and 1 `UnsupportedSource(SbasBroadcast)`. In particular, G04 was native WGS84/G2296; R02 and E03 propagated but had `NoPath`; C02 propagated with unknown source realization; I03 propagated but had unsupported NavIC source; J04 had no selected record, with unhealthy and unsupported message candidates. These are observations of this code and input, not frame accuracy validation.

The CLI also checks whether `t_rx - pseudorange/c` changes the selected key for each SV with a positive first pseudorange. It found zero changes. This is a selection sensitivity check only: it does not correct satellite or receiver clocks or atmospheric delay, so the computed instant is not a validated transmit time.

## VS Code Debug Test

With the repo open in VS Code and Rust Analyzer available, open `tests/nav_mixed_first_epoch.rs` and use **Debug Test** above `representative_real_records_keep_selection_propagation_and_frame_errors_distinct`. Set a breakpoint at the `nav_select_ephemeris` call and inspect `sv`, `t`, `report.candidates`, `report.selected`, then step through `spatial_state_at` and `FrameTransformer.to_frame`. Check that a selected record's `native.state.position_km` exists before a frame error, while J04 has no selected record. VS Code breakpoint behavior remains for the user to verify locally; the terminal test does not establish debugger availability.
