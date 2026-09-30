//! Diagnose every SV in the first OBS epoch against a mixed RINEX 4 NAV file.
//! cargo run --features nav --example nav_first_epoch -- OBS NAV [--compare-nav ORIGINAL_NAV] [--candidates]
use rinex::{
    navigation::rinex::{
        selection::{NavSelection, UnknownHealthPolicy},
        spatial_state::{FrameRequest, FrameTransformer, SpatialPoint, TransformOptions},
    },
    prelude::{Duration, Epoch, Rinex, SV},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    str::FromStr,
};

const C_M_S: f64 = 299_792_458.0;

fn read(path: &str) -> Result<Rinex, Box<dyn Error>> {
    if path.ends_with(".gz") {
        Ok(Rinex::from_gzip_file(path)?)
    } else {
        Ok(Rinex::from_file(path)?)
    }
}

fn selected_key(report: &NavSelection<'_>) -> Option<rinex::navigation::NavKey> {
    report.chosen().map(|candidate| *candidate.key)
}

fn original_sv_labels(path: &str) -> Result<BTreeMap<SV, String>, Box<dyn Error>> {
    let contents = std::fs::read_to_string(path)?;
    let mut lines = contents
        .lines()
        .skip_while(|line| !line.contains("END OF HEADER"));
    let _header_end = lines.next().ok_or("OBS header is incomplete")?;
    let epoch = lines.next().ok_or("OBS epoch is missing")?;
    if !epoch.starts_with("> 2024 05 10 03 00  0.0000000  0 56") {
        return Err("unexpected first OBS epoch".into());
    }
    let mut labels = BTreeMap::new();
    for line in lines.take_while(|line| !line.starts_with('>')) {
        let label = line.get(..3).ok_or("short OBS satellite row")?;
        let sv = SV::from_str(label)?;
        if labels.insert(sv, label.to_string()).is_some() {
            return Err("duplicate first-epoch satellite".into());
        }
    }
    Ok(labels)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let obs_path = args
        .next()
        .ok_or("usage: nav_first_epoch OBS NAV [--compare-nav ORIGINAL_NAV] [--candidates]")?;
    let nav_path = args.next().ok_or("missing NAV path")?;
    let mut compare_path = None;
    let mut show_candidates = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compare-nav" if compare_path.is_none() => {
                compare_path = Some(args.next().ok_or("missing original NAV path")?)
            },
            "--candidates" => show_candidates = true,
            _ => return Err(format!("unexpected argument: {arg}").into()),
        }
    }
    let obs = read(&obs_path)?;
    let nav = read(&nav_path)?;
    let original = compare_path.as_deref().map(read).transpose()?;
    let (obs_key, observations) = obs.observations_iter().next().ok_or("OBS has no epoch")?;
    let t: Epoch = obs_key.epoch;
    let svs: BTreeSet<SV> = observations
        .signals
        .iter()
        .map(|signal| signal.sv)
        .collect();
    let labels = original_sv_labels(&obs_path)?;
    if labels.keys().copied().collect::<BTreeSet<_>>() != svs {
        return Err("OBS raw SV labels and decoded signals differ".into());
    }
    let parse_report = nav.nav_parse_report();
    println!("parse=ok epoch={t} obs_svs={} nav_eph_decoded={} nav_rejected={} nav_unsupported={} policy=Reject target=Wgs84 options=default catalog={}",
        svs.len(), nav.nav_ephemeris_frames_iter().count(), parse_report.rejected_records(),
        parse_report.unsupported_records(), FrameTransformer.catalog_version());
    if show_candidates {
        for diagnostic in parse_report.diagnostics() {
            println!("  parse_diagnostic={diagnostic:?}");
        }
    }
    let mut totals = BTreeMap::<String, usize>::new();
    let mut changed_at_tx = Vec::new();
    let mut compare_mismatches = Vec::new();
    for (sv, label) in labels {
        let report = nav.nav_select_ephemeris(sv, t, UnknownHealthPolicy::Reject);
        if let Some(ref full) = original {
            let full_report = full.nav_select_ephemeris(sv, t, UnknownHealthPolicy::Reject);
            if selected_key(&report) != selected_key(&full_report) {
                compare_mismatches.push(format!(
                    "{label}: fixture={:?} original={:?}",
                    selected_key(&report),
                    selected_key(&full_report)
                ));
            }
            for candidate in &report.candidates {
                let counterpart = full_report
                    .candidates
                    .iter()
                    .find(|other| other.key == candidate.key);
                if counterpart.map(|other| other.rejection) != Some(candidate.rejection) {
                    compare_mismatches.push(format!(
                        "{label}: candidate={:?} fixture_rejection={:?} original_rejection={:?}",
                        candidate.key,
                        candidate.rejection,
                        counterpart.map(|other| other.rejection)
                    ));
                }
            }
        }
        let mut rejections = BTreeMap::<String, usize>::new();
        for candidate in &report.candidates {
            if let Some(reason) = candidate.rejection {
                *rejections.entry(format!("{reason:?}")).or_default() += 1;
            }
            if show_candidates {
                println!(
                    "  candidate sv={label} msg={:?} toc={} toe={:?} health={:?} rejection={:?}",
                    candidate.key.msgtype,
                    candidate.clock_reference,
                    candidate.orbit_reference,
                    candidate.ephemeris.orbits.get("health"),
                    candidate.rejection
                );
            }
        }
        let Some(chosen) = report.chosen() else {
            println!(
                "{label} selection=none candidates={} rejections={rejections:?}",
                report.candidates.len()
            );
            *totals.entry("selection_none".into()).or_default() += 1;
            continue;
        };
        let selected = format!("sv={label} msg={:?} record_epoch={} toc={} toe={:?} health={:?} candidates={} rejections={rejections:?}",
            chosen.key.msgtype, chosen.key.epoch, chosen.clock_reference, chosen.orbit_reference,
            chosen.ephemeris.orbits.get("health"), report.candidates.len());
        if let Some(rho_m) = observations
            .signals
            .iter()
            .find(|signal| {
                signal.sv == sv
                    && signal.observable.is_pseudo_range_observable()
                    && signal.value.is_finite()
                    && signal.value > 0.0
            })
            .map(|signal| signal.value)
        {
            // This is only a reception-minus-pseudorange/c selection sensitivity
            // check. No satellite clock, receiver clock, or atmospheric correction.
            let tx = t - Duration::from_seconds(rho_m / C_M_S);
            let tx_report = nav.nav_select_ephemeris(sv, tx, UnknownHealthPolicy::Reject);
            if selected_key(&report) != selected_key(&tx_report) {
                changed_at_tx.push(format!(
                    "{label}: reception={:?} approximate_tx={:?} dt_s={:.6}",
                    selected_key(&report),
                    selected_key(&tx_report),
                    rho_m / C_M_S
                ));
            }
        }
        let native = match chosen.spatial_state_at(t) {
            Ok(state) => state,
            Err(err) => {
                println!("{selected} propagation_error={err:?}");
                *totals.entry("propagation_error".into()).or_default() += 1;
                continue;
            },
        };
        let state = &native.state;
        let point = match SpatialPoint::from_nav(state) {
            Ok(point) => point,
            Err(err) => {
                println!(
                    "{selected} native_km={:?} point_error={err:?}",
                    state.position_km
                );
                *totals.entry("point_error".into()).or_default() += 1;
                continue;
            },
        };
        match FrameTransformer.to_frame(&point, FrameRequest::Wgs84, TransformOptions::default()) {
            Ok(result) => {
                let status = format!("{:?}", result.position_status());
                *totals.entry(status.clone()).or_default() += 1;
                println!("{selected} native_frame={:?} source={:?} realization={:?} source_evidence={:?} native_km={:?} target=Wgs84 target_km={:?} status={status} target_realization={:?} catalog={} edges={:?} edge_info={:?} caution={:?}",
                    state.native_frame(), state.source(), state.realization(), state.source_evidence(),
                    state.position_km, result.position_km, result.target_realization,
                    result.catalog_version, result.edge_ids, result.edge_info, result.position_accuracy_note);
            },
            Err(err) => {
                *totals.entry(format!("FrameError::{err:?}")).or_default() += 1;
                println!("{selected} native_frame={:?} source={:?} realization={:?} source_evidence={:?} native_km={:?} target=Wgs84 frame_error={err:?}",
                    state.native_frame(), state.source(), state.realization(), state.source_evidence(), state.position_km);
            },
        }
    }
    println!("summary={totals:?}");
    println!(
        "approximate_transmit_time_selection_changes={} (raw pseudorange/c only)",
        changed_at_tx.len()
    );
    for change in &changed_at_tx {
        println!("  {change}");
    }
    if original.is_some() {
        println!("original_selection_mismatches={}", compare_mismatches.len());
        for mismatch in &compare_mismatches {
            println!("  {mismatch}");
        }
        if !compare_mismatches.is_empty() {
            return Err("fixture selection differs from original NAV".into());
        }
    }
    Ok(())
}
