//! Message-aware native NAV state and a dated terrestrial-frame conversion.
//!
//! A frame family name alone is not a frame realization: GPS and NavIC
//! broadcasts can both carry WGS-84-family coordinates.
use super::{
    glonass_fdma::FdmaError,
    legacy_kepler::KeplerStateError,
    sbas::SbasError,
    selection::{NativeFrame, NavCandidate, NavRejection},
};
use crate::{
    navigation::{NavKey, NavMessageType},
    prelude::{Constellation, Epoch},
};
use std::str::FromStr;

/// The broadcasting system that defines the native terrestrial axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFrameIdentity {
    GpsBroadcastWgs84,
    NavicBroadcastWgs84,
    QzssBroadcastJgs,
    GlonassBroadcastPz90,
    GalileoBroadcastGtrf,
    BeidouBroadcast,
    SbasBroadcast,
    /// A concrete terrestrial realization asserted by a non-NAV caller.
    Realization(FrameId),
}

/// Concrete Earth-fixed realizations supported by the fixed catalogue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameId {
    Wgs84G2296,
    Pz90_11,
    /// QZSS PNT JGS aligned to ITRF2014 in the documented 2021–2023 period.
    QzssJgsItrf2014Aligned,
    /// Galileo GTRF23v01, aligned to ITRF2020 from 2023-05-05.
    GalileoGtrf23v01,
    /// BeiDou BDCS(2019v01), inferred for dated C10/C20 D1 and C05 D2 samples.
    Bdcs2019v01,
    Itrf2020,
    Itrf2014,
}

/// RINEX NAV alone does not identify a particular terrestrial-frame realization.
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
    nominal_sample: Option<NominalSample>,
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

/// Only these published reference records may use an unverified nominal frame label.
/// This is a sample guard, not a general NavIC or SBAS frame relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NominalSample {
    NavicI02,
    GaganS27,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialStateError {
    Rejected(NavRejection),
    UnsupportedMessage,
    Kepler(KeplerStateError),
    Fdma(FdmaError),
    Sbas(SbasError),
}

impl std::fmt::Display for SpatialStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SpatialStateError {}

impl NavCandidate<'_> {
    /// Reuse the selected message's verified propagator; do not reselect it.
    pub fn spatial_state_at(&self, t: Epoch) -> Result<NavSpatialState, SpatialStateError> {
        if let Some(reason) = self.rejection {
            return Err(SpatialStateError::Rejected(reason));
        }
        let (source, native_frame, position_km, velocity_km_s) =
            if self.key.sv.constellation.is_sbas()
                && matches!(
                    self.key.msgtype,
                    NavMessageType::SBAS | NavMessageType::LNAV
                )
            {
                let s = self.sbas_state_at(t).map_err(SpatialStateError::Sbas)?;
                (
                    SourceFrameIdentity::SbasBroadcast,
                    s.frame,
                    s.position_km,
                    s.velocity_km_s,
                )
            } else {
                match (self.key.sv.constellation, self.key.msgtype) {
                    (Constellation::Glonass, NavMessageType::FDMA | NavMessageType::LNAV) => {
                        let s = self.fdma_state_at(t).map_err(SpatialStateError::Fdma)?;
                        (
                            SourceFrameIdentity::GlonassBroadcastPz90,
                            s.frame,
                            s.position_km,
                            s.velocity_km_s,
                        )
                    },
                    (constellation, message)
                        if matches!(
                            (constellation, message),
                            (
                                Constellation::GPS | Constellation::QZSS | Constellation::IRNSS,
                                NavMessageType::LNAV
                            ) | (
                                Constellation::Galileo,
                                NavMessageType::INAV | NavMessageType::FNAV
                            ) | (
                                Constellation::BeiDou,
                                NavMessageType::D1 | NavMessageType::D2
                            )
                        ) =>
                    {
                        let source = match constellation {
                            Constellation::GPS => SourceFrameIdentity::GpsBroadcastWgs84,
                            Constellation::IRNSS => SourceFrameIdentity::NavicBroadcastWgs84,
                            Constellation::QZSS => SourceFrameIdentity::QzssBroadcastJgs,
                            Constellation::Galileo => SourceFrameIdentity::GalileoBroadcastGtrf,
                            Constellation::BeiDou => SourceFrameIdentity::BeidouBroadcast,
                            _ => return Err(SpatialStateError::UnsupportedMessage),
                        };
                        let s = self.kepler_state_at(t).map_err(SpatialStateError::Kepler)?;
                        (source, s.frame, s.position_km, s.velocity_km_s)
                    },
                    _ => return Err(SpatialStateError::UnsupportedMessage),
                }
            };
        // The record, orbit reference, and requested state instant must all
        // fall in the conservatively documented PZ-90.11 broadcast period.
        let (realization, source_evidence) = if source == SourceFrameIdentity::GpsBroadcastWgs84
            && self.orbit_reference.is_some_and(in_g2296_window)
            && in_g2296_window(self.key.epoch)
            && in_g2296_window(t)
        {
            (
                FrameRealization::Known(FrameId::Wgs84G2296),
                Some(G2296_SOURCE_EVIDENCE),
            )
        } else if source == SourceFrameIdentity::GlonassBroadcastPz90
            && self.orbit_reference.is_some_and(in_pz9011_window)
            && in_pz9011_window(self.key.epoch)
            && in_pz9011_window(t)
        {
            (
                FrameRealization::Known(FrameId::Pz90_11),
                Some(PZ9011_SOURCE_EVIDENCE),
            )
        } else if source == SourceFrameIdentity::QzssBroadcastJgs
            && self.key.msgtype == NavMessageType::LNAV
            && qzss_jgs2014_resolved(self.key.epoch, self.orbit_reference, t)
        {
            (
                FrameRealization::Known(FrameId::QzssJgsItrf2014Aligned),
                Some(QZSS_JGS2014_SOURCE_EVIDENCE),
            )
        } else if source == SourceFrameIdentity::GalileoBroadcastGtrf
            && matches!(
                self.key.msgtype,
                NavMessageType::INAV | NavMessageType::FNAV
            )
            && galileo_gtrf23_resolved(self.key.epoch, self.orbit_reference, t)
        {
            (
                FrameRealization::Known(FrameId::GalileoGtrf23v01),
                Some(GALILEO_GTRF23_SOURCE_EVIDENCE),
            )
        } else if source == SourceFrameIdentity::BeidouBroadcast
            && matches!(
                (self.key.msgtype, self.key.sv.prn),
                (NavMessageType::D1, 10 | 20) | (NavMessageType::D2, 5)
            )
            && bdcs2019_sample_resolved(self.key.epoch, self.orbit_reference, t)
        {
            (
                FrameRealization::Known(FrameId::Bdcs2019v01),
                Some(BDCS2019_SOURCE_EVIDENCE),
            )
        } else {
            (FrameRealization::Unknown, None)
        };
        let nominal_sample = nominal_sample_for(self, source);
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
                nominal_sample,
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
    BoundedApproximate,
    /// Numerically evaluated position with no validated epoch/domain error bound.
    UnboundedApproximate,
}

/// What supports the returned position's target-frame label.
/// This describes the frame operation, not total satellite-position accuracy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionStatus {
    NativeIdentity,
    NumericalTransform,
    MarkedApproximation,
    /// The source-to-target physical relation is unverified; the coordinates are nominal.
    NominalAssumption,
}

/// A target state. Unknown realization remains explicit rather than inventing Gxxxx.
#[derive(Clone, Copy, Debug)]
pub struct FrameResult {
    pub epoch: Epoch,
    pub source: SourceFrameIdentity,
    pub target: SourceFrameIdentity,
    /// The requested output label. For `NominalAssumption` this does not
    /// establish that the native coordinates physically realize that frame.
    pub target_realization: FrameRealization,
    pub source_realization: FrameRealization,
    /// Always inspect `position_status()` before interpreting this target XYZ.
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
    /// NAV broadcast realization evidence, separate from the conversion edge.
    /// None for caller-asserted points and unresolved NAV sources.
    pub source_evidence: Option<&'static str>,
    /// Explicit, unverified alignment assumption. Never a known source realization.
    pub assumption: Option<&'static FrameAssumptionInfo>,
    /// A published operation accuracy is not a strict position upper bound.
    pub position_accuracy_note: Option<&'static str>,
    pub velocity_note: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameAssumptionInfo {
    pub id: &'static str,
    pub source_url: &'static str,
    /// Hash of the reference fixture, not a hash of the caller's runtime file.
    pub reference_fixture_sha256: &'static str,
    pub scope: &'static str,
    pub operation: &'static str,
    pub time_note: &'static str,
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

    /// Inspect this before treating `position_km` as a coordinate in `target`.
    /// A concrete target ID alone does not establish a physical frame relation.
    pub fn position_status(&self) -> PositionStatus {
        if self.assumption.is_some() {
            PositionStatus::NominalAssumption
        } else {
            match self.method {
                FrameMethod::Native => PositionStatus::NativeIdentity,
                FrameMethod::Helmert | FrameMethod::Composite => PositionStatus::NumericalTransform,
                FrameMethod::BoundedApproximate | FrameMethod::UnboundedApproximate => {
                    PositionStatus::MarkedApproximation
                },
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBasis {
    NavMessageAndCatalogDate,
    /// The caller supplied the source identity; the library did not verify it.
    /// A dated catalogue may infer a concrete GPS broadcast realization from it.
    CallerAsserted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MethodPolicy {
    /// Allows marked, unbounded approximations when no validated path exists.
    BestAvailable,
    /// Requires a validated numerical path; excludes approximate edges.
    NumericalOnly,
}

#[derive(Clone, Copy, Debug)]
pub struct TransformOptions {
    pub method: MethodPolicy,
    pub max_position_error_m: Option<f64>,
    pub require_velocity: bool,
    /// Reject marked approximations and unverified nominal assumptions.
    /// Informational accuracy/velocity notes on numerical paths are retained.
    pub warnings_as_errors: bool,
}

impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            method: MethodPolicy::BestAvailable,
            max_position_error_m: None,
            require_velocity: false,
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
    nominal_sample: Option<NominalSample>,
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
            realization: match source {
                SourceFrameIdentity::Realization(id) => FrameRealization::Known(id),
                _ => FrameRealization::Unknown,
            },
            source_evidence: None,
            nominal_sample: None,
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
        point.nominal_sample = state.nominal_sample;
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
    PositionBoundExceeded,
    InvalidPositionBound,
    ApproximationExcluded,
    DomainNotApplicable,
    VelocityUnavailable,
    InconsistentFrame,
    NonFiniteState,
    WarningRejected(&'static str),
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
        let expected = match self.source {
            SourceFrameIdentity::GpsBroadcastWgs84 => NativeFrame::GpsBroadcastWgs84,
            SourceFrameIdentity::NavicBroadcastWgs84 => NativeFrame::NavicBroadcastWgs84,
            SourceFrameIdentity::SbasBroadcast => NativeFrame::SbasBroadcast,
            SourceFrameIdentity::GlonassBroadcastPz90 => NativeFrame::GlonassBroadcastPz90,
            SourceFrameIdentity::QzssBroadcastJgs => NativeFrame::QzssBroadcastJgs,
            SourceFrameIdentity::GalileoBroadcastGtrf => NativeFrame::GalileoBroadcastGtrf,
            SourceFrameIdentity::BeidouBroadcast => NativeFrame::BeiDouBroadcastCgcs2000,
            SourceFrameIdentity::Realization(_) => self.native_frame,
        };
        if self.native_frame != expected {
            return Err(FrameError::InconsistentFrame);
        }
        FrameTransformer.to_frame(
            &SpatialPoint::from_nav(self)?,
            target,
            TransformOptions::default(),
        )
    }
}

fn navic_i02_reference_epoch() -> Epoch {
    Epoch::from_str("2023-03-12T00:00:00 GPST").expect("fixed NavIC I02 reference epoch")
}

fn sbas_s27_reference_epoch() -> Epoch {
    Epoch::from_str("2023-03-12T01:15:44 GPST").expect("fixed SBAS S27 reference epoch")
}

/// Keep fixture-specific eligibility in one place. Matching values restricts the
/// library's explicit assumption; it does not authenticate a caller's NAV file.
fn nominal_sample_for(
    candidate: &NavCandidate<'_>,
    source: SourceFrameIdentity,
) -> Option<NominalSample> {
    let key = candidate.key;
    let eph = candidate.ephemeris;
    if source == SourceFrameIdentity::NavicBroadcastWgs84
        && key.sv.prn == 2
        && key.msgtype == NavMessageType::LNAV
        && key.epoch == navic_i02_reference_epoch()
        && candidate.orbit_reference == Some(navic_i02_reference_epoch())
        && candidate.clock_reference == navic_i02_reference_epoch()
        && eph.get_orbit_f64("iodec") == Some(0.0)
        && eph.clock_bias == 1.104795373976e-4
        && eph.clock_drift == -2.819433575496e-11
        && eph.clock_drift_rate == 0.0
        && [
            ("m0", 2.597586517985),
            ("e", 1.982442918234e-3),
            ("sqrta", 6.493359437943e3),
            ("omega0", 1.593280097620),
            ("week", 2253.0),
            ("health", 0.0),
        ]
        .iter()
        .all(|(field, expected)| eph.get_orbit_f64(field) == Some(*expected))
    {
        return Some(NominalSample::NavicI02);
    }
    if source == SourceFrameIdentity::SbasBroadcast
        && key.sv.constellation == Constellation::GAGAN
        && key.sv.prn == 27
        && matches!(key.msgtype, NavMessageType::SBAS | NavMessageType::LNAV)
        && key.epoch == sbas_s27_reference_epoch()
        && candidate.orbit_reference == Some(sbas_s27_reference_epoch())
        && candidate.clock_reference == sbas_s27_reference_epoch()
        && eph.clock_bias == 1.117587089539e-8
        && eph.clock_drift == 2.273736754432e-11
        && eph.clock_drift_rate == 0.0
        && [
            ("satPosX", 24160.61976),
            ("velX", -0.00267625),
            ("accelX", -1.0e-7),
            ("health", 0.0),
            ("satPosY", 34538.67944),
            ("velY", 0.00012625),
            ("accelY", 2.0e-7),
            ("accuracyCode", 4096.0),
            ("satPosZ", 30.4972),
            ("velZ", -0.001396),
            ("accelZ", -1.875e-7),
            ("iodn", 28.0),
            ("t_tm", 4519.0),
        ]
        .iter()
        .all(|(field, expected)| eph.get_orbit_f64(field) == Some(*expected))
    {
        return Some(NominalSample::GaganS27);
    }
    None
}

const CATALOG_VERSION: &str = "N09d-SBAS-S27-nominal-approx-v7";
const NAVIC_I02_ASSUMPTION: FrameAssumptionInfo = FrameAssumptionInfo {
    id: "rinex:NavIC-I02:unknown-WGS84-to-nominal-ITRF2014:zero-v1",
    source_url: "https://www.isro.gov.in/media_isro/pdf/Missions/irnss_sps_icd_version1.1-2017.pdf",
    reference_fixture_sha256: "ae632c2debb026be9acaf207549a903bda9d89466c0ed1535c0e7cb2b2309ecd",
    scope: "I02 LNAV record/ToE/ToC 2023-03-12T00:00:00 GPST proxy, IODEC 0, within existing 7200 s selection window; no runtime-file hash check",
    operation: "copy native WGS84-family ECEF XYZ as nominal ITRF2014; source realization and frame offset unverified",
    time_note: "IRNSST is represented by GPST proxy; physical time-scale difference is unverified",
};
const SBAS_S27_ASSUMPTION: FrameAssumptionInfo = FrameAssumptionInfo {
    id: "rinex:GAGAN-S27:unknown-WGS84-to-nominal-ITRF2014:zero-v1",
    source_url: "https://aim-india.aai.aero/eAIP_Archive/19-05-2022/eAIP/IN-ENR%204.3-en-GB.html",
    reference_fixture_sha256: "93c7b062ebb651d17c02cc4cebb023b4717c6b56cec6b6a24a63fdc8b718fbee",
    scope: "GAGAN S27/PRN127 SBAS EPH or identical LNAV-compatible record at 2023-03-12T01:15:44 GPST, matching broadcast fields, within |t-Toc| < 360 s; no runtime-file hash check",
    operation: "copy native SBAS WGS84-family ECEF XYZ as nominal ITRF2014; source realization and frame offset unverified",
    time_note: "RINEX SBAS record and propagation use GPST; no separate time-system offset applied",
};
const PZ9011_SOURCE_EVIDENCE: &str = "ICG17:2023:GNSS-TRFs:p9; ICG11:2016:PZ90.11-introduction:p12";
const G2296_SOURCE_EVIDENCE: &str = "https://www.navcen.uscg.gov/gps-constellation (NANU 2024014)";
const PZ9011_TO_ITRF2014: &str = "ICG:2018:PZ90.11-to-ITRF2014:static-2010-approx";
const QZSS_JGS2014_SOURCE_EVIDENCE: &str = "QZSS:PNT-coordinate-system:2023-11-10:JGS-ITRF2014-period; QZSS:PNT-update-complete:2021-02-15";
const QZSS_JGS2014_TO_ITRF2014: &str = "QZSS:PNT:JGS-ITRF2014-alignment:zero-offset-approx";
const GALILEO_GTRF23_SOURCE_EVIDENCE: &str =
    "ESA:GGSP:GTRF23v01:applicable-2023-05-05; ICG18:2024:planned-GTRF-update";
const GTRF23_TO_ITRF2020: &str = "ESA:GGSP:GTRF23v01-ITRF2020:zero-offset-approx";
const BDCS2019_SOURCE_EVIDENCE: &str =
    "CSNO:BDCS:2019v01; IGS:2022-workshop:2019v01-current:date-inferred";
const BDCS2019_TO_ITRF2014: &str = "rinex:BDCS2019v01-ITRF2014:zero-offset-approx";
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
const QZSS_JGS2014_EDGE: &[&str] = &[QZSS_JGS2014_TO_ITRF2014];
const GTRF23_THEN_ITRF2014: &[&str] = &[GTRF23_TO_ITRF2020, ITRF2020_TO_ITRF2014];
const BDCS2019_EDGE: &[&str] = &[BDCS2019_TO_ITRF2014];
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
    valid_window:
        "2014-01-15 through 2024-12-31 UTC (library approximation window, not publication validity)",
    source_url: "https://www.unoosa.org/documents/pdf/icg/2018/icg13/wgd/wgd_24.pdf",
    position_metric: "2010.0 ground-station fit RMS 0.012 m; no satellite or epoch error bound",
    velocity_capability: "not established",
};
const QZSS_JGS2014_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: QZSS_JGS2014_TO_ITRF2014,
    source: FrameId::QzssJgsItrf2014Aligned,
    target: FrameId::Itrf2014,
    method: FrameMethod::UnboundedApproximate,
    parameter_reference_epoch: "none; zero-offset approximation of documented ITRF2014 alignment",
    valid_window: "2021-02-16 through 2023-11-08 UTC (conservative library window)",
    source_url: "https://qzss.go.jp/en/technical/dod/pnt/coordinate-system.html",
    position_metric:
        "PNT monitor-station offset within 0.02 m (95%); no satellite or strict upper bound",
    velocity_capability: "not established",
};
const GTRF23_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: GTRF23_TO_ITRF2020,
    source: FrameId::GalileoGtrf23v01,
    target: FrameId::Itrf2020,
    method: FrameMethod::UnboundedApproximate,
    parameter_reference_epoch: "none; zero-offset approximation of GTRF23v01 alignment",
    valid_window: "2024-05-01 through 2024-05-31 UTC (narrow library inference window)",
    source_url: "https://navigation-office.esa.int/attachments/32835744/1/GTRF_IGS_Stop_6.pdf",
    position_metric: "GTRF station alignment <0.03 m (2 sigma); no satellite or strict upper bound",
    velocity_capability: "not established",
};
const BDCS2019_INFO: FrameEdgeInfo = FrameEdgeInfo {
    id: BDCS2019_TO_ITRF2014,
    source: FrameId::Bdcs2019v01,
    target: FrameId::Itrf2014,
    method: FrameMethod::UnboundedApproximate,
    parameter_reference_epoch: "2019v01 alignment; zero-offset approximation, no dated rates",
    valid_window: "2022-06-01 through 2022-06-30 UTC (narrow library inference window)",
    source_url: "https://files.igs.org/pub/resource/pubs/workshop/2022/TourdelIGS4_05_Hu.pdf",
    position_metric: "2019 station alignment, not a satellite or 2022 strict error bound",
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
const QZSS_JGS2014_INFO_PATH: &[FrameEdgeInfo] = &[QZSS_JGS2014_INFO];
const GTRF23_THEN_ITRF2014_INFO: &[FrameEdgeInfo] = &[GTRF23_INFO, ITRF_INFO];
const BDCS2019_INFO_PATH: &[FrameEdgeInfo] = &[BDCS2019_INFO];

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
    /// operational GPS update completed. The ITRF edge has its own window.
    /// Cross-frame velocity is withheld until
    /// the full chain has an independently checked velocity reference.
    pub fn to_frame(
        &self,
        point: &SpatialPoint,
        request: FrameRequest,
        options: TransformOptions,
    ) -> Result<FrameResult, FrameError> {
        let result = self.to_frame_inner(point, request, options)?;
        if options.warnings_as_errors
            && matches!(
                result.position_status(),
                PositionStatus::MarkedApproximation | PositionStatus::NominalAssumption
            )
        {
            return Err(FrameError::WarningRejected(
                result
                    .position_accuracy_note
                    .or(result.velocity_note)
                    .unwrap_or("CAUTION: unverified frame assumption"),
            ));
        }
        Ok(result)
    }

    fn to_frame_inner(
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

        if point.source == SourceFrameIdentity::NavicBroadcastWgs84
            && request == FrameRequest::Realization(FrameId::Itrf2014)
        {
            if point.nominal_sample != Some(NominalSample::NavicI02)
                || point.source_basis != SourceBasis::NavMessageAndCatalogDate
                || point.realization != FrameRealization::Unknown
            {
                return Err(FrameError::UnsupportedSource(point.source));
            }
            if (point.epoch - navic_i02_reference_epoch())
                .to_seconds()
                .abs()
                >= 7200.0
            {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::NavBroadcastSatellite,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: SourceFrameIdentity::Realization(FrameId::Itrf2014),
                source_realization: FrameRealization::Unknown,
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km: point.position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: NATIVE_EDGES,
                edge_info: NATIVE_INFO,
                source_basis: point.source_basis,
                source_evidence: None,
                assumption: Some(&NAVIC_I02_ASSUMPTION),
                position_accuracy_note: Some("CAUTION: nominal ITRF2014 XYZ copies NavIC I02 native WGS84-family XYZ under an unverified zero-offset assumption; source realization and physical frame accuracy are unknown; no strict position-error bound"),
                velocity_note: Some("cross-frame velocity is not validated; IRNSST uses a GPST proxy"),
            });
        }

        if point.source == SourceFrameIdentity::SbasBroadcast
            && request == FrameRequest::Realization(FrameId::Itrf2014)
        {
            if point.nominal_sample != Some(NominalSample::GaganS27)
                || point.source_basis != SourceBasis::NavMessageAndCatalogDate
                || point.realization != FrameRealization::Unknown
            {
                return Err(FrameError::UnsupportedSource(point.source));
            }
            if (point.epoch - sbas_s27_reference_epoch())
                .to_seconds()
                .abs()
                >= 360.0
            {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::NavBroadcastSatellite,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: SourceFrameIdentity::Realization(FrameId::Itrf2014),
                source_realization: FrameRealization::Unknown,
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km: point.position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: NATIVE_EDGES,
                edge_info: NATIVE_INFO,
                source_basis: point.source_basis,
                source_evidence: None,
                assumption: Some(&SBAS_S27_ASSUMPTION),
                position_accuracy_note: Some("CAUTION: nominal ITRF2014 XYZ copies GAGAN S27 native WGS84-family XYZ under an unverified zero-offset assumption; source realization and physical frame accuracy are unknown; no strict position-error bound"),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
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
            SourceFrameIdentity::GlonassBroadcastPz90
                if point.realization == FrameRealization::Known(FrameId::Pz90_11) =>
            {
                FrameId::Pz90_11
            },
            SourceFrameIdentity::GlonassBroadcastPz90 => {
                return Err(FrameError::UnknownSourceRealization)
            },
            SourceFrameIdentity::QzssBroadcastJgs
                if point.realization
                    == FrameRealization::Known(FrameId::QzssJgsItrf2014Aligned) =>
            {
                FrameId::QzssJgsItrf2014Aligned
            },
            SourceFrameIdentity::QzssBroadcastJgs => {
                return Err(FrameError::UnknownSourceRealization)
            },
            SourceFrameIdentity::GalileoBroadcastGtrf
                if point.realization == FrameRealization::Known(FrameId::GalileoGtrf23v01) =>
            {
                FrameId::GalileoGtrf23v01
            },
            SourceFrameIdentity::GalileoBroadcastGtrf => {
                return Err(FrameError::UnknownSourceRealization)
            },
            SourceFrameIdentity::BeidouBroadcast
                if point.realization == FrameRealization::Known(FrameId::Bdcs2019v01) =>
            {
                FrameId::Bdcs2019v01
            },
            SourceFrameIdentity::BeidouBroadcast => {
                return Err(FrameError::UnknownSourceRealization)
            },
            SourceFrameIdentity::GpsBroadcastWgs84 if resolved_g2296(point) => FrameId::Wgs84G2296,
            SourceFrameIdentity::GpsBroadcastWgs84 => {
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
        if source_id == FrameId::QzssJgsItrf2014Aligned && target_id == FrameId::Itrf2014 {
            if !in_qzss_jgs2014_window(point.epoch) {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::AnyEarthFixed,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            // The official source documents alignment, not a seven-parameter
            // correction. Keep zero correction visibly approximate.
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: target_identity,
                source_realization: FrameRealization::Known(FrameId::QzssJgsItrf2014Aligned),
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km: point.position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: QZSS_JGS2014_EDGE,
                edge_info: QZSS_JGS2014_INFO_PATH,
                source_basis: point.source_basis,
                source_evidence: point.source_evidence,
                assumption: None,
                position_accuracy_note: Some("CAUTION: QZSS PNT JGS was aligned to ITRF2014 in this period. This zero-offset position approximation is not an exact frame identity; the published 0.02 m (95%) monitor-station alignment is not a satellite-position or strict error bound."),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
        }
        if source_id == FrameId::QzssJgsItrf2014Aligned
            || target_id == FrameId::QzssJgsItrf2014Aligned
        {
            return Err(FrameError::NoPath);
        }
        if source_id == FrameId::GalileoGtrf23v01 && target_id == FrameId::Itrf2014 {
            if !in_gtrf23_sample_window(point.epoch) || !in_itrf_window(point.epoch) {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::AnyEarthFixed,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            // The first edge has no published offset parameters. The second
            // edge uses ITRF2020 Table 2 at this point's coordinate epoch.
            let position_km = itrf2020_to_2014(point.position_km, point.epoch);
            if !position_km.iter().all(|v| v.is_finite()) {
                return Err(FrameError::NonFiniteState);
            }
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: target_identity,
                source_realization: FrameRealization::Known(FrameId::GalileoGtrf23v01),
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: GTRF23_THEN_ITRF2014,
                edge_info: GTRF23_THEN_ITRF2014_INFO,
                source_basis: point.source_basis,
                source_evidence: point.source_evidence,
                assumption: None,
                position_accuracy_note: Some("CAUTION: GTRF23v01 to ITRF2020 is a zero-offset approximation. ESA's station alignment statistic is not a satellite-position or strict error bound; 2024-05 applicability is inferred from the 2023 effective date and the 2024 ICG update report. The ITRF2020 to ITRF2014 edge is numerical."),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
        }
        if source_id == FrameId::GalileoGtrf23v01 || target_id == FrameId::GalileoGtrf23v01 {
            return Err(FrameError::NoPath);
        }
        if source_id == FrameId::Bdcs2019v01 && target_id == FrameId::Itrf2014 {
            if !in_bdcs2019_sample_window(point.epoch) {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::AnyEarthFixed,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
            }
            return Ok(FrameResult {
                epoch: point.epoch,
                source: point.source,
                target: target_identity,
                source_realization: FrameRealization::Known(FrameId::Bdcs2019v01),
                target_realization: FrameRealization::Known(FrameId::Itrf2014),
                position_km: point.position_km,
                velocity_km_s: None,
                method: FrameMethod::UnboundedApproximate,
                catalog_version: CATALOG_VERSION,
                edge_ids: BDCS2019_EDGE,
                edge_info: BDCS2019_INFO_PATH,
                source_basis: point.source_basis,
                source_evidence: point.source_evidence,
                assumption: None,
                position_accuracy_note: Some("CAUTION: BDCS(2019v01) to ITRF2014 uses a zero-offset approximation. Its applicability to this 2022 broadcast is inferred from 2022 IGS material, not a day-specific provider certificate. Published station alignment is not a satellite-position or strict 2022 error bound."),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
        }
        if source_id == FrameId::Bdcs2019v01 || target_id == FrameId::Bdcs2019v01 {
            return Err(FrameError::NoPath);
        }
        if source_id == FrameId::Pz90_11 && target_id == FrameId::Itrf2014 {
            if !in_pz9011_window(point.epoch) {
                return Err(FrameError::OutsideCatalogWindow);
            }
            qualify_path(
                PathEvidence {
                    method: FrameMethod::UnboundedApproximate,
                    metric: PositionMetric::Unbounded,
                    domain: PathDomain::AnyEarthFixed,
                },
                point,
                options,
            )?;
            if options.require_velocity {
                return Err(FrameError::VelocityUnavailable);
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
                assumption: None,
                position_accuracy_note: Some("CAUTION: ICG-13 PZ-90.11 to ITRF2014 parameters were estimated at 2010.0 from ground stations. Applying them unchanged at this point's epoch is an approximation; the published 0.012 m RMS is not a satellite-position or epoch error bound."),
                velocity_note: Some("cross-frame velocity is not validated"),
            });
        }
        if source_id == FrameId::Pz90_11 || target_id == FrameId::Pz90_11 {
            return Err(FrameError::NoPath);
        }
        if (source_id == FrameId::Wgs84G2296 || target_id == FrameId::Wgs84G2296)
            && !in_g2296_window(point.epoch)
        {
            return Err(FrameError::OutsideCatalogWindow);
        }
        if !in_itrf_window(point.epoch) {
            return Err(FrameError::OutsideCatalogWindow);
        }
        // The older GPS and ITRF catalogue edges are numerical.
        // Published operation accuracy and parameter uncertainty are not a
        // proven worst-case satellite-position bound.
        qualify_path(
            PathEvidence {
                method: FrameMethod::Helmert,
                metric: PositionMetric::Unbounded,
                domain: PathDomain::AnyEarthFixed,
            },
            point,
            options,
        )?;
        if options.require_velocity {
            return Err(FrameError::VelocityUnavailable);
        }

        let itrf2020 = match source_id {
            FrameId::Wgs84G2296 | FrameId::Itrf2020 => point.position_km,
            FrameId::Itrf2014 => itrf2014_to_2020(point.position_km, point.epoch),
            FrameId::Pz90_11 => unreachable!("PZ-90.11 paths returned above"),
            FrameId::QzssJgsItrf2014Aligned => unreachable!("QZSS paths returned above"),
            FrameId::GalileoGtrf23v01 => unreachable!("Galileo paths returned above"),
            FrameId::Bdcs2019v01 => unreachable!("BeiDou paths returned above"),
        };
        let position_km = match target_id {
            FrameId::Wgs84G2296 | FrameId::Itrf2020 => itrf2020,
            FrameId::Itrf2014 => itrf2020_to_2014(itrf2020, point.epoch),
            FrameId::Pz90_11 => unreachable!("PZ-90.11 paths returned above"),
            FrameId::QzssJgsItrf2014Aligned => unreachable!("QZSS paths returned above"),
            FrameId::GalileoGtrf23v01 => unreachable!("Galileo paths returned above"),
            FrameId::Bdcs2019v01 => unreachable!("BeiDou paths returned above"),
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
            assumption: None,
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

#[allow(dead_code)] // The fixed v1 catalogue installs only the numerical edge.
#[derive(Clone, Copy)]
enum PositionMetric {
    Unbounded,
    Rms,
    UpperBound(f64),
}

#[allow(dead_code)] // A satellite-only edge is exercised with a synthetic candidate.
#[derive(Clone, Copy)]
enum PathDomain {
    AnyEarthFixed,
    NavBroadcastSatellite,
}

#[derive(Clone, Copy)]
struct PathEvidence {
    method: FrameMethod,
    metric: PositionMetric,
    domain: PathDomain,
}

/// Shared option gate; synthetic candidate tests below exercise the decisions
/// without installing unsupported approximate edges in the public catalogue.
fn qualify_path(
    evidence: PathEvidence,
    point: &SpatialPoint,
    options: TransformOptions,
) -> Result<(), FrameError> {
    if options.method == MethodPolicy::NumericalOnly
        && matches!(
            evidence.method,
            FrameMethod::BoundedApproximate | FrameMethod::UnboundedApproximate
        )
    {
        return Err(FrameError::ApproximationExcluded);
    }
    match evidence.domain {
        PathDomain::AnyEarthFixed => {},
        PathDomain::NavBroadcastSatellite => {
            if point.source_basis != SourceBasis::NavMessageAndCatalogDate {
                return Err(FrameError::DomainNotApplicable);
            }
        },
    }
    if let Some(limit) = options.max_position_error_m {
        match evidence.metric {
            PositionMetric::Unbounded => return Err(FrameError::PositionBoundUnavailable),
            PositionMetric::Rms => return Err(FrameError::PositionBoundUnavailable),
            PositionMetric::UpperBound(bound) if bound > limit => {
                return Err(FrameError::PositionBoundExceeded)
            },
            PositionMetric::UpperBound(_) => {},
        }
    }
    Ok(())
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
        assumption: None,
        position_accuracy_note: None,
        velocity_note: None,
    })
}

fn in_g2296_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2024, 3, 4, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)
}

fn resolved_g2296(point: &SpatialPoint) -> bool {
    in_g2296_window(point.epoch)
        && (point.source_basis == SourceBasis::CallerAsserted
            || point.realization == FrameRealization::Known(FrameId::Wgs84G2296))
}

fn in_pz9011_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2014, 1, 15, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2025, 1, 1, 0, 0, 0, 0)
}

fn in_qzss_jgs2014_window(epoch: Epoch) -> bool {
    // The coordinate history lists Japanese change dates, not exact instants.
    // Start after the 2021 change and stop before the 2023 change work.
    epoch >= Epoch::from_gregorian_utc(2021, 2, 16, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2023, 11, 9, 0, 0, 0, 0)
}

fn in_gtrf23_sample_window(epoch: Epoch) -> bool {
    // ESA dates the start to 2023-05-05. The 2024 ICG report says another
    // update was still planned. Limit this inferred interval to sample month.
    epoch >= Epoch::from_gregorian_utc(2024, 5, 1, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2024, 6, 1, 0, 0, 0, 0)
}

fn in_bdcs2019_sample_window(epoch: Epoch) -> bool {
    // IGS workshop material still calls 2019v01 the current solution in 2022.
    // Limit the inferred operational assignment to the real C10/C20/C05 sample month.
    epoch >= Epoch::from_gregorian_utc(2022, 6, 1, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2022, 7, 1, 0, 0, 0, 0)
}

fn bdcs2019_sample_resolved(
    record_epoch: Epoch,
    orbit_reference: Option<Epoch>,
    evaluation_epoch: Epoch,
) -> bool {
    in_bdcs2019_sample_window(record_epoch)
        && orbit_reference.is_some_and(in_bdcs2019_sample_window)
        && in_bdcs2019_sample_window(evaluation_epoch)
}

fn galileo_gtrf23_resolved(
    record_epoch: Epoch,
    orbit_reference: Option<Epoch>,
    evaluation_epoch: Epoch,
) -> bool {
    in_gtrf23_sample_window(record_epoch)
        && orbit_reference.is_some_and(in_gtrf23_sample_window)
        && in_gtrf23_sample_window(evaluation_epoch)
}

fn qzss_jgs2014_resolved(
    record_epoch: Epoch,
    orbit_reference: Option<Epoch>,
    evaluation_epoch: Epoch,
) -> bool {
    in_qzss_jgs2014_window(record_epoch)
        && orbit_reference.is_some_and(in_qzss_jgs2014_window)
        && in_qzss_jgs2014_window(evaluation_epoch)
}

fn pz9011_to_itrf2014_approx(position_km: [f64; 3]) -> [f64; 3] {
    // ICG-13 2018 wgd_24.pdf pp. 5, 8: coordinate-frame rotation convention.
    // Translation is metres, rotation is milliarcseconds, scale is 10^-6.
    // The published scale is zero to the displayed precision. Apply these
    // 2010.0 parameters unchanged at the point epoch only as an approximation.
    let [x, y, z] = position_km.map(|v| v * 1000.0);
    let mas_to_rad = std::f64::consts::PI / (180.0 * 3600.0 * 1000.0);
    let [rx, ry, rz] = [0.035 * mas_to_rad, -0.087 * mas_to_rad, 0.036 * mas_to_rad];
    let translation_m = [-0.0053, -0.0040, -0.0032];
    [
        (x + rz * y - ry * z + translation_m[0]) / 1000.0,
        (y - rz * x + rx * z + translation_m[1]) / 1000.0,
        (z + ry * x - rx * y + translation_m[2]) / 1000.0,
    ]
}

fn in_itrf_window(epoch: Epoch) -> bool {
    epoch >= Epoch::from_gregorian_utc(2015, 1, 1, 0, 0, 0, 0)
        && epoch < Epoch::from_gregorian_utc(2027, 1, 1, 0, 0, 0, 0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bdcs2019_source_requires_all_three_instants_inside_sample_window() {
        let inside = Epoch::from_gregorian_utc(2022, 6, 8, 7, 0, 0, 0);
        let before = Epoch::from_gregorian_utc(2022, 5, 31, 23, 59, 59, 0);
        let first = Epoch::from_gregorian_utc(2022, 6, 1, 0, 0, 0, 0);
        let last = Epoch::from_gregorian_utc(2022, 6, 30, 23, 59, 59, 0);
        let after = Epoch::from_gregorian_utc(2022, 7, 1, 0, 0, 0, 0);
        assert!(bdcs2019_sample_resolved(first, Some(inside), last));
        assert!(!bdcs2019_sample_resolved(before, Some(inside), inside));
        assert!(!bdcs2019_sample_resolved(inside, Some(after), inside));
        assert!(!bdcs2019_sample_resolved(inside, Some(inside), after));
        assert!(!bdcs2019_sample_resolved(inside, None, inside));
    }

    #[test]
    fn galileo_gtrf23_source_requires_all_three_instants_inside_sample_window() {
        let inside = Epoch::from_gregorian_utc(2024, 5, 7, 0, 29, 42, 0);
        let before = Epoch::from_gregorian_utc(2024, 4, 30, 23, 59, 59, 0);
        let first = Epoch::from_gregorian_utc(2024, 5, 1, 0, 0, 0, 0);
        let last = Epoch::from_gregorian_utc(2024, 5, 31, 23, 59, 59, 0);
        let after = Epoch::from_gregorian_utc(2024, 6, 1, 0, 0, 0, 0);
        assert!(galileo_gtrf23_resolved(first, Some(inside), last));
        assert!(!galileo_gtrf23_resolved(before, Some(inside), inside));
        assert!(!galileo_gtrf23_resolved(inside, Some(after), inside));
        assert!(!galileo_gtrf23_resolved(inside, Some(inside), after));
        assert!(!galileo_gtrf23_resolved(inside, None, inside));
    }

    #[test]
    fn qzss_jgs2014_source_requires_all_three_instants_inside_the_conservative_window() {
        let inside = Epoch::from_gregorian_utc(2023, 3, 12, 0, 0, 0, 0);
        let before = Epoch::from_gregorian_utc(2021, 2, 15, 23, 59, 59, 0);
        let first = Epoch::from_gregorian_utc(2021, 2, 16, 0, 0, 0, 0);
        let last = Epoch::from_gregorian_utc(2023, 11, 8, 23, 59, 59, 0);
        let after = Epoch::from_gregorian_utc(2023, 11, 9, 0, 0, 0, 0);
        assert!(qzss_jgs2014_resolved(first, Some(inside), last));
        assert!(!qzss_jgs2014_resolved(before, Some(inside), inside));
        assert!(!qzss_jgs2014_resolved(inside, Some(after), inside));
        assert!(!qzss_jgs2014_resolved(inside, Some(inside), after));
        assert!(!qzss_jgs2014_resolved(inside, None, inside));
    }

    #[test]
    fn synthetic_approximation_evidence_is_filtered_by_method_domain_and_bound() {
        let direct = SpatialPoint::new(
            [10000.0; 3],
            Epoch::from_gregorian_utc(2024, 5, 7, 0, 0, 0, 0),
            SourceFrameIdentity::Realization(FrameId::Itrf2020),
            None,
        )
        .unwrap();
        let synthetic = PathEvidence {
            method: FrameMethod::BoundedApproximate,
            metric: PositionMetric::UpperBound(2.0),
            domain: PathDomain::NavBroadcastSatellite,
        };
        assert_eq!(
            qualify_path(synthetic, &direct, TransformOptions::default()),
            Err(FrameError::DomainNotApplicable)
        );
        let mut nav = direct;
        nav.source_basis = SourceBasis::NavMessageAndCatalogDate;
        assert_eq!(
            qualify_path(
                synthetic,
                &nav,
                TransformOptions {
                    method: MethodPolicy::NumericalOnly,
                    ..Default::default()
                }
            ),
            Err(FrameError::ApproximationExcluded)
        );
        assert_eq!(
            qualify_path(
                synthetic,
                &nav,
                TransformOptions {
                    max_position_error_m: Some(2.0),
                    ..Default::default()
                }
            ),
            Ok(())
        );
        assert_eq!(
            qualify_path(
                synthetic,
                &nav,
                TransformOptions {
                    max_position_error_m: Some(1.9),
                    ..Default::default()
                }
            ),
            Err(FrameError::PositionBoundExceeded)
        );
        assert_eq!(
            qualify_path(
                PathEvidence {
                    metric: PositionMetric::Rms,
                    ..synthetic
                },
                &nav,
                TransformOptions {
                    max_position_error_m: Some(2.0),
                    ..Default::default()
                }
            ),
            Err(FrameError::PositionBoundUnavailable)
        );
    }
}
