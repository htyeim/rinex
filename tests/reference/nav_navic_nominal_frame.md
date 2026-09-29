# N09d NavIC I02 LNAV nominal ITRF2014 position

## What the result means

The [ISRO NavIC SPS ICD 1.1](https://www.isro.gov.in/media_isro/pdf/Missions/irnss_sps_icd_version1.1-2017.pdf)
specifies WGS84 for user-position computation and supplies the broadcast
orbit equations. It does not identify the operational WGS84 realization
or its dated relation to ITRF2014 for this record. The
[NGA WGS84 reference-frame page](https://earth-info.nga.mil/?action=wgs84&dir=wgs84)
describes the GPS monitoring-station realization; that relation is not
assigned to NavIC. The source remains `NavicBroadcastWgs84` with
`FrameRealization::Unknown`.

At the user's request, `BestAvailable` now returns a **nominal ITRF2014**
position by copying the real I02 native WGS84-family ECEF XYZ, without
claiming that the frames physically coincide. This has no known strict
position-error upper bound. The result carries assumption ID
`rinex:NavIC-I02:unknown-WGS84-to-nominal-ITRF2014:zero-v1`, an
`UnboundedApproximate` method, `CAUTION`, no target-frame velocity, and
structured assumption metadata. No concrete source `FrameId` or frame
edge is invented. `warnings_as_errors=true` rejects it with
`WarningRejected`; `NumericalOnly`, a position-error bound, and a target
velocity request also reject it.

## Input and scope

The public RINEX 4.00 microfixture is
`tests/fixtures/nav_navic_i02_2023071.rnx`, SHA-256
`ae632c2debb026be9acaf207549a903bda9d89466c0ed1535c0e7cb2b2309ecd`.
Source, license, original record, field checks, and the independent
Python orbit calculation are in [NAV_NAVIC_LNAV.md](NAV_NAVIC_LNAV.md).
The catalogue exception requires I02 LNAV, record/ToE/ToC at
2023-03-12 00:00:00 in the library's GPST proxy, IODEC 0, and an
evaluation instant within the existing 7200 s selection window.
These record fields limit the current library branch; they are not a
cryptographic identity check of a runtime NAV file. The SHA-256 above is
the **reference fixture** hash, so users auditing another input should
record that file's actual hash separately. Caller-asserted NavIC points,
other messages and dates, and other target frames do not gain this path.

IRNSST and GPST share the week origin but are independently realized.
The parser's GPST representation of IRNSST is an explicit time proxy;
the physical offset is not measured or corrected here. That assumption
is separate from the spatial zero-offset assumption.

The independent [Python script](nav_navic_lnav.py) produces
[three native reference rows](nav_navic_lnav_expected.json) at ToE ±1800 s
and ToE. At ToE its XYZ is
`[20972.353636599695, 34616.32508750093, -12067.52585104587] km`.
The nominal target has the same numerical XYZ by the stated operation.
Agreement checks parsing, propagation, and copying; it does not establish
the true NavIC-to-ITRF2014 frame offset or physical accuracy.

## Reproduce from the rinex root

```sh
shasum -a 256 tests/fixtures/nav_navic_i02_2023071.rnx
python3 tests/reference/nav_navic_lnav.py > /private/tmp/nav_navic_lnav_expected_check.json
diff -u tests/reference/nav_navic_lnav_expected.json /private/tmp/nav_navic_lnav_expected_check.json
cargo test --offline --locked --features nav,log --test nav_navic_nominal_frame -- --nocapture
cargo run --offline --locked --features nav --example nav_probe -- state tests/fixtures/nav_navic_i02_2023071.rnx I02 '2023-03-12T00:00:00 GPST' --target-frame ITRF2014 --details
cargo run --offline --locked --features nav --example nav_probe -- state tests/fixtures/nav_navic_i02_2023071.rnx I02 '2023-03-12T00:00:00 GPST' --target-frame ITRF2014 --warnings-as-errors
```

The default CLI should display `source_realization=Unknown`,
`position_status=NominalAssumption`, the assumption ID, and a `CAUTION`
line; the final command should exit with
`WarningRejected`. In VS Code, use rust-analyzer **Debug Test** on
`real_i02_default_gives_warned_nominal_position_with_unknown_source`
in `tests/nav_navic_nominal_frame.rs`. At `FrameTransformer::to_frame_inner`,
inspect `point.nominal_sample`, `point.realization`, `result.assumption`,
and the output XYZ. VS Code breakpoint behavior and physical accuracy
require separate user verification.
