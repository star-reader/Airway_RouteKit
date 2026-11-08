use routekit::RouteElement;

pub fn find_resolved_waypoint<'a>(
    parsed: &'a routekit::ParsedRoute,
    identifier: &str,
) -> Option<&'a routekit::Waypoint> {
    for element in &parsed.elements {
        match element {
            RouteElement::Waypoint(wp) if wp.identifier == identifier => return Some(wp),
            RouteElement::Direct { from, to } => {
                if from.identifier == identifier {
                    return Some(from);
                }
                if to.identifier == identifier {
                    return Some(to);
                }
            }
            _ => {}
        }
    }
    None
}
