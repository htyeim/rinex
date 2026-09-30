//! N09d: an explicitly auditable nominal ITRF2014 position for one real I02 LNAV record.
#![cfg(feature = "nav")]

use rinex::{
    navigation::rinex::{
        selection::UnknownHealthPolicy,
        spatial_state::{
            FrameError, FrameId, FrameMethod, FrameRealization, FrameRequest, FrameTransformer,
            MethodPolicy, PositionStatus, SourceFrameIdentity, SpatialPoint, TransformOptions,
        },
    },
    prelude::{Duration, Epoch, Rinex, SV},
};
use std::str::FromStr;

const FIXTURE: &str = "tests/fixtures/nav_navic_i02_2023071.rnx";
const EXPECTED: &str = include_str!("reference/nav_navic_lnav_expected.json");
const NAVIC_I02_ASSUMPTION_ID: &str = "rinex:NavIC-I02:unknown-WGS84-to-nominal-ITRF2014:zero-v1";

fn toe() -> Epoch {
    Epoch::from_str("2023-03-12T00:00:00 GPST").unwrap()
}

fn selected_point(epoch: Epoch) -> SpatialPoint {
    let nav = Rinex::from_file(FIXTURE).unwrap();
    let sv = SV::from_str("I02").unwrap();
    let report = nav.nav_select_ephemeris(sv, epoch, UnknownHealthPolicy::Reject);
    let native = report.chosen().unwrap().spatial_state_at(epoch).unwrap();
    assert_eq!(native.state.realization(), FrameRealization::Unknown);
    SpatialPoint::from_nav(&native.state).unwrap()
}

#[test]
fn real_i02_default_gives_warned_nominal_position_with_unknown_source() {
    let rows: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
    for row in rows.as_array().unwrap() {
        let epoch = toe() + Duration::from_seconds(row["offset_s"].as_f64().unwrap());
        let point = selected_point(epoch);
        let result = FrameTransformer
            .to_frame(
                &point,
                FrameRequest::Realization(FrameId::Itrf2014),
                TransformOptions::default(),
            )
            .unwrap();
        assert_eq!(result.epoch, epoch);
        assert_eq!(result.source, SourceFrameIdentity::NavicBroadcastWgs84);
        assert_eq!(result.source_realization, FrameRealization::Unknown);
        assert_eq!(
            result.target_realization,
            FrameRealization::Known(FrameId::Itrf2014)
        );
        assert_eq!(result.method, FrameMethod::UnboundedApproximate);
        assert_eq!(result.position_status(), PositionStatus::NominalAssumption);
        assert!(result.edge_ids.is_empty());
        assert!(result.info().is_empty());
        assert_eq!(result.velocity_km_s, None);
        assert!(result.source_evidence.is_none());
        let assumption = result.assumption.unwrap();
        assert_eq!(
            assumption.id,
            "rinex:NavIC-I02:unknown-WGS84-to-nominal-ITRF2014:zero-v1"
        );
        assert_eq!(
            assumption.reference_fixture_sha256,
            "ae632c2debb026be9acaf207549a903bda9d89466c0ed1535c0e7cb2b2309ecd"
        );
        assert!(assumption.time_note.contains("GPST proxy"));
        assert!(result.position_accuracy_note.unwrap().contains("CAUTION"));
        for axis in 0..3 {
            let expected = row["position_km"][axis].as_f64().unwrap();
            assert!((point.position_km[axis] - expected).abs() < 1e-6);
            assert_eq!(result.position_km[axis], point.position_km[axis]);
        }
    }
}

#[test]
fn strict_and_unsupported_requests_fail_closed() {
    let point = selected_point(toe());
    let request = FrameRequest::Realization(FrameId::Itrf2014);
    let strict_error = FrameTransformer
        .to_frame(
            &point,
            request,
            TransformOptions {
                warnings_as_errors: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(strict_error, FrameError::WarningRejected(note) if note.contains("CAUTION")));
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
                max_frame_operation_error_m: Some(1_000.0),
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
                .to_frame(&point, request, options)
                .unwrap_err(),
            expected
        );
    }

    let asserted = SpatialPoint::new(
        point.position_km,
        toe(),
        SourceFrameIdentity::NavicBroadcastWgs84,
        None,
    )
    .unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(&asserted, request, TransformOptions::default())
            .unwrap_err(),
        FrameError::UnsupportedSource(SourceFrameIdentity::NavicBroadcastWgs84)
    );
    let generic = FrameTransformer
        .to_frame(&point, FrameRequest::Wgs84, TransformOptions::default())
        .unwrap();
    assert_eq!(
        generic.fallback_reason,
        Some(FrameError::UnsupportedSource(
            SourceFrameIdentity::NavicBroadcastWgs84
        ))
    );
    assert_ne!(generic.assumption.unwrap().id, NAVIC_I02_ASSUMPTION_ID);
    let mut outside = point;
    outside.epoch += Duration::from_seconds(7200.0);
    assert_eq!(
        FrameTransformer
            .to_frame(&outside, request, TransformOptions::default())
            .unwrap_err(),
        FrameError::InconsistentFrame
    );
}

#[test]
fn warning_audit_allows_clean_native_result_and_rejects_other_warned_paths() {
    let strict = TransformOptions {
        warnings_as_errors: true,
        ..Default::default()
    };
    let native = SpatialPoint::new(
        [10_000.0, 20_000.0, 30_000.0],
        toe(),
        SourceFrameIdentity::Realization(FrameId::Itrf2014),
        None,
    )
    .unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(
                &native,
                FrameRequest::Realization(FrameId::Itrf2014),
                strict
            )
            .unwrap()
            .method,
        FrameMethod::Native
    );
    let beidou = SpatialPoint::new(
        native.position_km,
        Epoch::from_str("2022-06-08T09:00:00 BDT").unwrap(),
        SourceFrameIdentity::Realization(FrameId::Bdcs2019v01),
        None,
    )
    .unwrap();
    assert!(matches!(
        FrameTransformer.to_frame(&beidou, FrameRequest::Realization(FrameId::Itrf2014), strict),
        Err(FrameError::WarningRejected(note)) if note.contains("CAUTION")
    ));
}

#[test]
fn other_real_navic_record_does_not_inherit_i02_assumption() {
    let nav = Rinex::from_file("data/NAV/V4/rinex402_examples_MN.rnx").unwrap();
    let sv = SV::from_str("I02").unwrap();
    let (key, _) = nav
        .nav_ephemeris_frames_iter()
        .find(|(key, _)| key.sv == sv)
        .unwrap();
    let report = nav.nav_select_ephemeris(sv, key.epoch, UnknownHealthPolicy::Reject);
    let state = report
        .chosen()
        .unwrap()
        .spatial_state_at(key.epoch)
        .unwrap();
    assert_eq!(state.state.realization(), FrameRealization::Unknown);
    let result = state
        .state
        .to_frame(FrameRequest::Realization(FrameId::Itrf2014))
        .unwrap();
    assert_ne!(result.assumption.unwrap().id, NAVIC_I02_ASSUMPTION_ID);
    assert_eq!(
        result.fallback_reason,
        Some(FrameError::UnsupportedSource(
            SourceFrameIdentity::NavicBroadcastWgs84
        ))
    );
}

#[test]
fn altered_i02_orbit_does_not_inherit_nominal_assumption() {
    let mut nav = Rinex::from_file("tests/fixtures/nav_navic_i02_2023071.rnx").unwrap();
    let (_, frame) = nav.record.as_mut_nav().unwrap().iter_mut().next().unwrap();
    frame
        .as_mut_ephemeris()
        .unwrap()
        .orbits
        .insert("sqrta".into(), rinex::navigation::OrbitItem::F64(6493.36));
    let report = nav.nav_select_ephemeris(
        SV::from_str("I02").unwrap(),
        toe(),
        UnknownHealthPolicy::Reject,
    );
    let native = report.chosen().unwrap().spatial_state_at(toe()).unwrap();
    let result = native
        .state
        .to_frame(FrameRequest::Realization(FrameId::Itrf2014))
        .unwrap();
    assert_ne!(result.assumption.unwrap().id, NAVIC_I02_ASSUMPTION_ID);
    assert_eq!(
        result.fallback_reason,
        Some(FrameError::UnsupportedSource(
            SourceFrameIdentity::NavicBroadcastWgs84
        ))
    );
}
