//! N09d: real BeiDou-2 C10 D1 IGSO, dated BDCS2019v01 marked approximation.
#![cfg(feature = "nav")]

use rinex::{
    navigation::{
        rinex::{
            selection::{NativeFrame, UnknownHealthPolicy},
            spatial_state::{
                FrameError, FrameId, FrameMethod, FrameRealization, FrameRequest, FrameTransformer,
                MethodPolicy, SourceBasis, SourceFrameIdentity, SpatialPoint, TransformOptions,
            },
        },
        NavMessageType,
    },
    prelude::{Duration, Epoch, Rinex, SV},
};
use std::str::FromStr;

const FIXTURE: &str = "tests/fixtures/nav_legacy_kms_2022159.rnx";
const EXPECTED: &str = include_str!("reference/nav_bds_igso_frame_expected.json");

fn xyz(row: &serde_json::Value, field: &str) -> [f64; 3] {
    std::array::from_fn(|i| row[field][i].as_f64().unwrap())
}

fn assert_xyz(actual: [f64; 3], expected: [f64; 3], tolerance_km: f64) -> f64 {
    let mut maximum: f64 = 0.0;
    for axis in 0..3 {
        let difference = (actual[axis] - expected[axis]).abs();
        maximum = maximum.max(difference);
        assert!(
            difference < tolerance_km,
            "axis {axis}: actual={} expected={} difference={difference} km",
            actual[axis],
            expected[axis]
        );
    }
    maximum
}

fn toe() -> Epoch {
    Epoch::from_str("2022-06-08T07:00:00 BDT").unwrap()
}

#[test]
fn real_c10_d1_igso_reaches_marked_itrf2014_at_its_own_epoch() {
    let nav = Rinex::from_file(FIXTURE).unwrap();
    let sv = SV::from_str("C10").unwrap();
    let reference: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
    let mut maximum_km: f64 = 0.0;

    for row in reference.as_array().unwrap() {
        let epoch = toe() + Duration::from_seconds(row["offset_s"].as_f64().unwrap());
        let selected = nav.nav_select_ephemeris(sv, epoch, UnknownHealthPolicy::Reject);
        let native = selected.chosen().unwrap().spatial_state_at(epoch).unwrap();
        assert_eq!(native.key.sv, sv);
        assert_eq!(native.key.msgtype, NavMessageType::D1);
        assert_eq!(native.record_epoch, toe());
        assert_eq!(native.orbit_reference, Some(toe()));
        assert_eq!(
            native.state.native_frame(),
            NativeFrame::BeiDouBroadcastCgcs2000
        );
        assert_eq!(native.state.source(), SourceFrameIdentity::BeidouBroadcast);
        assert_eq!(
            native.state.realization(),
            FrameRealization::Known(FrameId::Bdcs2019v01)
        );
        assert!(native.state.source_evidence().unwrap().contains("2019v01"));
        maximum_km = maximum_km.max(assert_xyz(
            native.state.position_km,
            xyz(row, "native_position_km"),
            1e-6,
        ));

        let result = FrameTransformer
            .to_frame(
                &SpatialPoint::from_nav(&native.state).unwrap(),
                FrameRequest::Realization(FrameId::Itrf2014),
                TransformOptions::default(),
            )
            .unwrap();
        assert_eq!(result.epoch, epoch);
        assert_eq!(result.source, SourceFrameIdentity::BeidouBroadcast);
        assert_eq!(
            result.source_realization,
            FrameRealization::Known(FrameId::Bdcs2019v01)
        );
        assert_eq!(
            result.target,
            SourceFrameIdentity::Realization(FrameId::Itrf2014)
        );
        assert_eq!(result.source_basis, SourceBasis::NavMessageAndCatalogDate);
        assert_eq!(result.source_evidence, native.state.source_evidence());
        assert_eq!(result.method, FrameMethod::UnboundedApproximate);
        assert_eq!(
            result.edge_ids,
            &["rinex:BDCS2019v01-ITRF2014:zero-offset-approx"]
        );
        assert_eq!(result.info()[0].source, FrameId::Bdcs2019v01);
        assert_eq!(result.info()[0].target, FrameId::Itrf2014);
        assert!(result.position_accuracy_note.unwrap().contains("CAUTION"));
        assert_eq!(result.velocity_km_s, None);
        maximum_km = maximum_km.max(assert_xyz(
            result.position_km,
            xyz(row, "itrf2014_position_km"),
            1e-6,
        ));
    }
    println!("maximum XYZ difference from independent reference: {maximum_km:.3e} km");
}

#[test]
fn asserted_bdcs_point_uses_same_path_and_rejects_strict_requests() {
    let reference: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
    let row = &reference[1];
    let native_km = xyz(row, "native_position_km");
    let asserted = SpatialPoint::new(
        native_km,
        toe(),
        SourceFrameIdentity::Realization(FrameId::Bdcs2019v01),
        Some([0.1, 0.2, 0.3]),
    )
    .unwrap();
    let target = FrameRequest::Realization(FrameId::Itrf2014);
    let result = FrameTransformer
        .to_frame(&asserted, target, TransformOptions::default())
        .unwrap();
    assert_xyz(result.position_km, xyz(row, "itrf2014_position_km"), 1e-9);
    assert_eq!(result.source_basis, SourceBasis::CallerAsserted);
    assert_eq!(result.method, FrameMethod::UnboundedApproximate);
    assert_eq!(result.velocity_km_s, None);

    for (options, expected) in [
        (
            TransformOptions {
                method: MethodPolicy::NumericalOnly,
                ..Default::default()
            },
            FrameError::ApproximationExcluded,
        ),
        (
            TransformOptions {
                max_position_error_m: Some(100.0),
                ..Default::default()
            },
            FrameError::PositionBoundUnavailable,
        ),
        (
            TransformOptions {
                require_velocity: true,
                ..Default::default()
            },
            FrameError::VelocityUnavailable,
        ),
    ] {
        assert_eq!(
            FrameTransformer
                .to_frame(&asserted, target, options)
                .unwrap_err(),
            expected
        );
    }

    let unknown =
        SpatialPoint::new(native_km, toe(), SourceFrameIdentity::BeidouBroadcast, None).unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(&unknown, target, TransformOptions::default())
            .unwrap_err(),
        FrameError::UnknownSourceRealization
    );
    let outside = SpatialPoint::new(
        native_km,
        Epoch::from_str("2022-07-01T00:00:00 UTC").unwrap(),
        SourceFrameIdentity::Realization(FrameId::Bdcs2019v01),
        None,
    )
    .unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(&outside, target, TransformOptions::default())
            .unwrap_err(),
        FrameError::OutsideCatalogWindow
    );
}
