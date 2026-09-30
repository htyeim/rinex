//! F4 first unit: selected real mixed NAV records and explicit diagnostic boundaries.
#![cfg(feature = "nav")]
use rinex::{
    navigation::rinex::{
        selection::{NavRejection, UnknownHealthPolicy},
        spatial_state::{
            FrameError, FrameId, FrameRealization, FrameRequest, FrameTransformer, MethodPolicy,
            PositionStatus, SourceBasis, SourceFrameIdentity, SpatialPoint, TransformOptions,
        },
    },
    navigation::OrbitItem,
    prelude::{Epoch, Rinex, SV},
};
use std::{collections::BTreeSet, str::FromStr};

const NAV: &str = "tests/fixtures/nav_mixed_2024131_first_epoch.rnx";
const OBS: &str = "tests/fixtures/obs_mixed_2024131_first_epoch.rnx";

fn instant() -> Epoch {
    Epoch::from_str("2024-05-10T03:00:00 GPST").unwrap()
}

#[test]
fn selected_real_records_reach_all_three_core_targets_without_upgrading_unknown_sources() {
    let nav = Rinex::from_file(NAV).unwrap();
    let obs = Rinex::from_file(OBS).unwrap();
    let (_, observations) = obs.observations_iter().next().unwrap();
    let svs: BTreeSet<SV> = observations
        .signals
        .iter()
        .map(|signal| signal.sv)
        .collect();
    assert_eq!(svs.len(), 56);
    let mut selected = 0;
    let mut nominal = 0;
    let mut rejected = 0;
    for sv in svs {
        let report = nav.nav_select_ephemeris(sv, instant(), UnknownHealthPolicy::Reject);
        let Some(chosen) = report.chosen() else {
            rejected += 1;
            continue;
        };
        selected += 1;
        let native = chosen.spatial_state_at(instant()).unwrap();
        let point = SpatialPoint::from_nav(&native.state).unwrap();
        for target in [
            FrameRequest::Realization(FrameId::Wgs84G2296),
            FrameRequest::Realization(FrameId::Itrf2020),
            FrameRequest::Realization(FrameId::Itrf2014),
        ] {
            let result = FrameTransformer
                .to_frame(&point, target, TransformOptions::default())
                .unwrap();
            assert!(
                result
                    .position_km
                    .iter()
                    .all(|coordinate| coordinate.is_finite()),
                "{sv}"
            );
            assert_eq!(
                result.source_realization,
                native.state.realization(),
                "{sv}"
            );
            if result.position_status() == PositionStatus::NominalAssumption {
                nominal += 1;
                assert_eq!(result.position_km, native.state.position_km, "{sv}");
                assert!(result.fallback_reason.is_some(), "{sv}");
                assert!(result.assumption.is_some(), "{sv}");
                assert!(
                    result
                        .cautions()
                        .iter()
                        .any(|caution| caution.contains("physical")),
                    "{sv}"
                );
            } else {
                assert!(result.fallback_reason.is_none(), "{sv}");
            }
        }
    }
    assert_eq!((selected, rejected, nominal), (47, 9, 57));
}

#[test]
fn strict_options_and_caller_asserted_points_have_distinct_gates() {
    let nav = Rinex::from_file(NAV).unwrap();
    let c02 = nav.nav_select_ephemeris(
        SV::from_str("C02").unwrap(),
        instant(),
        UnknownHealthPolicy::Reject,
    );
    let native = c02.chosen().unwrap().spatial_state_at(instant()).unwrap();
    let point = SpatialPoint::from_nav(&native.state).unwrap();
    let target = FrameRequest::Realization(FrameId::Itrf2020);
    let nominal = FrameTransformer
        .to_frame(&point, target, TransformOptions::default())
        .unwrap();
    assert_eq!(
        nominal.fallback_reason,
        Some(FrameError::UnknownSourceRealization)
    );
    for (options, error) in [
        (
            TransformOptions {
                method: MethodPolicy::NumericalOnly,
                ..Default::default()
            },
            FrameError::ApproximationExcluded,
        ),
        (
            TransformOptions {
                max_frame_operation_error_m: Some(1.0),
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
                .to_frame(&point, target, options)
                .unwrap_err(),
            error
        );
    }
    assert!(matches!(
        FrameTransformer
            .to_frame(
                &point,
                target,
                TransformOptions {
                    warnings_as_errors: true,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        FrameError::WarningRejected(_)
    ));
    let asserted = SpatialPoint::new(
        point.position_km,
        instant(),
        SourceFrameIdentity::BeidouBroadcast,
        None,
    )
    .unwrap();
    assert_eq!(
        FrameTransformer
            .to_frame(&asserted, target, TransformOptions::default())
            .unwrap_err(),
        FrameError::UnknownSourceRealization
    );
    let opted = FrameTransformer
        .to_frame(
            &asserted,
            target,
            TransformOptions {
                allow_unverified_nominal: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(opted.position_status(), PositionStatus::NominalAssumption);
    assert_eq!(opted.source_basis, SourceBasis::CallerAsserted);
    assert_eq!(opted.source_realization, FrameRealization::Unknown);
    assert!(opted
        .cautions()
        .iter()
        .any(|note| note.contains("NAV health")));
    let evidenced = SpatialPoint::new(
        [10000.0, 20000.0, 30000.0],
        instant(),
        SourceFrameIdentity::Realization(FrameId::Itrf2020),
        None,
    )
    .unwrap();
    let evidenced_result = FrameTransformer
        .to_frame(
            &evidenced,
            FrameRequest::Realization(FrameId::Itrf2014),
            TransformOptions::default(),
        )
        .unwrap();
    assert_eq!(
        evidenced_result.position_status(),
        PositionStatus::NumericalTransform
    );
    assert!(evidenced_result
        .cautions()
        .iter()
        .any(|note| note.contains("NAV health")));
    assert_eq!(
        FrameTransformer
            .to_frame(
                &evidenced,
                FrameRequest::Realization(FrameId::Itrf2020),
                TransformOptions {
                    max_frame_operation_error_m: Some(0.0),
                    ..Default::default()
                }
            )
            .unwrap()
            .position_status(),
        PositionStatus::NativeIdentity
    );
    assert_eq!(
        FrameTransformer
            .to_frame(
                &evidenced,
                FrameRequest::Realization(FrameId::Itrf2014),
                TransformOptions {
                    max_frame_operation_error_m: Some(1.0),
                    ..Default::default()
                }
            )
            .unwrap_err(),
        FrameError::PositionBoundUnavailable
    );
}

#[test]
fn unavailable_catalog_relation_preserves_known_source_and_reason() {
    let point = SpatialPoint::new(
        [10000.0, 20000.0, 30000.0],
        instant(),
        SourceFrameIdentity::Realization(FrameId::Bdcs2019v01),
        None,
    )
    .unwrap();
    let target = FrameRequest::Realization(FrameId::Itrf2020);
    assert_eq!(
        FrameTransformer
            .to_frame(&point, target, TransformOptions::default())
            .unwrap_err(),
        FrameError::OutsideCatalogWindow
    );
    let result = FrameTransformer
        .to_frame(
            &point,
            target,
            TransformOptions {
                allow_unverified_nominal: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(result.position_status(), PositionStatus::NominalAssumption);
    assert_eq!(
        result.source_realization,
        FrameRealization::Known(FrameId::Bdcs2019v01)
    );
    assert_eq!(
        result.fallback_reason,
        Some(FrameError::OutsideCatalogWindow)
    );

    let nav = Rinex::from_file(NAV).unwrap();
    let e03 = nav.nav_select_ephemeris(
        SV::from_str("E03").unwrap(),
        instant(),
        UnknownHealthPolicy::Reject,
    );
    let native = e03.chosen().unwrap().spatial_state_at(instant()).unwrap();
    let point = SpatialPoint::from_nav(&native.state).unwrap();
    let result = FrameTransformer
        .to_frame(
            &point,
            FrameRequest::Realization(FrameId::Pz90_11),
            TransformOptions::default(),
        )
        .unwrap();
    assert_eq!(result.position_status(), PositionStatus::NominalAssumption);
    assert_eq!(
        result.source_realization,
        FrameRealization::Known(FrameId::GalileoGtrf23v01)
    );
    assert_eq!(result.fallback_reason, Some(FrameError::NoPath));
    assert_eq!(result.position_km, native.state.position_km);
}

#[test]
fn unknown_health_requires_explicit_allow_and_invalid_data_still_rejects() {
    let mut nav = Rinex::from_file(NAV).unwrap();
    let sv = SV::from_str("G04").unwrap();
    for (key, frame) in nav.record.as_mut_nav().unwrap().iter_mut() {
        if key.sv == sv {
            frame.as_mut_ephemeris().unwrap().orbits.remove("health");
        }
    }
    let rejected = nav.nav_select_ephemeris(sv, instant(), UnknownHealthPolicy::Reject);
    assert!(rejected.chosen().is_none());
    assert!(rejected
        .candidates
        .iter()
        .any(|candidate| candidate.rejection == Some(NavRejection::UnknownHealth)));
    let allowed = nav.nav_select_ephemeris(sv, instant(), UnknownHealthPolicy::Allow);
    let chosen = allowed.chosen().unwrap();
    assert_eq!(chosen.health, None);
    assert!(chosen.spatial_state_at(instant()).is_ok());

    let mut invalid = nav.clone();
    for (key, frame) in invalid.record.as_mut_nav().unwrap().iter_mut() {
        if key.sv == sv {
            frame
                .as_mut_ephemeris()
                .unwrap()
                .orbits
                .insert("dataValidity".into(), OrbitItem::F64(1.0));
        }
    }
    let report = invalid.nav_select_ephemeris(sv, instant(), UnknownHealthPolicy::Allow);
    assert!(report.chosen().is_none());
    assert!(report
        .candidates
        .iter()
        .any(|candidate| candidate.rejection == Some(NavRejection::InvalidData)));
}
