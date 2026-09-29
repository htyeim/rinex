//! Read a supported NAV record, propagate it, and request an ITRF2014 position.
//! cargo run --features nav --example nav_frame -- FILE G15 '2024-05-07T02:05:00 GPST'
use rinex::{
    navigation::rinex::{
        selection::UnknownHealthPolicy,
        spatial_state::{FrameId, FrameRequest},
    },
    prelude::{Epoch, Rinex, SV},
};
use std::{error::Error, str::FromStr};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: nav_frame FILE SV EPOCH")?;
    let sv = SV::from_str(&args.next().ok_or("missing SV")?)?;
    let epoch = Epoch::from_str(&args.next().ok_or("missing epoch")?)?;
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
    let native = candidate.spatial_state_at(epoch)?;
    let result = native
        .state
        .to_frame(FrameRequest::Realization(FrameId::Itrf2014))?;
    println!(
        "native_frame={:?} native_km={:?}",
        native.state.native_frame(),
        native.state.position_km
    );
    println!(
        "target={:?} epoch={} position_km={:?}",
        result.target_realization, result.epoch, result.position_km
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
