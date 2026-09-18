use crate::cli::{ExpandArgs, ExpandCommands};
use evenframe_core::error::{EvenframeError, Result};
use evenframe_core::tooling::BuildConfig;
use evenframe_core::tooling::WorkspaceScanner;
use evenframe_core::tooling::expansion_cache::{self, CacheManifest, hash_file};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

pub async fn run(args: ExpandArgs) -> Result<()> {
    match args.command {
        ExpandCommands::Status => status().await,
        ExpandCommands::Warm => warm().await,
        ExpandCommands::Clear => clear().await,
    }
}

async fn status() -> Result<()> {
    let config = BuildConfig::discover()?;
    let target_dir = expansion_cache::find_target_dir(&config.scan_path);
    let expanded_dir = target_dir.join(".evenframe-expanded");

    if !expanded_dir.exists() {
        println!("No expansion cache found.");
        println!("Run `evenframe expand warm` to populate it.");
        return Ok(());
    }

    let mut total_size: u64 = 0;
    let mut total_entries: usize = 0;
    let mut total_hits: usize = 0;
    let mut total_misses: usize = 0;

    let unreadable = |path: &Path, e: std::io::Error| {
        EvenframeError::WorkspaceScan(format!("failed to read {}: {e}", path.display()))
    };
    for entry in fs::read_dir(&expanded_dir).map_err(|e| unreadable(&expanded_dir, e))? {
        let entry = entry.map_err(|e| unreadable(&expanded_dir, e))?;
        let cache_dir = entry.path();
        if !entry
            .file_type()
            .map_err(|e| unreadable(&cache_dir, e))?
            .is_dir()
        {
            continue;
        }
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let Some(manifest) = CacheManifest::load(&cache_dir, &crate_name) else {
            println!("  {crate_name}: no usable manifest (rebuilt on the next scan)");
            continue;
        };

        let mut crate_size: u64 = 0;
        let mut hits = 0;
        let mut misses = 0;

        for (rel_source, cache_entry) in &manifest.entries {
            total_entries += 1;

            let frag_path = cache_dir.join(&cache_entry.fragment_path);
            crate_size += fs::metadata(&frag_path)
                .map_err(|e| unreadable(&frag_path, e))?
                .len();

            let source_path = manifest.src_dir.join(rel_source);
            let is_hit = source_path.exists()
                && match hash_file(&source_path) {
                    Ok(hash) => hash == cache_entry.input_hash,
                    Err(e) => {
                        warn!("Counting {} as stale: {e}", source_path.display());
                        false
                    }
                };

            if is_hit {
                hits += 1;
            } else {
                misses += 1;
            }
        }

        total_size += crate_size;
        total_hits += hits;
        total_misses += misses;

        let entry_count = manifest.entries.len();
        println!(
            "  {crate_name}: {entry_count} files ({hits} current, {misses} stale), {size}",
            size = format_bytes(crate_size),
        );
    }

    println!();
    println!(
        "Total: {total_entries} files, {total_hits} current, {total_misses} stale, {}",
        format_bytes(total_size),
    );

    Ok(())
}

async fn warm() -> Result<()> {
    let config = BuildConfig::discover()?;

    info!("Warming expansion cache for all workspace crates");
    println!("Warming expansion cache...");

    let scanner = WorkspaceScanner::with_path(config.scan_path, config.apply_aliases, true)
        .with_extra_files(config.include_files);
    let types = scanner.scan_for_evenframe_types()?;

    println!("Cache warmed: {} types discovered.", types.len());
    Ok(())
}

async fn clear() -> Result<()> {
    let config = BuildConfig::discover()?;
    let target_dir = expansion_cache::find_target_dir(&config.scan_path);
    let expanded_dir = target_dir.join(".evenframe-expanded");

    if expanded_dir.exists() {
        let size = dir_size(&expanded_dir);
        fs::remove_dir_all(&expanded_dir)?;
        println!("Cleared expansion cache ({}).", format_bytes(size));
    } else {
        println!("No expansion cache to clear.");
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
