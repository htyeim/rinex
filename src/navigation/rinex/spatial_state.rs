//! Message-aware native NAV state and a dated terrestrial-frame conversion.
//!
//! A frame family name alone is not a frame realization: GPS and NavIC
//! broadcasts can both carry a WGS-84 family label.
use super::{
    glonass_fdma::FdmaError,
    selection::{NativeFrame, NavCandidate, StateError},
};
use crate::{
    navigation::{NavKey, NavMessageType},
    prelude::{Constellation, Epoch},
};

/// The broadcasting system that defines the native terrestrial axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFrameIdentity {
    GpsBroadcastWgs84,
    GlonassBroadcastPz90,
    NavicBroadcastWgs84,
    /// A concrete terrestrial realization asserted by a non-NAV caller.
    Realization(FrameId),
}

/// Concrete Earth-fixed realizations supported by the fixed GPS catalogue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameId {
    Wgs84G2296,
    Pz90_11,
    Itrf2020,
    Itrf2014,
}

/// RINEX NAV does not identify a particular terrestrial-frame realization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRealization {
    Unknown,
    Known(FrameId),
}

/// Minimal state needed for a later frame conversion, independent of the NAV file.
#[derive(Clone, Copy, Debug)]
pub struct BroadcastFixedState {
    pub epoch: Epoch,
    native_frame: NativeFrame,
    source: SourceFrameIdentity,
    realization: FrameRealization,
    source_evidence: Option<&'static str>,
    pub position_km: [f64; 3],
    pub velocity_km_s: [f64; 3],
}

/// Owned message identity and reference times accompany the native state.
#[derive(Clone, Copy, Debug)]
pub struct NavSpatialState {
    pub key: NavKey,
    pub orbit_reference: Option<Epoch>,
    pub clock_reference: Epoch,
    pub record_epoch: Epoch,
    pub state: BroadcastFixedState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialStateError {
    Fdma(FdmaError),
    Gps(StateError),
}

impl std::fmt::Display for SpatialStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SpatialStateError {}

impl NavCandidate<'_> {
    /// Reuse the selected GPS LNAV or GLONASS FDMA propagator; do not reselect it.
    pub fn spatial_state_at(&self, t: Epoch) -> Result<NavSpatialState, SpatialStateError> {
        let (native_frame, source, realization, source_evidence, position_km, velocity_km_s) =
            match (self.key.sv.constellation, self.key.msgtype) {
                (Constellation::GPS, NavMessageType::LNAV) => {
                    let native = self.native_state_at(t).map_err(SpatialStateError::Gps)?;
                    let g2296 = self.orbit_reference.is_some_and(in_g2296_window)
                        && in_g2296_window(self.key.epoch)
                        && in_g2296_window(t);
                    (
                        native.frame,
                        SourceFrameIdentity::GpsBroadcastWgs84,
                        if g2296 {
                            FrameRealization::Known(FrameId::Wgs84G2296)
                        } else {
                            FrameRealization::Unknown
                        },
                        g2296.then_some(G2296_SOURCE_EVIDENCE),
                        native.position_km,
                        native.velocity_km_s,
                    )
                },
                (Constellation::Glonass, NavMessageType::FDMA | NavMessageType::LNAV) => {
                    if let Some(reason) = self.rejection {
                        return Err(SpatialStateError::Fdma(FdmaError::Rejected(reason)));
                    }
                    let native = self.fdma_state_at(t).map_err(SpatialStateError::Fdma)?;
                    let pz9011 = self.orbit_reference.is_some_and(in_pz9011_window)
                        && in_pz9011_window(self.key.epoch)
                        && in_pz9011_window(t);
                    (
                        native.frame,
                        SourceFrameIdentity::GlonassBroadcastPz90,
                        if pz9011 {
                            FrameRealization::Known(FrameId::Pz90_11)
                        } else {
                            FrameRealization::Unknown
                        },
                        pz9011.then_some(PZ9011_SOURCE_EVIDENCE),
                        native.position_km,
                        native.velocity_km_s,
                    )
                },
                _ => return Err(SpatialStateError::Gps(StateError::UnsupportedMessage)),
            };
        Ok(NavSpatialState {
            key: *self.key,
            orbit_reference: self.orbit_reference,
            clock_reference: self.clock_reference,
            record_epoch: self.key.epoch,
            state: BroadcastFixedState {
                epoch: t,
                native_frame,
                source,
                realization,
                source_evidence,
                position_km,
                velocity_km_s,
            },
        })
    }
}

/// Request GPS broadcast WGS-84 at the coordinate epoch or a concrete realization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRequest {
    Wgs84,
    Realization(FrameId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameMethod {
    Native,
    Helmert,
    Composite,
    /// Published parameters held fixed beyond their reference epoch; no validated error bound.
    UnboundedApproximate,
}

/// A target state. Unknown realization remains explicit rather than inventing Gxxxx.
#[derive(Clone, Copy, Debug)]
pub struct FrameResult {
    pub epoch: Epoch,
    pub source: SourceFrameIdentity,
    pub target: SourceFrameIdentity,
    pub target_realization: FrameRealization,
    pub source_realization: FrameRealization,
    pub position_km: [f64; 3],
    pub velocity_km_s: Option<[f64; 3]>,
    pub method: FrameMethod,
    /// Frozen parameter catalogue; no runtime download or kernel is used.
    pub catalog_version: &'static str,
    /// Ordered parameter/operation IDs that produced the returned position.
    pub edge_ids: &'static [&'static str],
    /// Ordered per-edge direction, method, provenance, and applicability.
    pub edge_info: &'static [FrameEdgeInfo],
    pub source_basis: SourceBasis,
    /// Operational evidence used to resolve a NAV source realization.
    /// None for caller-asserted points or unresolved NAV sources.
    pub source_evidence: Option<&'static str>,
    /// A published operation accuracy is not a strict position upper bound.
    pub position_accuracy_note: Option<&'static str>,
    pub velocity_note: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameEdgeInfo {
    pub id: &'static str,
    pub source: FrameId,
    pub target: FrameId,
    pub method: FrameMethod,
    pub parameter_reference_epoch: &'static str,
    pub valid_window: &'static str,
    pub source_url: &'static str,
    pub position_metric: &'static str,
    pub velocity_capability: &'static str,
}

impl FrameResult {
    /// Inspect every chosen edge without re-running the transformation.
    pub fn info(&self) -> &'static [FrameEdgeInfo] {
        self.edge_info
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBasis {
    NavMessageAndCatalogDate,
    CallerAsserted,
}

#[derive(Clone, Copy, Debug)]
pub struct TransformOptions {
    pub max_position_error_m: Option<f64>,
    pub require_velocity: bool,
    pub numerical_only: bool,
    pub warnings_as_errors: bool,
}

impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            max_position_error_m: None,
            require_velocity: false,
            numerical_only: false,
            warnings_as_errors: false,
        }
    }
}

/// Earth-fixed XYZ and coordinate epoch. A caller-provided source is an assertion.
/// Units are km and km/s; no NavKey, file path, or receiver metadata is needed.
#[derive(Clone, Copy, Debug)]
pub struct SpatialPoint {
    pub epoch: Epoch,
    pub source: SourceFrameIdentity,
    pub position_km: [f64; 3],
    pub velocity_km_s: Option<[f64; 3]>,
    source_basis: SourceBasis,
    realization: FrameRealization,
    source_evidence: Option<&'static str>,
}

impl SpatialPoint {
    pub fn new(
        position_km: [f64; 3],
        epoch: Epoch,
        source: SourceFrameIdentity,
        velocity_km_s: Option<[f64; 3]>,
    ) -> Result<Self, FrameError> {
        Self::from_parts(position_km, Some(epoch), Some(source), velocity_km_s)
    }

    /// Also allows ingestion boundaries to report missing metadata explicitly.
    pub fn from_parts(
        position_km: [f64; 3],
        epoch: Option<Epoch>,
        source: Option<SourceFrameIdentity>,
        velocity_km_s: Option<[f64; 3]>,
    ) -> Result<Self, FrameError> {
        let epoch = epoch.ok_or(FrameError::MissingEpoch)?;
        let source = source.ok_or(FrameError::MissingSource)?;
        if !position_km
            .iter()
            .chain(velocity_km_s.iter().flatten())
            .all(|v| v.is_finite())
        {
            return Err(FrameError::NonFiniteState);
        }
        Ok(Self {
            epoch,
            source,
            position_km,
            velocity_km_s,
            source_basis: SourceBasis::CallerAsserted,
            realization: FrameRealization::Unknown,
            source_evidence: None,
        })
    }

    /// Adapt an already propagated NAV state without selecting or propagating again.
    pub fn from_nav(state: &BroadcastFixedState) -> Result<Self, FrameError> {
        let mut point = Self::new(
            state.position_km,
            state.epoch,
            state.source,
            Some(state.velocity_km_s),
        )?;
        point.source_basis = SourceBasis::NavMessageAndCatalogDate;
        point.realization = state.realization;
        point.source_evidence = state.source_evidence;
        Ok(point)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    MissingSource,
    MissingEpoch,
    UnsupportedSource(SourceFrameIdentity),
    UnknownSourceRealization,
    OutsideCatalogWindow,
    NoPath,
    PositionBoundUnavailable,
    InvalidPositionBound,
    VelocityUnavailable,
    ApproximationExcluded,
    WarningRejected,
    InconsistentFrame,
    NonFiniteState,
}
impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FrameError {}

impl BroadcastFixedState {
    pub fn native_frame(&self) -> NativeFrame {
        self.native_frame
    }

    pub fn source(&self) -> SourceFrameIdentity {
        self.source
    }

    pub fn realization(&self) -> FrameRealization {
        self.realization
    }

    pub fn source_evidence(&self) -> Option<&'static str> {
        self.source_evidence
    }

    /// Use the same fixed frame converter as generic spatial points.
    /// This convenience call keeps the native state and its NavKey untouched.
    pub fn to_frame(&self, target: FrameRequest) -> Result<FrameResult, FrameError> {
        if (self.source == SourceFrameIdentity::GpsBroadcastWgs84
            && self.native_frame != NativeFrame::GpsBroadcastWgs84)
            || (self.source == SourceFrameIdentity::GlonassBroadcastPz90
                && self.native_frame != NativeFrame::GlonassBroadcastPz90)
        {
            return Err(FrameError::InconsistentFrame);
        }
        FrameTransformer.to_frame(
            &SpatialPoint::from_nav(self)?,
            target,
            TransformOptions::default(),
        )
    }
}

const CATALOG_VERSION: &str = "GPS-G2296-PZ9011-ITRF2020-ITRF2014-v2";
const G2296_SOURCE_EVIDENCE: &str = "https://www.navcen.uscg.gov/gps-constellation (NANU 2024014)";
const PZ9011_SOURCE_EVIDENCE: &str = "https://www.unoosa.org/documents/pdf/icg/2023/ICG-17/icg17_wgd_02_03.pdf; https://www.unoosa.org/pdf/icg/2016/icg11/wgd/13wgd.pdf";
const PZ9011_TO_ITRF2014: &str = "ICG:2018:PZ90.11-to-ITRF2014:static-2010-approx";
const G2296_TO_ITRF2020: &str = "EPSG:10608";
const ITRF2020_TO_ITRF2014: &str = "ITRF2020:Table2:2015.0";
const NATIVE_EDGES: &[&str] = &[];
const G2296_EDGE: &[&str] = &[G2296_TO_ITRF2020];
const G2296_INVERSE_EDGE: &[&str] = &["EPSG:10608:inverse"];
const ITRF_EDGE: &[&str] = &[ITRF2020_TO_ITRF2014];
const ITRF_INVERSE_EDGE: &[&str] = &["ITRF2020:Table2:2015.0:inverse"];
const G2296_THEN_ITRF: &[&str] = &[G2296_TO_ITRF2020, ITRF2020_TO_ITRF2014];
const ITRF_THEN_G2296: &[&str] = &["ITRF2020:Table2:2015.0:inverse", "EPSG:10608:inverse"];
const PZ9011_EDGE: &[&str] = &[PZ9011_TO_ITRF2014];
const G2296_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: G2296_TO_ITRF2020,
    source: FrameId::Wgs84G2296,
    target: FrameId::Itrf2020,
    method: FrameMethod::Helmert,
    parameter_reference_epoch: "2024.0",
    valid_window: "2024-03-04 through 2024-12-31 UTC (catalogue restriction)",
    source_url: "https://epsg.io/10608",
    position_metric: "EPSG operation accuracy 0.01 m at 2024.0; not a strict satellite upper bound",
    velocity_capability: "unvalidated",
};
const G2296_INVERSE_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: "EPSG:10608:inverse",
    source: FrameId::Itrf2020,
    target: FrameId::Wgs84G2296,
    ..G2296_INFO
};
const ITRF_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: ITRF2020_TO_ITRF2014,
    source: FrameId::Itrf2020,
    target: FrameId::Itrf2014,
    method: FrameMethod::Helmert,
    parameter_reference_epoch: "2015.0",
    valid_window: "2015-01-01 through 2026-12-31 UTC (catalogue restriction)",
    source_url: "https://itrf.ign.fr/en/solutions/itrf2020",
    position_metric: "published parameter uncertainties; no strict satellite upper bound",
    velocity_capability: "rates published; target velocity unvalidated",
};
const ITRF_INVERSE_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: "ITRF2020:Table2:2015.0:inverse",
    source: FrameId::Itrf2014,
    target: FrameId::Itrf2020,
    ..ITRF_INFO
};
const PZ9011_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: PZ9011_TO_ITRF2014,
    source: FrameId::Pz90_11,
    target: FrameId::Itrf2014,
    method: FrameMethod::UnboundedApproximate,
    parameter_reference_epoch: "2010.0; parameters frozen at later coordinate epochs",
    valid_window: "2014-01-15 through 2024-12-31 UTC (conservative library policy)",
    source_url:
        "https://www.unoosa.org/documents/pdf/icg/2019/resources/PZ-90.11_v.1.2_04.11.2018.pdf",
    position_metric: "2010.0 ground-station fit; no strict satellite or later-epoch error bound",
    velocity_capability: "not established",
};
const NATIVE_INFO: &[FrameEdgeInfo] = &[];
const G2296_INFO_PATH: &[FrameEdgeInfo] = &[G2296_INFO];
const G2296_INVERSE_INFO_PATH: &[FrameEdgeInfo] = &[G2296_INVERSE_INFO];
const ITRF_INFO_PATH: &[FrameEdgeInfo] = &[ITRF_INFO];
const ITRF_INVERSE_INFO_PATH: &[FrameEdgeInfo] = &[ITRF_INVERSE_INFO];
const G2296_THEN_ITRF_INFO: &[FrameEdgeInfo] = &[G2296_INFO, ITRF_INFO];
const ITRF_THEN_G2296_INFO: &[FrameEdgeInfo] = &[ITRF_INVERSE_INFO, G2296_INVERSE_INFO];
const PZ9011_INFO_PATH: &[FrameEdgeInfo] = &[PZ9011_INFO];

/// Fixed, offline, narrowly dated terrestrial-frame parameter catalogue.
/// It uses no ANISE frame or kernel: these GNSS realizations are not ANISE frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTransformer;

impl FrameTransformer {
    pub const fn catalog_version(&self) -> &'static str {
        CATALOG_VERSION
    }

    /// Convert a generic Earth-fixed point. The coordinate epoch is preserved.
    /// G2296 is resolved only for 2024-03-04 through 2024-12-31, after the
    /// operational GPS update completed. Cross-frame velocity is withheld until
    /// the full chain has an independently checked velocity reference.
    pub fn to_frame(
        &self,
        point: &SpatialPoint,
        request: FrameRequest,
        options: TransformOptions,
    ) -> Result<FrameResult, FrameError> {
        if !point
            .position_km
            .iter()
            .chain(point.velocity_km_s.iter().flatten())
            .all(|v| v.is_finite())
        {
            return Err(FrameError::NonFiniteState);
        }
        if let Some(limit) = options.max_position_error_m {
            if !limit.is_finite() || limit < 0.0 {
                return Err(FrameError::InvalidPositionBound);
            }
        }

        // Bare WGS84 requests against a GPS broadcast state are exact identity
        // requests, even when the specific historical realization is unknown.
        if request == FrameRequest::Wgs84 && point.source == SourceFrameIdentity::GpsBroadcastWgs84
        {
            let realization = if resolved_g2296(point) {
                FrameRealization::Known(FrameId::Wgs84G2296)
            } else {
                FrameRealization::Unknown
            };
            return native_result(
                point,
                SourceFrameIdentity::GpsBroadcastWgs84,
                realization,
                options,
            );
        }

        let source_id = match point.source {
            SourceFrameIdentity::Realization(id) => id,
            SourceFrameIdentity::GpsBroadcastWgs84 if resolved_g2296(point) => FrameId::Wgs84G2296,
            SourceFrameIdentity::GpsBroadcastWgs84 => {
                return Err(FrameError::UnknownSourceRealization)
            },
            SourceFrameIdentity::GlonassBroadcastPz90
                if point.realization == FrameRealization::Known(FrameId::Pz90_11) =>
            {
                FrameId::Pz90_11
            },
            SourceFrameIdentity::GlonassBroadcastPz90 => {
                return Err(FrameError::UnknownSourceRealization)
            },
            other => return Err(FrameError::UnsupportedSource(other)),
        };
        let target_id = match request {
            FrameRequest::Wgs84 if in_g2296_window(point.epoch) => FrameId::Wgs84G2296,
            FrameRequest::Wgs84 => return Err(FrameError::OutsideCatalogWindow),
            FrameRequest::Realization(id) => id,
        };
        let target_identity = match request {
            FrameRequest::Wgs84 => SourceFrameIdentity::GpsBroadcastWgs84,
            FrameRequest::Realization(id) => SourceFrameIdentity::Realization(id),
        };
        if source_id == target_id {
            return native_result(
                point,
                target_identity,
                FrameRealization::Known(target_id),
                options,
            );
        }
        if source_id == FrameId::Pz90_11 && target_id == FrameId::Itrf2014 {
            if !in_pz9011_window(point.epoch) {
                return Err(FrameError::OutsideCatalogWindow);
            }
            if options.numerical_only {
                return Err(FrameError::ApproximationExcluded);
            }
            if options.max_position_error_m.is_some() {
                return Err(FrameError::PositionBoundUnavailable);
            }
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            if options.warnings_as_errors {
                return Err(FrameError::WarningRejected);
            }
            let position_km = pz9011_to_itrf2014_approx(point.position_km);
            if !position_km.iter().all(|v| v.is_finite()) {
                return Err(FrameError::NonFiniteState);
            }
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: target_identity,
                source_realization: FrameRealization::Known(FrameId::Pz90_11),
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: PZ9011_EDGE,
                edge_info: PZ9011_INFO_PATH,
                source_basis: point.source_basis,
                source_evidence: point.source_evidence,
                position_accuracy_note: Some("CAUTION: 2010.0 PZ-90.11 to ITRF2014 ground-station parameters are held fixed at this coordinate epoch; no strict satellite or later-epoch error bound is available."),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
        }
        if source_id == FrameId::Pz90_11 || target_id == FrameId::Pz90_11 {
            return Err(FrameError::NoPath);
        }
        let uses_g2296 = source_id == FrameId::Wgs84G2296 || target_id == FrameId::Wgs84G2296;
        if (uses_g2296 && !in_g2296_window(point.epoch))
            || (!uses_g2296 && !in_itrf_window(point.epoch))
        {
            return Err(FrameError::OutsideCatalogWindow);
        }
        // Operation accuracy and parameter uncertainties do not bound the
        // total satellite-position error at this epoch.
        if options.max_position_error_m.is_some() {
            return Err(FrameError::PositionBoundUnavailable);
        }
        if options.require_velocity {
            return Err(FrameError::VelocityUnavailable);
        }

        let itrf2020 = match source_id {
            FrameId::Wgs84G2296 | FrameId::Itrf2020 => point.position_km,
            FrameId::Itrf2014 => itrf2014_to_2020(point.position_km, point.epoch),
            FrameId::Pz90_11 => unreachable!("PZ-90.11 path returned above"),
        };
        let position_km = match target_id {
            FrameId::Wgs84G2296 | FrameId::Itrf2020 => itrf2020,
            FrameId::Itrf2014 => itrf2020_to_2014(itrf2020, point.epoch),
            FrameId::Pz90_11 => unreachable!("PZ-90.11 path returned above"),
        };
        if !position_km.iter().all(|v| v.is_finite()) {
            return Err(FrameError::NonFiniteState);
        }
        let (edge_ids, edge_info) = match (source_id, target_id) {
            (FrameId::Wgs84G2296, FrameId::Itrf2020) => (G2296_EDGE, G2296_INFO_PATH),
            (FrameId::Itrf2020, FrameId::Wgs84G2296) => {
                (G2296_INVERSE_EDGE, G2296_INVERSE_INFO_PATH)
            },
            (FrameId::Itrf2020, FrameId::Itrf2014) => (ITRF_EDGE, ITRF_INFO_PATH),
            (FrameId::Itrf2014, FrameId::Itrf2020) => (ITRF_INVERSE_EDGE, ITRF_INVERSE_INFO_PATH),
            (FrameId::Wgs84G2296, FrameId::Itrf2014) => (G2296_THEN_ITRF, G2296_THEN_ITRF_INFO),
            (FrameId::Itrf2014, FrameId::Wgs84G2296) => (ITRF_THEN_G2296, ITRF_THEN_G2296_INFO),
            _ => return Err(FrameError::NoPath),
        };
        Ok(FrameResult {
            epoch: point.epoch,
            source: point.source,
            target: target_identity,
            source_realization: FrameRealization::Known(source_id),
            target_realization: FrameRealization::Known(target_id),
            position_km,
            velocity_km_s: None,
            method: if edge_ids.len() == 1 {
                FrameMethod::Helmert
            } else {
                FrameMethod::Composite
            },
            catalog_version: CATALOG_VERSION,
            edge_ids,
            edge_info,
            source_basis: point.source_basis,
            source_evidence: point.source_evidence,
            position_accuracy_note: Some(
                if edge_info.iter().any(|edge| {
                    edge.source == FrameId::Wgs84G2296 || edge.target == FrameId::Wgs84G2296
                }) {
                    "EPSG:10608 operation accuracy is 0.01 m at 2024.0; ITRF2020 Table 2 gives parameter uncertainties when used. Neither is a satellite-position upper bound."
                } else {
                    "ITRF2020 Table 2 parameter uncertainties are not a satellite-position upper bound."
                },
            ),
            velocity_note: Some("cross-frame velocity has no independent chain validation"),
        })
    }
}

fn native_result(
    point: &SpatialPoint,
    target: SourceFrameIdentity,
    realization: FrameRealization,
    options: TransformOptions,
) -> Result<FrameResult, FrameError> {
    if options.require_velocity && point.velocity_km_s.is_none() {
        return Err(FrameError::VelocityUnavailable);
    }
    Ok(FrameResult {
        epoch: point.epoch,
        source: point.source,
        target,
        source_realization: realization,
        target_realization: realization,
        position_km: point.position_km,
        velocity_km_s: point.velocity_km_s,
        method: FrameMethod::Native,
        catalog_version: CATALOG_VERSION,
        edge_ids: NATIVE_EDGES,
        edge_info: NATIVE_INFO,
        source_basis: point.source_basis,
        source_evidence: point.source_evidence,
        position_accuracy_note: None,
        velocity_note: None,
    })
}

fn in_g2296_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2024, 3, 4, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)
}

fn in_pz9011_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2014, 1, 15, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)
}

fn pz9011_to_itrf2014_approx(position_km: [f64; 3]) -> [f64; 3] {
    // UNOOSA ICG 2018 PZ-90.11 v1.2: frame-rotation convention, 2010.0.
    // Translation is metres; rotation is milliarcseconds. Published scale
    // rounds to zero. Holding the parameters fixed is an unbounded approximation.
    let [x, y, z] = position_km.map(|v| v * 1000.0);
    let mas_to_rad = std::f64::consts::PI / (180.0 * 3600.0 * 1000.0);
    let [rx, ry, rz] = [0.035 * mas_to_rad, -0.087 * mas_to_rad, 0.036 * mas_to_rad];
    let [dx, dy, dz] = [-0.0053, -0.0040, -0.0032];
    [
        (x + rz * y - ry * z + dx) / 1000.0,
        (y - rz * x + rx * z + dy) / 1000.0,
        (z + ry * x - rx * y + dz) / 1000.0,
    ]
}

fn in_itrf_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2015, 1, 1, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2027, 1, 1, 0, 0, 0, 0)
}

fn resolved_g2296(point: &SpatialPoint) -> bool {
    in_g2296_window(point.epoch)
        && (point.source_basis == SourceBasis::CallerAsserted
            || point.realization == FrameRealization::Known(FrameId::Wgs84G2296))
}

fn itrf2020_parameters(epoch: Epoch) -> ([f64; 3], f64) {
    let reference = Epoch::from_gregorian_utc(2015, 1, 1, 0, 0, 0, 0);
    let years = (epoch - reference).to_seconds() / 31_557_600.0;
    // IERS ITRF2020 Table 2: ITRF2014 minus ITRF2020, position-vector
    // convention. Translation and rate in mm and mm/Julian year, scale in ppb.
    // The published rotations and rotation rates are zero.
    ([-1.4, -0.9 - 0.1 * years, 1.4 + 0.2 * years], 1.0 - 0.42e-9)
}

fn itrf2020_to_2014(position_km: [f64; 3], epoch: Epoch) -> [f64; 3] {
    let (translation_mm, scale) = itrf2020_parameters(epoch);
    std::array::from_fn(|i| scale * position_km[i] + translation_mm[i] * 1e-6)
}

fn itrf2014_to_2020(position_km: [f64; 3], epoch: Epoch) -> [f64; 3] {
    let (translation_mm, scale) = itrf2020_parameters(epoch);
    std::array::from_fn(|i| (position_km[i] - translation_mm[i] * 1e-6) / scale)
}
