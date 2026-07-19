use routekit::{RouteElement, RouteKit};

const TEST_DB_PATH: &str = "raw_data/e_dfd_PMDG.s3db";

const KLAX_ZSSS_ROUTE: &str = "KLAX CHATY EHF SAC AUGEY AKN SPY CREMR PINTT RULOY ONEIL NYMPH NUZAN NRKEY NIPPI NOGAL NUBDA NANNO NODAN ASTER SDE ELDAK GTC BEGSA NESKO SAMON SUGNO MIHOU BESMU OLTUN STOUT DGC ISAKY ASOVO CAMAS POTET ONIKU NIRAT PONIK SADLI LAMEN AKARA DUMET IPRAG PUD JTN ZSSS";

#[test]
fn test_klax_zsss_waypoint_only_route_resolves_without_unknown() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).expect("init routekit");
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
