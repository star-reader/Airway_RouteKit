use routekit::{RouteElement, RouteKit};

const TEST_DB_PATH: &str = "raw_data/e_dfd_PMDG.s3db";

fn find_resolved_waypoint(parsed: &routekit::ParsedRoute, identifier: &str) -> Option<routekit::Waypoint> {
    for element in &parsed.elements {
        match element {
            RouteElement::Waypoint(wp) if wp.identifier == identifier => return Some(wp.clone()),
            RouteElement::Direct { from, to } => {
                if from.identifier == identifier {
                    return Some(from.clone());
                }
                if to.identifier == identifier {
                    return Some(to.clone());
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn test_real_db_chi_prefers_local_candidate() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).expect("init routekit");
    let parsed = kit
        .parse_route("ZYHB HRB NODAL CHI VENOS ZUCK")
        .expect("parse route");

    let chi = find_resolved_waypoint(&parsed, "CHI").expect("CHI should be resolved");
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

#[test]
fn test_real_db_rejects_far_gorpi_and_tosas_on_shanghai_route() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).expect("init routekit");
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
    assert_eq!(pk.icao_code, "ZS", "PK should resolve to local ZS candidate");
}
