//! Times the geography and time-zone layer: the first use (loading the
//! GeoNames subset and building its indexes), parsing a location string
//! not seen before, and reading and judging a time-zone requirement day by
//! day across the reference year:
//! `cargo run --release -p jobhunt-eligibility --example gazetteer_load`.

use std::time::Instant;

use jobhunt_eligibility::geo::{parse_places, resolve_name};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::zones::{day_pairs, zones_in};

fn main() {
    let started = Instant::now();
    let _ = resolve_name("São Paulo");
    let loaded = started.elapsed();

    let texts = [
        "San Francisco, CA",
        "Remote (US)",
        "London, UK; São Paulo, Brazil; Remote (US)",
        "Cambridge, MA",
        "Remote - LATAM",
        "Dourados, MS, Brasil",
    ];
    let started = Instant::now();
    let rounds = 1_000;
    for i in 0..rounds {
        for t in texts {
            // A new string each round, so nothing is remembered.
            std::hint::black_box(parse_places(&format!("{t} {i}")));
        }
    }
    let parse = started.elapsed() / (rounds * texts.len() as u32);

    let started = Instant::now();
    let zone = ProfileFacts::living_in("United States").zone();
    let first_zone = started.elapsed();
    std::hint::black_box(zone);

    let (sao_paulo, _) = ProfileFacts::living_in("São Paulo, Brazil")
        .zone()
        .unwrap_or_else(|| std::process::exit(1));
    let started = Instant::now();
    let rounds = 10_000;
    for _ in 0..rounds {
        for (_, zone) in zones_in("You must overlap at least 4 hours with EST or US hours.") {
            std::hint::black_box(day_pairs(&sao_paulo, &zone));
        }
    }
    let judged = started.elapsed() / rounds;

    println!("first use (load + index): {loaded:?}");
    println!("parse of an unseen location string: {parse:?}");
    println!("first zone of a 29-zone country: {first_zone:?}");
    println!("time-zone sentence read and compared over the year: {judged:?}");
}
