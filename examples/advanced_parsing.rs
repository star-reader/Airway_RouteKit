use routekit::RouteKit;

fn main() -> routekit::Result<()> {
    let mut args = std::env::args().skip(1);
    let database = args
        .next()
        .unwrap_or_else(|| "raw_data/e_navdata.s3db".into());
    let input = args.collect::<Vec<_>>().join(" ");
    let input = if input.is_empty() {
        "ZBAA ELKUR W40 YQG W142 DALIM A593 VMB W161 SASAN ZSPD".to_owned()
    } else {
        input
    };
    let kit = RouteKit::new(database)?;
    let parsed = kit.parse_route(&input)?;
    // 调用成功后仍需读取 is_valid；warnings 和 errors 是独立的诊断。
    println!("{}", serde_json::to_string_pretty(&parsed)?);
    Ok(())
}
