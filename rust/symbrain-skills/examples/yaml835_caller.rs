//! Real API probe over an externally preseeded isolated root.
use std::path::PathBuf;

use symbrain_skills::install::{
    StatusKind, StatusOptions, SyncOptions, status_with_cache, sync_selected,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("isolated root required")?);
    let setting = args.next().ok_or("cache setting required")?;
    if !root.is_absolute() || args.next().is_some() {
        return Err("absolute root and cache setting required".into());
    }
    let cache = match setting.as_str() {
        "explicit" => Some(root.join("comparison-cache")),
        "empty" => Some(PathBuf::new()),
        "default" => None,
        _ => return Err("unknown cache setting".into()),
    };
    let options = StatusOptions {
        home_dir: root.join("home"),
        library_dir: root.join("library"),
        base_dir: Some(root.join("base")),
        targets: vec!["opencode".into()],
        ..Default::default()
    };
    let rows = status_with_cache(&options, cache.as_deref())?;
    if rows.len() != 1 || rows[0].status != StatusKind::Stale || rows[0].error.is_some() {
        return Err(format!("unexpected status: {rows:?}").into());
    }
    let plans = sync_selected(
        &SyncOptions {
            home_dir: options.home_dir.clone(),
            library_dir: options.library_dir.clone(),
            base_dir: options.base_dir.clone(),
            render_dir: Some(root.join("retained-render")),
            targets: options.targets.clone(),
            dry_run: true,
            ..Default::default()
        },
        &rows,
    )?;
    if plans.len() != 1 || plans[0].action != "planned" || !plans[0].error.is_empty() {
        return Err(format!("unexpected dry run: {plans:?}").into());
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "success":true, "platform":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "status":rows, "dry_run":plans
        }))?
    );
    Ok(())
}
