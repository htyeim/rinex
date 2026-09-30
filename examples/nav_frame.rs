//! Read a supported NAV record, propagate it, and request a target frame.
//! cargo run --features nav --example nav_frame -- FILE G15 '2024-05-07T02:05:00 GPST' --target wgs84
use rinex::{
    navigation::rinex::{
        selection::UnknownHealthPolicy,
        spatial_state::{FrameId, FrameRequest, FrameTransformer, SpatialPoint, TransformOptions},
    },
    prelude::{Epoch, Rinex, SV},
};
use std::{error::Error, io::Write, str::FromStr};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: nav_frame FILE SV EPOCH [--target wgs84|g2296|itrf2020|itrf2014] [--warnings-as-errors]")?;
    let sv = SV::from_str(&args.next().ok_or("missing SV")?)?;
    let epoch = Epoch::from_str(&args.next().ok_or("missing epoch")?)?;
    let mut strict = false;
    let mut target = FrameRequest::Realization(FrameId::Itrf2014);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--warnings-as-errors" => strict = true,
            "--target" => {
                target = match args.next().as_deref() {
                    Some("wgs84") => FrameRequest::Wgs84,
                    Some("g2296") => FrameRequest::Realization(FrameId::Wgs84G2296),
                    Some("itrf2020") => FrameRequest::Realization(FrameId::Itrf2020),
                    Some("itrf2014") => FrameRequest::Realization(FrameId::Itrf2014),
                    _ => {
                        return Err("unknown target; use wgs84, g2296, itrf2020, or itrf2014".into())
                    },
                };
            },
            _ => return Err(format!("unexpected argument: {arg}").into()),
        }
    }
    let nav = if file.ends_with(".gz") {
        Rinex::from_gzip_file(&file)?
    } else {
        Rinex::from_file(&file)?
    };
    let report = nav.nav_select_ephemeris(sv, epoch, UnknownHealthPolicy::Reject);
    let candidate = report.chosen().ok_or_else(|| {
        for candidate in &report.candidates {
            eprintln!(
                "candidate={:?} toc={} toe={:?} rejection={:?}",
                candidate.key.msgtype,
                candidate.clock_reference,
                candidate.orbit_reference,
                candidate.rejection
            );
        }
        "no propagatable supported NAV record"
    })?;
    println!(
        "selected={} msgtype: {:?}",
        candidate.key.sv, candidate.key.msgtype
    );
    let native = candidate.spatial_state_at(epoch)?;
    println!(
        "toc={} toe={:?} native_frame={:?} source={:?} source_realization={:?} source_evidence={:?} native_km={:?} request={target:?}",
        candidate.clock_reference,
        candidate.orbit_reference,
        native.state.native_frame(),
        native.state.source(),
        native.state.realization(),
        native.state.source_evidence(),
        native.state.position_km
    );
    std::io::stdout().flush()?;
    let result = FrameTransformer
        .to_frame(
            &SpatialPoint::from_nav(&native.state)?,
            target,
            TransformOptions {
                warnings_as_errors: strict,
                ..Default::default()
            },
        )
        .map_err(|err| {
            eprintln!("frame_error={err:?}");
            err
        })?;
    println!(
        "target={:?} source_realization={:?} epoch={} position_km={:?}",
        result.target_realization, result.source_realization, result.epoch, result.position_km
    );
    println!(
        "source_basis={:?} source_evidence={:?} position_status={:?} method={:?} catalog={} edges={:?} edge_info={:?} velocity_km_s={:?}",
        result.source_basis,
        result.source_evidence,
        result.position_status(),
        result.method,
        result.catalog_version,
        result.edge_ids,
        result.edge_info,
        result.velocity_km_s
    );
    if let Some(note) = result.position_accuracy_note {
        println!("{note}");
    }
    if let Some(assumption) = result.assumption {
        println!("assumption_id={} scope={}", assumption.id, assumption.scope);
    }
    Ok(())
}
