//! 使用真实导航数据库的可选回归测试；显式运行但缺少数据库时会失败。

#[path = "support/route.rs"]
mod route;

use route::find_resolved_waypoint;
use routekit::{RouteElement, RouteKit};
use std::path::PathBuf;

fn real_database() -> RouteKit {
    let path = std::env::var_os("ROUTEKIT_TEST_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("raw_data/e_dfd_PMDG.s3db")
        });
    assert!(
        path.is_file(),
        "navigation database missing: {}; set ROUTEKIT_TEST_DB to an existing database",
        path.display()
    );
    RouteKit::new(&path)
        .unwrap_or_else(|error| panic!("cannot initialize {}: {error}", path.display()))
}

#[test]
#[ignore = "requires a navigation database; run cargo test --test real_database -- --ignored"]
fn test_real_db_chi_prefers_local_candidate() {
    let kit = real_database();
    let parsed = kit
        .parse_route("ZYHB HRB NODAL CHI VENOS ZUCK")
        .expect("parse route");

    let chi = find_resolved_waypoint(&parsed, "CHI").expect("CHI should be resolved");
    assert_eq!(
        chi.icao_code, "ZY",
        "CHI should resolve to local ZY candidate"
    );
    assert!(
        chi.coordinate.latitude > 0.0,
        "should not jump to southern hemisphere candidate"
    );
}

#[test]
#[ignore = "requires a navigation database; run cargo test --test real_database -- --ignored"]
fn test_real_db_long_route_no_unknown_for_valid_vor_tokens() {
    let kit = real_database();
    let route = "ZYHB HRB PABKI PAGDO ISBOP NULRA LJB LEMOT NUBKI BIDIB ISKEM TOSID NODAL CHI VENOS UDETI UKDUM MUDAM BUMDU SOTMU VYK AVBOX BEKDO LEBUN ORODO VADMO TAMIX NUNGA OMLIX LIMGI UPSIK EMVIL SOSNU ENSOX IGITA TOSIM PONEP KIGUM IGDUL AKREB OLNER GUTVI ZUCK";
    let parsed = kit.parse_route(route).expect("parse route");

    for identifier in ["HRB", "LJB", "VYK", "CHI"] {
        assert!(
            find_resolved_waypoint(&parsed, identifier).is_some(),
            "{identifier} should resolve; warnings: {:?}",
            parsed.warnings
        );
    }
}

#[test]
#[ignore = "requires a navigation database; run cargo test --test real_database -- --ignored"]
fn test_real_db_rejects_far_gorpi_and_tosas_on_shanghai_route() {
    let kit = real_database();
    let route = "ZYHB BUBDI HRB PABKI PAGDO ISBOP NULRA LJB LEMOT NUBKI BIDIB ISKEM TOSID NODAL CHI VENOS UDETI DOBGA ORIXA TEKAM FD XIVID TAO XDX AVLOK IDVEL VEVED GORPI ODULO UDOXI HSH TOSAS PK JTN ZSSS";
    let parsed = kit.parse_route(route).expect("parse route");

    let gorpi = find_resolved_waypoint(&parsed, "GORPI");
    assert!(
        gorpi.is_none(),
        "GORPI should be skipped when only distant European candidate exists, got {:?}",
        gorpi
    );

    let tosas = find_resolved_waypoint(&parsed, "TOSAS");
    assert!(
        tosas.is_none(),
        "TOSAS should be skipped when only distant equatorial candidate exists, got {:?}",
        tosas
    );

    let pk = find_resolved_waypoint(&parsed, "PK").expect("PK should still resolve near Shanghai");
    assert_eq!(
        pk.icao_code, "ZS",
        "PK should resolve to local ZS candidate"
    );
}

const KLAX_ZSSS_ROUTE: &str = "KLAX CHATY EHF SAC AUGEY AKN SPY CREMR PINTT RULOY ONEIL NYMPH NUZAN NRKEY NIPPI NOGAL NUBDA NANNO NODAN ASTER SDE ELDAK GTC BEGSA NESKO SAMON SUGNO MIHOU BESMU OLTUN STOUT DGC ISAKY ASOVO CAMAS POTET ONIKU NIRAT PONIK SADLI LAMEN AKARA DUMET IPRAG PUD JTN ZSSS";

#[test]
#[ignore = "requires a navigation database; run cargo test --test real_database -- --ignored"]
fn test_klax_zsss_waypoint_only_route_resolves_without_unknown() {
    let kit = real_database();
    let parsed = kit.parse_route(KLAX_ZSSS_ROUTE).expect("parse route");

    let unknowns: Vec<_> = parsed
        .elements
        .iter()
        .filter_map(|e| match e {
            RouteElement::Unknown(id) => Some(id.clone()),
            _ => None,
        })
        .collect();

    assert!(
        unknowns.is_empty(),
        "waypoint-only route should not produce unknown elements, got {:?}; warnings: {:?}",
        unknowns,
        parsed.warnings
    );

    let direct_count = parsed
        .elements
        .iter()
        .filter(|e| matches!(e, RouteElement::Direct { .. }))
        .count();
    assert!(
        direct_count > 0,
        "consecutive waypoints without airways should produce implicit DCT segments"
    );
}
