use routekit::{RouteElement, RouteKit};

const TEST_DB_PATH: &str = "raw_data/e_dfd_PMDG.s3db";

#[test]
fn test_real_db_chi_prefers_local_candidate() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).expect("init routekit");
    let parsed = kit
        .parse_route("ZYHB HRB NODAL CHI VENOS ZUCK")
        .expect("parse route");

    let chi = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Waypoint(wp) if wp.identifier == "CHI" => Some(wp.clone()),
        _ => None,
    });

    let chi = chi.expect("CHI should be resolved");
    assert_eq!(chi.icao_code, "ZY", "CHI should resolve to local ZY candidate");
    assert!(chi.coordinate.latitude > 0.0, "should not jump to southern hemisphere candidate");
}

#[test]
fn test_real_db_long_route_no_unknown_for_valid_vor_tokens() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).expect("init routekit");
    let route = "ZYHB HRB PABKI PAGDO ISBOP NULRA LJB LEMOT NUBKI BIDIB ISKEM TOSID NODAL CHI VENOS UDETI UKDUM MUDAM BUMDU SOTMU VYK AVBOX BEKDO LEBUN ORODO VADMO TAMIX NUNGA OMLIX LIMGI UPSIK EMVIL SOSNU ENSOX IGITA TOSIM PONEP KIGUM IGDUL AKREB OLNER GUTVI ZUCK";
    let parsed = kit.parse_route(route).expect("parse route");

    let warnings = parsed.warnings.join(" | ");
    assert!(
        !warnings.contains("无法识别的元素: HRB")
            && !warnings.contains("无法识别的元素: LJB")
            && !warnings.contains("无法识别的元素: VYK")
            && !warnings.contains("无法识别的元素: CHI"),
        "unexpected warnings: {}",
        warnings
    );
}
