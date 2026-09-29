//! The documented region definitions (README, "Region definitions"),
//! at their boundaries.

#![allow(clippy::unwrap_used)]

use jobhunt_eligibility::geo::{Area, Membership, Region, country, lookup_name, parse_places};
use jobhunt_eligibility::zones::{Clock, reference_date};

fn m(region: Region, code: &str) -> Membership {
    region.contains(country(code).unwrap())
}

#[test]
fn north_america_latam_and_mexico() {
    use Membership::{Maybe, No, Yes};
    assert_eq!(m(Region::NorthAmerica, "US"), Yes);
    assert_eq!(m(Region::NorthAmerica, "CA"), Yes);
    assert_eq!(m(Region::NorthAmerica, "MX"), Maybe);
    assert_eq!(m(Region::NorthAmerica, "GT"), No);
    assert_eq!(m(Region::LatinAmerica, "MX"), Yes);
    assert_eq!(m(Region::LatinAmerica, "US"), No);
    assert_eq!(m(Region::LatinAmerica, "BR"), Yes);
    assert_eq!(m(Region::LatinAmerica, "CU"), Yes);
    // Not Spanish- or Portuguese-speaking: disputed.
    for code in ["BZ", "GY", "SR", "JM", "TT"] {
        assert_eq!(m(Region::LatinAmerica, code), Maybe, "{code}");
    }
    for code in ["US", "CA", "MX", "BR", "JM", "PA"] {
        assert_eq!(m(Region::Americas, code), Yes, "{code}");
    }
    assert_eq!(m(Region::Americas, "ES"), No);
}

#[test]
fn central_and_south_america() {
    use Membership::{Maybe, No, Yes};
    for code in ["BZ", "CR", "SV", "GT", "HN", "NI", "PA"] {
        assert_eq!(m(Region::CentralAmerica, code), Yes, "{code}");
        assert_eq!(m(Region::SouthAmerica, code), No, "{code}");
    }
    assert_eq!(m(Region::CentralAmerica, "MX"), Maybe);
    for code in [
        "AR", "BO", "BR", "CL", "CO", "EC", "GY", "PY", "PE", "SR", "UY", "VE",
    ] {
        assert_eq!(m(Region::SouthAmerica, code), Yes, "{code}");
    }
    assert_eq!(m(Region::SouthAmerica, "MX"), No);
}

#[test]
fn europe_eu_and_eea_are_different() {
    use Membership::{Maybe, No, Yes};
    assert_eq!(m(Region::Europe, "GB"), Yes);
    assert_eq!(m(Region::Europe, "CH"), Yes);
    assert_eq!(m(Region::Europe, "TR"), Maybe);
    assert_eq!(m(Region::EuropeanUnion, "DE"), Yes);
    assert_eq!(
        m(Region::EuropeanUnion, "GB"),
        Maybe,
        "EU is often used loosely"
    );
    assert_eq!(m(Region::EuropeanUnion, "NO"), Maybe);
    assert_eq!(m(Region::EuropeanUnion, "BR"), No);
    assert_eq!(m(Region::Eea, "NO"), Yes);
    assert_eq!(m(Region::Eea, "IS"), Yes);
    assert_eq!(m(Region::Eea, "LI"), Yes);
    assert_eq!(m(Region::Eea, "CH"), No, "EEA is formal");
    assert_eq!(m(Region::Eea, "GB"), No);
    assert_eq!(lookup_name("EEA"), Some(Area::Region(Region::Eea)));
    assert_eq!(
        lookup_name("European Union"),
        Some(Area::Region(Region::EuropeanUnion))
    );
}

#[test]
fn emea_and_apac() {
    use Membership::{Maybe, No, Yes};
    for code in ["DE", "GB", "TR", "AE", "IL", "ZA", "NG", "EG", "KE"] {
        assert_eq!(m(Region::Emea, code), Yes, "{code}");
    }
    assert_eq!(m(Region::Emea, "IN"), No);
    assert_eq!(m(Region::Emea, "US"), No);
    for code in ["JP", "SG", "AU", "NZ", "IN", "ID", "KR"] {
        assert_eq!(m(Region::Apac, code), Yes, "{code}");
    }
    for code in ["PK", "BD", "LK", "NP"] {
        assert_eq!(m(Region::Apac, code), Maybe, "{code}");
    }
    assert_eq!(m(Region::Apac, "AE"), No);
    assert_eq!(m(Region::Asia, "IL"), Maybe);
}

#[test]
fn regions_have_codes_and_zones() {
    for r in Region::ALL {
        assert!(!r.code().is_empty());
        let zone = r.zone().unwrap_or_else(|| panic!("{r:?} has zones"));
        let (lo, hi) = zone.year_range();
        assert!(lo <= hi, "{r:?}");
    }
    // Europe from the Azores to Ukraine, on IANA rules: the Azores are
    // UTC-1 in winter and UTC+0 in summer.
    let europe = Region::Europe.zone().unwrap();
    assert_eq!(europe.at(reference_date(1, 15).unwrap()).0, -60);
    assert_eq!(europe.at(reference_date(7, 15).unwrap()).0, 0);
    assert!(
        europe
            .clocks()
            .contains(&Clock::iana("Europe/Lisbon").unwrap())
    );
    assert!(
        !europe
            .clocks()
            .contains(&Clock::iana("Europe/Moscow").unwrap())
    );
    let places = parse_places("Remote - EMEA");
    assert_eq!(places[0].area, Some(Area::Region(Region::Emea)));
    assert!(places[0].remote);
    assert_eq!(Area::Country(country("BR").unwrap()).code(), "country:BR");
    assert_eq!(Area::Region(Region::LatinAmerica).code(), "region:latam");
}
