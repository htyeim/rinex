# N09d GAGAN S27 nominal ITRF2014 position

## Meaning and evidence

[RINEX 4.02](https://files.igs.org/pub/data/format/rinex_4.02.pdf) defines
the SBAS satellite number as PRN minus 100, so S27 means PRN127. The
[Airports Authority of India 2022 eAIP](https://aim-india.aai.aero/eAIP_Archive/19-05-2022/eAIP/IN-ENR%204.3-en-GB.html)
lists PRN127 as GAGAN GSAT-8 and says its navigation uses WGS84. It does
not identify the concrete WGS84 realization for this March 2023 broadcast
or a dated relation to ITRF2014. The evidence gap is detailed in
[nav_sbas_s27_frame_gap.md](nav_sbas_s27_frame_gap.md).

At the user's request, `BestAvailable` copies this record's native SBAS
ECEF XYZ to a **nominal** ITRF2014 position. The operation assumes an
unverified zero frame offset. Its result keeps `SbasBroadcast` and
`FrameRealization::Unknown`, and carries the versioned ID
`rinex:GAGAN-S27:unknown-WGS84-to-nominal-ITRF2014:zero-v1`,
`UnboundedApproximate`, `CAUTION`, and no target-frame velocity or strict
position-error bound. There is no invented known source `FrameId` or frame
edge. `--warnings-as-errors` rejects the result with `WarningRejected`;
`NumericalOnly`, a requested position-error bound, or a target velocity
also reject it. This numeric copy does not establish physical alignment.

## Exact scope and independent numbers

The public RINEX 4.00 [microfixture](../fixtures/nav_sbas_s27_2023071.rnx)
has SHA-256
`93c7b062ebb651d17c02cc4cebb023b4717c6b56cec6b6a24a63fdc8b718fbee`.
Its source, license, record mapping, and [independent Python model](nav_sbas_s27.py)
are documented in [NAV_SBAS.md](NAV_SBAS.md). The rule requires GAGAN
S27/PRN127, the SBAS EPH message or an identical legacy LNAV compatible
representation, record/orbit/clock reference at 2023-03-12 01:15:44 GPST,
and the fixture's clock parameters, `t_tm`, IODN, health, accuracy code,
and broadcast position/velocity/acceleration coefficients. Evaluation
must satisfy `|t−Toc| < 360 s`. These checks restrict the NAV record's
content; they do not authenticate a runtime file by hash. A user auditing
another input should record its actual hash separately. Other SBAS
services, another S27 record, a caller-asserted point, and other target
frames do not gain this path.

The independent [five reference rows](nav_sbas_s27_expected.json) cover
Toc ±300, ±120, and Toc. At `+120 s`, the native position is
`[24160.29789, 34538.69603, 30.32833] km`; the nominal target has the
same numerical XYZ by the stated operation. Tests check the native
polynomial against those rows and the exact copy separately. They cannot
bound the true GAGAN-to-ITRF2014 frame offset.

## Reproduce from the rinex root

```sh
shasum -a 256 tests/fixtures/nav_sbas_s27_2023071.rnx
python3 tests/reference/nav_sbas_s27.py > /private/tmp/nav_sbas_s27_expected_check.json
diff -u tests/reference/nav_sbas_s27_expected.json /private/tmp/nav_sbas_s27_expected_check.json
cargo test --offline --locked --features nav,log --test nav_sbas_nominal_frame -- --nocapture
cargo run --offline --locked --features nav,log --example nav_probe -- state tests/fixtures/nav_sbas_s27_2023071.rnx S27 '2023-03-12T01:17:44 GPST' --target-frame ITRF2014 --details
cargo run --offline --locked --features nav,log --example nav_probe -- state tests/fixtures/nav_sbas_s27_2023071.rnx S27 '2023-03-12T01:17:44 GPST' --target-frame ITRF2014 --warnings-as-errors
```

The default CLI should show `source_realization=Unknown`,
`position_status=NominalAssumption`, the assumption ID and `CAUTION`.
The final command should exit 1 with `WarningRejected`.
In VS Code, use rust-analyzer **Debug Test** on
`real_s27_five_epochs_give_warned_nominal_position` in
`tests/nav_sbas_nominal_frame.rs`. At
`NavCandidate::spatial_state_at`, inspect `nominal_sample`; at
`FrameTransformer::to_frame_inner`, inspect `point.nominal_sample`,
`point.realization`, `result.assumption` and `position_km`. VS Code
breakpoints, user execution, Rust 1.89 CI and physical frame accuracy
require separate verification.
