# Dated frame targets and mixed-NAV regression

The offline catalogue is `rinex-nav-frame-catalog-v3`. All eight current `FrameId` values can be
requested through `FrameRequest::Realization` and through `nav_frame --target`.
`FrameRequest::Wgs84` remains a separate generic request.

The catalogue includes the inverse of its PZ-90.11 operation and four reverse
**marked approximation** edges for existing zero-offset alignments:

| Source → target | Library window | What is actually asserted |
| --- | --- | --- |
| ITRF2014 → QZSS JGS aligned to ITRF2014 | 2021-02-16 to 2023-11-08 UTC | Reverse of the existing zero-offset alignment approximation, based on [QZSS's coordinate history](https://qzss.go.jp/en/technical/dod/pnt/coordinate-system.html). |
| ITRF2020 → QZSS JGS aligned to ITRF2020 | 2023-11-11 to 2024-12-31 UTC | Reverse of the marked zero-offset approximation for the later version. |
| ITRF2020 → Galileo GTRF23v01 | 2024-05-01 to 2024-05-31 UTC | Reverse of the existing zero-offset alignment approximation, based on [ESA's GTRF23v01 description](https://navigation-office.esa.int/attachments/32835744/1/GTRF_IGS_Stop_6.pdf). |
| ITRF2014 → BeiDou BDCS2019v01 | 2022-06-01 to 2022-06-30 UTC | Reverse of the existing zero-offset alignment approximation for the dated C05/C10/C20 samples, using the [BDCS2019v01 alignment source](https://files.igs.org/pub/resource/pubs/workshop/2022/TourdelIGS4_05_Hu.pdf). |

The dates are conservative library windows, not asserted official end dates.
No reverse edge converts a source-family name into a known realization. The
QZSS/ESA/BDCS sources describe reference station alignment, not strict
satellite position bounds. The reverse zero offsets are therefore copies of
ECEF XYZ with `MarkedApproximation`, ordered edge IDs, source provenance,
per-edge `CAUTION`, no transformed velocity, and no strict frame-operation
error bound. A path containing several approximate edges retains every
edge's warning. Outside a dated path, a qualified NAV state may return a
`NominalAssumption` with its original `OutsideCatalogWindow` reason; this is
not physical target coverage.

The real 2024 mixed NAV and OBS first-epoch fixtures contain 56 observed SVs.
Selection with unknown health rejected finds 47 usable records and 9 rejected
records. Across all eight concrete targets the 47 records produce 376 results:

| Target | Native | Numerical | Marked | Nominal |
| --- | ---: | ---: | ---: | ---: |
| WGS84 G2296 | 11 | 0 | 17 | 19 |
| ITRF2020 | 0 | 11 | 17 | 19 |
| ITRF2014 | 0 | 11 | 17 | 19 |
| PZ-90.11 | 9 | 0 | 19 | 19 |
| QZSS JGS/ITRF2014 | 0 | 0 | 0 | 47 |
| QZSS JGS/ITRF2020 | 0 | 0 | 28 | 19 |
| Galileo GTRF23v01 | 8 | 0 | 20 | 19 |
| BeiDou BDCS2019v01 | 0 | 0 | 0 | 47 |

These are code classifications for the supplied fixture and catalogue date,
not position or scientific accuracy measures. The 19 unknown-source records
are 15 BeiDou, 3 NavIC, and 1 GAGAN S27; they retain unknown realizations.
The 9 rejected records do not gain coordinates. Real 2023 J02 QZSS and 2022
C05 BDCS fixtures exercise their separate historical date branches.

The regression fixtures and their extraction provenance are documented in
[`mixed_nav_first_epoch.md`](mixed_nav_first_epoch.md). In this fixture, J04
still fails NAV selection; target coverage counts only selected records.

From the repository root:

```sh
cargo test --offline --features nav --test nav_all_frame_targets --test nav_pz9011_inverse
cargo run --offline --features nav --example nav_frame -- tests/fixtures/nav_mixed_2024131_first_epoch.rnx E03 '2024-05-10T03:00:00 GPST' --target jgs2020
cargo run --offline --features nav --example nav_frame -- tests/fixtures/nav_mixed_2024131_first_epoch.rnx C02 '2024-05-10T03:00:00 GPST' --target bdcs2019v01
```

The first CLI request should use GTRF23v01 → ITRF2020 → JGS/ITRF2020 and
retain both approximate-edge cautions. C02 should retain unknown BDCS source
realization and return `NominalAssumption` with
`UnknownSourceRealization`, not claim BDCS2019v01 physical coordinates.
For a debugger entry, use the
`all_selected_mixed_records_reach_each_representable_target_with_correct_rank`
test. Inspect `chosen.health` at `NavCandidate::spatial_state_at`, `path` at
`find_catalog_path`, and `result.edge_info` and `result.fallback_reason` at
`FrameTransformer::to_frame_inner`. E03→JGS2020 has two approximate edges;
C02→BDCS2019v01 is a nominal diagnostic with unknown source. E03→JGS2014 is
nominal at the 2024 fixture epoch because that catalogue edge's window ended
in 2023.
