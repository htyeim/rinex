> Historical mixed-NAV snapshot. For current diagnostic behavior and validation, see `nav_directed_frame_paths.md`.

# Mixed NAV remaining frame decision, 2024-05-10 03:00 GPST

This completes the remaining first-epoch frame unit after F1 and the verified
R02 path. The original NAV SHA-256 is
`9cffb1b1f2978c46e8352d78f603dd089ecd7f32312c1a6c4ae585b5d0e538b6`.
The byte-preserving NAV fixture SHA-256 is
`324cbfaab4bc404490d8af375fa20b7324f04cfcae4a3b91aca6a68add8040d6`;
the OBS fixture SHA-256 is
`251ccd03b457cc2fa5b46075c077198e1364fecd98bf340cb62aba05edb8bc52`.
Original line spans and record hashes are in
`tests/fixtures/mixed_2024131_first_epoch_manifest.json`. The query uses
`UnknownHealthPolicy::Reject`, `FrameRequest::Wgs84` and default options.

## Opened paths

- **E03 INAV and other selected Galileo records in May 2024:** the source is
  GTRF23v01, as inferred from the [ESA GGSP 2023 presentation, page 7](https://navigation-office.esa.int/attachments/32835744/1/GTRF_IGS_Stop_6.pdf),
  which says this realization was applicable from 2023-05-05 and aligned to
  ITRF2020. The existing source gate requires record, ToE and query within
  May 2024. Its first edge copies XYZ as a *marked zero-offset approximation*
  to ITRF2020; the existing EPSG:10608 inverse then copies it numerically to
  WGS84 G2296. The direct ITRF2020 target is also available. The full path
  remains `MarkedApproximation`, with ordered edges, `CAUTION`, no cross-frame
  velocity and no strict error bound. ESA's station statistic is not a
  satellite-position bound. The independent Python orbit script
  `nav_galileo_e03_mixed_frame.py` (SHA-256
  `1e7d778e7bae4c3750dbf6a521a7f83f7ae3e13c4a7a478da055ca1b0c86fcaf`)
  reads the E03 INAV block at fixture line 870, ToE 442200 s, and evaluates
  the orbit at +600 s. Its XYZ is
  `[13527.734303005513, 20608.988501740743, -16393.79374112982] km`.
  The Rust test checks every component within `1e-8 km`. This checks orbit
  arithmetic and the declared zero-offset chain, not the physical offset.
- **QZSS JGS with ITRF2020 applied:** the [QZSS Cabinet Office coordinate history](https://qzss.go.jp/en/technical/dod/pnt/coordinate-system.html)
  explicitly covers broadcast ephemerides and lists ITRF2020 from 2023-11-09.
  The library starts on 2023-11-11 UTC, after the change date, and ends before
  2025-01-01 UTC as a conservative case window. LNAV record, ToE and query
  must all fit. JGS is a distinct source realization,
  `QzssJgsItrf2020Aligned`; it is **not** ITRF2020 identity. The first edge is
  a marked zero-offset approximation. It can be followed by the existing
  numerical ITRF2020 → ITRF2014 or G2296 edge. The 2 cm (95%) monitor-station
  statistic is not a satellite-position bound. A caller-asserted point tests
  these paths and policy gates; the real J04 has three unhealthy LNAV records
  and six unsupported CNAV/CNV2 records at this query, so selection returns
  none and no native or converted J04 position is claimed.

The old 2023 JGS → ITRF2014 path retains its own date window. Both new first
edges have no published correction parameters; zero is an explicitly marked
assumption, not a measured transformation. The existing EPSG and ITRF edges
retain their documented direction, units, rates and epoch policies in
`nav_frame_transform.md`. `NumericalOnly`, `warnings_as_errors`, a strict
`max_frame_operation_error_m`, and `require_velocity` each reject the new approximate
chains. The tested date boundaries reject QZSS points outside its new window.

## Closed source and message units

- **C02 D2:** the real record selects and propagates in native BDCS coordinates,
  but its 2024-05 *specific realization* remains unknown. The
  [2022 IGS workshop BDCS(2019v01) presentation](https://files.igs.org/pub/resource/pubs/workshop/2022/TourdelIGS4_05_Hu.pdf)
  supplies a version and transformation parameters for its then-current
  solution, not a 2024-05 C02 D2 assignment. The [BeiDou B2b beta ICD, section 3.2](https://en.beidou.gov.cn/SYSTEMS/Officialdocument/202001/P020231201549984668644.pdf)
  gives the system-level BDCS definition but does not identify this D2
  record's dated realization.
  The existing 2022-06 C10/C20/C05 inference does not extend to C02. Result:
  `UnknownSourceRealization`. A dated provider assignment is required to open
  a path; changing the ellipsoid label is insufficient.
- **I03 NavIC:** the [ISRO SPS ICD v1.1, section 5.8 and Appendix B](https://www.isro.gov.in/media_isro/pdf/SateliteNavigation/irnss_sps_icd_version1.1-2017.pdf)
  identifies WGS 84 for user positions and provides ECEF orbit equations. It
  does not identify I03's 2024-05 WGS 84 realization or a transformation to
  G2296/ITRF2020. The [ISRO navigation service page](https://www.isro.gov.in/SatelliteNavigationServices.html)
  likewise does not provide these dated parameters. Result:
  `UnsupportedSource(NavicBroadcastWgs84)`. The 2023 I02 nominal exception
  cannot be generalized to I03.
- **SBAS:** the one selected 2024 record propagates in its native SBAS frame,
  but has no supported target relation. A 2023 GAGAN S27 nominal sample cannot
  be generalized. The [EGNOS 2024 service definition, section 4.2](https://egnos.gsc-europa.eu/new_egnos_ops/sites/default/files/documents/egnos_sol_sdd_in_force.pdf)
  describes *EGNOS* ETRF, not a relation for another SBAS service or the exact
  selected broadcast record; it also cautions that EGNOS GEO ranging is not
  supported in that service. Result: `UnsupportedSource(SbasBroadcast)`.
  Provider, broadcast-message and dated frame evidence are needed separately.

## Run and inspect

From this repository root:

```sh
python3 tests/reference/nav_galileo_e03_mixed_frame.py
cargo test --offline --features nav --test nav_mixed_remaining_frames --test nav_mixed_first_epoch --test nav_galileo_frame --test nav_qzss_frame
cargo run --offline --features nav --example nav_first_epoch -- tests/fixtures/obs_mixed_2024131_first_epoch.rnx tests/fixtures/nav_mixed_2024131_first_epoch.rnx
cargo run --offline --features nav --example nav_frame -- tests/fixtures/nav_mixed_2024131_first_epoch.rnx E03 '2024-05-10T03:00:00 GPST' --target wgs84
```

The 56-row CLI now reports 17 `MarkedApproximation`, 11 `NativeIdentity`,
9 `selection_none`, 15 `UnknownSourceRealization`, 3 unsupported NavIC and
1 unsupported SBAS. It reports no `NoPath` for selected, propagated records.
The selected record and native XYZ are printed before any frame result; no
J04 native state is fabricated.

For VS Code, use **Debug Test** above
`e03_reaches_g2296_with_independent_raw_orbit_reference` in
`tests/nav_mixed_remaining_frames.rs`. Break at `NavCandidate::spatial_state_at`
where `realization` is chosen and at `FrameTransformer::to_frame_inner` after
`source_id`/`target_id` resolution. Inspect `point.position_km`, ordered
`edge_info`, `result.position_status()`, and `result.velocity_km_s`. The
terminal tests have run; the user's VS Code breakpoint behavior and physical
frame accuracy remain unverified by the agent.
