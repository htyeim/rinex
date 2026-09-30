# Directed paths and nominal ECEF diagnostics

The current full target matrix and catalogue version are in
[`nav_all_frame_targets.md`](nav_all_frame_targets.md).

The source fixture manifest is `tests/fixtures/mixed_2024131_first_epoch_manifest.json`. Original NAV SHA-256 is `9cffb1b1f2978c46e8352d78f603dd089ecd7f32312c1a6c4ae585b5d0e538b6`; original OBS SHA-256 is `9d9bd52a22be147b6c75de89ccb2ca6f852413736ed42e95f2690bdf126c4f0a`. These hashes identify source files, not a caller's runtime input.

`FrameTransformer::to_frame` searches the installed directed catalogue edges within each edge's date window, preferring a numerical route over a marked approximation, then fewer edges and stable edge IDs. It preserves ordered `edge_info` and `edge_ids` derived from the same chosen path. Approximate edges cover PZ-90.11, JGS 2014 and 2020, GTRF23v01, and BDCS2019v01 in explicitly installed directions. Reverse operations have separate IDs and date checks; no edge is automatically reversed. Numerical G2296 ↔ ITRF2020 and ITRF2020 ↔ ITRF2014 remain available in their catalogue windows. A composition example is JGS 2014 → ITRF2014 → ITRF2020; the first edge keeps the result `MarkedApproximation`.

After a supported NAV record passes selection, health/data/validity checks and native propagation, lack of a frame relation gives `NominalAssumption`. The diagnostic copies finite native ECEF XYZ and records the requested target label, original `FrameError` in `fallback_reason`, stable assumption ID and `CAUTION` messages. It does not establish physical realization of the target. Source realization stays known or unknown as originally determined. A caller-asserted `SpatialPoint` can use evidenced paths; every caller-asserted result warns conditionally that NAV status was not checked if it is used as a broadcast satellite position. Nominal fallback additionally requires `allow_unverified_nominal: true`. A NAV point whose public source, epoch, XYZ or velocity fields are changed cannot reuse its earlier NAV qualification: it returns `InconsistentFrame`. Strict method, warnings, velocity and `max_frame_operation_error_m` gates still reject unsupported claims. The latter bounds only the frame operation, and no installed cross-frame edge has a strict satellite upper bound; a true native identity has zero frame-operation error.

With the checked mixed fixture at 2024-05-10 03:00:00 GPST, the `nav_directed_frame_paths` test finds 47 selected records, 9 unselected, and 57 nominal results across three concrete targets (19 records × G2296, ITRF2020 and ITRF2014). The `nav_first_epoch` CLI's generic WGS84 request reports 11 `NativeIdentity`, 17 `MarkedApproximation`, 19 `NominalAssumption` and 9 `selection_none`. These counts check code classification and zero-copy behavior, not satellite position or physical frame accuracy. BeiDou 2024 records retain unknown BDCS realization. NavIC retains an IRNSST/GPST-proxy caution. The selected S27 service remains a message-specific source, not a general SBAS physical relation. Old I02 and S27 2023 sample-specific assumption IDs remain restricted to their original records; other qualified NAV states use the general ID.

From the repository root:

```sh
cargo test --offline --features nav --test nav_directed_frame_paths --test nav_mixed_first_epoch --test nav_mixed_remaining_frames
cargo run --offline --features nav --example nav_first_epoch -- tests/fixtures/obs_mixed_2024131_first_epoch.rnx tests/fixtures/nav_mixed_2024131_first_epoch.rnx
cargo run --offline --features nav --example nav_frame -- tests/fixtures/nav_mixed_2024131_first_epoch.rnx C02 '2024-05-10T03:00:00 GPST' --target itrf2020
```

`nav_frame` also accepts `--warnings-as-errors` and `--allow-unknown-health`. Unknown health is rejected by default; when allowed, the selected candidate retains `health=None` and the CLI prints a separate NAV health caution. Known unhealthy or invalid data remain rejected.

A direct `nav_frame` run on the hash-checked original NAV selected E03 INAV at this epoch and returned `MarkedApproximation` for ITRF2020, with the single GTRF23v01 → ITRF2020 edge and no `fallback_reason`. Its native XYZ was `[13527.734303005513, 20608.98850174074, -16393.79374112982]` km. This run checks the real-file entry and classification; the zero-offset physical relationship remains unverified.

For debugging, enter `selected_real_records_reach_all_three_core_targets_without_upgrading_unknown_sources` in `tests/nav_directed_frame_paths.rs`. Break at `NavCandidate::spatial_state_at`, `FrameTransformer::to_frame_inner` near `source_id`, then `find_catalog_path` or `nominal_result`. Inspect `chosen.health`, `native.state.realization()`, `point.position_km`, `result.edge_info`, `result.fallback_reason`, and `result.cautions()`. C02 has finite native XYZ and unknown source realization; its ITRF2020 result copies XYZ with `UnknownSourceRealization`, while E03 uses a marked approximate path.
