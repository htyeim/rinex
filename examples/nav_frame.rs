//! Read a supported NAV record, propagate it, and request an ITRF2014 position.
//! cargo run --features nav --example nav_frame -- FILE G15 '2024-05-07T02:05:00 GPST'
use rinex::{
    navigation::rinex::{
        selection::UnknownHealthPolicy,
        spatial_state::{FrameId, FrameRequest, FrameTransformer, SpatialPoint, TransformOptions},
    },
    prelude::{Epoch, Rinex, SV},
};
use std::{error::Error, str::FromStr};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: nav_frame FILE SV EPOCH [--warnings-as-errors]")?;
    let sv = SV::from_str(&args.next().ok_or("missing SV")?)?;
    let epoch = Epoch::from_str(&args.next().ok_or("missing epoch")?)?;
    let strict = match args.next().as_deref() {
        None => false,
        Some("--warnings-as-errors") => true,
        _ => return Err("usage: nav_frame FILE SV EPOCH [--warnings-as-errors]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let nav = if file.ends_with(".gz") {
        Rinex::from_gzip_file(&file)?
    } else {
        Rinex::from_file(&file)?
    };
    let report = nav.nav_select_ephemeris(sv, epoch, UnknownHealthPolicy::Reject);
    let candidate = report
        .chosen()
        .ok_or("no propagatable supported NAV record")?;
    println!(
        "selected={} msgtype: {:?}",
        candidate.key.sv, candidate.key.msgtype
    );
    let native = candidate.spatial_state_at(epoch)?;
    let result = FrameTransformer.to_frame(
        &SpatialPoint::from_nav(&native.state)?,
        FrameRequest::Realization(FrameId::Itrf2014),
        TransformOptions {
            warnings_as_errors: strict,
            ..Default::default()
        },
    )?;
    println!(
        "native_frame={:?} native_km={:?}",
        native.state.native_frame(),
        native.state.position_km
    );
    println!(
        "target={:?} source_realization={:?} epoch={} position_km={:?}",
        result.target_realization, result.source_realization, result.epoch, result.position_km
    );
    println!(
        "source_basis={:?} source_evidence={:?} position_status={:?} method={:?} edges={:?} velocity_km_s={:?}",
        result.source_basis,
        result.source_evidence,
        result.position_status(),
        result.method,
        result.edge_ids,
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
