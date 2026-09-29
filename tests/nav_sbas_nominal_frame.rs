//! N09d: a warned nominal ITRF2014 position for one real GAGAN S27 record.
#![cfg(feature = "nav")]

use rinex::{
    navigation::{
        rinex::{
            selection::UnknownHealthPolicy,
            spatial_state::{
                FrameError, FrameId, FrameMethod, FrameRealization, FrameRequest, FrameTransformer,
                MethodPolicy, PositionStatus, SourceFrameIdentity, SpatialPoint, TransformOptions,
            },
        },
        NavMessageType, OrbitItem,
    },
    prelude::{Duration, Epoch, Rinex, SV},
};
use std::str::FromStr;

const FIXTURE: &str = "tests/fixtures/nav_sbas_s27_2023071.rnx";
const EXPECTED: &str = include_str!("reference/nav_sbas_s27_expected.json");

fn toc() -> Epoch {
    Epoch::from_str("2023-03-12T01:15:44 GPST").unwrap()
}

fn selected_point(nav: &Rinex, sv: SV, epoch: Epoch) -> SpatialPoint {
    let report = nav.nav_select_ephemeris(sv, epoch, UnknownHealthPolicy::Reject);
    let native = report.chosen().unwrap().spatial_state_at(epoch).unwrap();
    assert_eq!(native.state.realization(), FrameRealization::Unknown);
    SpatialPoint::from_nav(&native.state).unwrap()
}

fn target() -> FrameRequest {
    FrameRequest::Realization(FrameId::Itrf2014)
}

#[test]
fn real_s27_five_epochs_give_warned_nominal_position() {
    let nav = Rinex::from_file(FIXTURE).unwrap();
    let sv = SV::from_str("S27").unwrap();
    let expected: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
    for row in expected["rows"].as_array().unwrap() {
        let t = toc() + Duration::from_seconds(row["dt_s"].as_f64().unwrap());
        let report = nav.nav_select_ephemeris(sv, t, UnknownHealthPolicy::Reject);
        let native = report.chosen().unwrap().spatial_state_at(t).unwrap();
        let point = SpatialPoint::from_nav(&native.state).unwrap();
        let result = FrameTransformer
            .to_frame(&point, target(), TransformOptions::default())
            .unwrap();
        assert_eq!(result.epoch, t);
        assert_eq!(result.source, SourceFrameIdentity::SbasBroadcast);
        assert_eq!(result.source_realization, FrameRealization::Unknown);
        assert_eq!(
            result.target_realization,
            FrameRealization::Known(FrameId::Itrf2014)
        );
        assert_eq!(result.method, FrameMethod::UnboundedApproximate);
        assert_eq!(result.position_status(), PositionStatus::NominalAssumption);
        assert!(result.edge_ids.is_empty());
        assert!(result.info().is_empty());
        assert!(result.source_evidence.is_none());
        assert!(result.velocity_km_s.is_none());
        assert!(result.position_accuracy_note.unwrap().contains("CAUTION"));
        let assumption = result.assumption.unwrap();
        assert_eq!(
            assumption.id,
            "rinex:GAGAN-S27:unknown-WGS84-to-nominal-ITRF2014:zero-v1"
        );
        assert_eq!(
            assumption.reference_fixture_sha256,
            "93c7b062ebb651d17c02cc4cebb023b4717c6b56cec6b6a24a63fdc8b718fbee"
        );
        assert!(assumption.scope.contains("no runtime-file hash check"));
        for axis in 0..3 {
            let independent = row["position_km"][axis].as_f64().unwrap();
            assert!((point.position_km[axis] - independent).abs() < 1e-8);
            assert_eq!(result.position_km[axis], point.position_km[axis]);
        }
        assert_eq!(
            native.state.to_frame(target()).unwrap().position_km,
            result.position_km
        );
    }
}

#[test]
fn strict_and_unsupported_requests_reject_s27_assumption() {
    let nav = Rinex::from_file(FIXTURE).unwrap();
    let point = selected_point(&nav, SV::from_str("S27").unwrap(), toc());
    let strict = FrameTransformer
        .to_frame(
            &point,
            target(),
            TransformOptions {
                warnings_as_errors: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(strict, FrameError::WarningRejected(note) if note.contains("CAUTION")));
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
                max_position_error_m: Some(1_000.0),
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
                .to_frame(&point, target(), options)
                .unwrap_err(),
            expected
        );
    }
    for request in [
        FrameRequest::Wgs84,
        FrameRequest::Realization(FrameId::Itrf2020),
    ] {
        assert_eq!(
            FrameTransformer
                .to_frame(&point, request, TransformOptions::default())
                .unwrap_err(),
            FrameError::UnsupportedSource(SourceFrameIdentity::SbasBroadcast)
        );
    }
    let asserted = SpatialPoint::new(
        point.position_km,
        toc(),
        SourceFrameIdentity::SbasBroadcast,
        None,
    )
    .unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(&asserted, target(), TransformOptions::default())
            .unwrap_err(),
        FrameError::UnsupportedSource(SourceFrameIdentity::SbasBroadcast)
    );
    let mut outside = point;
    outside.epoch += Duration::from_seconds(360.0);
    assert_eq!(
        FrameTransformer
            .to_frame(&outside, target(), TransformOptions::default())
            .unwrap_err(),
        FrameError::OutsideCatalogWindow
    );
}

#[test]
fn altered_record_and_other_sbas_service_do_not_inherit_assumption() {
    for (field, changed) in [("iodn", 29.0), ("satPosX", 24160.61977)] {
        let mut nav = Rinex::from_file(FIXTURE).unwrap();
        let (_, frame) = nav.record.as_mut_nav().unwrap().iter_mut().next().unwrap();
        frame
            .as_mut_ephemeris()
            .unwrap()
            .orbits
            .insert(field.into(), OrbitItem::F64(changed));
        let point = selected_point(&nav, SV::from_str("S27").unwrap(), toc());
        assert_eq!(
            FrameTransformer
                .to_frame(&point, target(), TransformOptions::default())
                .unwrap_err(),
            FrameError::UnsupportedSource(SourceFrameIdentity::SbasBroadcast)
        );
    }

    // Synthetic re-keying probes the source-domain guard; it is not a real S26 NAV sample.
    let mut nav = Rinex::from_file(FIXTURE).unwrap();
    let record = nav.record.as_mut_nav().unwrap();
    let (mut key, frame) = record.pop_first().unwrap();
    key.sv = SV::from_str("S26").unwrap();
    record.insert(key, frame);
    let point = selected_point(&nav, key.sv, toc());
    assert_eq!(
        FrameTransformer
            .to_frame(&point, target(), TransformOptions::default())
            .unwrap_err(),
        FrameError::UnsupportedSource(SourceFrameIdentity::SbasBroadcast)
    );
}

#[test]
fn same_record_wrapped_as_legacy_lnav_uses_the_same_narrow_rule() {
    // Synthetic message rewrap checks the compatibility branch; the source fixture is EPH/SBAS.
    let mut nav = Rinex::from_file(FIXTURE).unwrap();
    let record = nav.record.as_mut_nav().unwrap();
    let (mut key, frame) = record.pop_first().unwrap();
    key.msgtype = NavMessageType::LNAV;
    record.insert(key, frame);
    let point = selected_point(&nav, key.sv, toc());
    let result = FrameTransformer
        .to_frame(&point, target(), TransformOptions::default())
        .unwrap();
    assert_eq!(
        result.assumption.unwrap().id,
        "rinex:GAGAN-S27:unknown-WGS84-to-nominal-ITRF2014:zero-v1"
    );
}
