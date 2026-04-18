//! patreon-dl — Download media content from Patreon creators you subscribe to.
//!
//! Uses Patreon's internal API with cookie-based authentication.
//! Requires a valid `session_id` cookie from an active browser session.

mod api;
mod download;
mod models;

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Parser;

/// Download media content from Patreon creators you subscribe to.
#[derive(Parser)]
#[command(name = "patreon-dl", version, about)]
struct Cli {
    /// Creator page URL (e.g. https://www.patreon.com/CreatorName) or vanity name.
    creator: String,

    /// Patreon session_id cookie from your browser.
    ///
    /// To obtain: log into patreon.com → DevTools → Application → Cookies → session_id
    #[arg(short, long, env = "PATREON_SESSION_ID")]
    session_id: String,

    /// Output directory.
    #[arg(short, long, default_value = "./output")]
    output: PathBuf,

    /// Maximum number of posts to download.
    #[arg(short, long)]
    limit: Option<usize>,

    /// Skip files that already exist on disk.
    #[arg(long)]
    skip_existing: bool,

    /// List media without downloading.
    #[arg(long)]
    dry_run: bool,

    /// Filter media types (comma-separated: image,video,audio).
    #[arg(long, value_delimiter = ',')]
    types: Option<Vec<String>>,

    /// Verbose output.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.session_id.is_empty() {
        bail!("Session ID is required. Set --session-id or PATREON_SESSION_ID env var.\n\
               To obtain: log into patreon.com → DevTools → Application → Cookies → session_id");
    }

    // 1. Create API client
    let client = api::PatreonClient::new(cli.session_id);

    // 2. Resolve creator → campaign ID
    eprintln!("Resolving campaign for: {}", cli.creator);
    let (campaign_id, creator_name) = client.resolve_campaign(&cli.creator)?;
    eprintln!("Campaign: {creator_name} (ID: {campaign_id})");

    // 3. Fetch all posts
    eprintln!("Fetching posts...");
    let posts = client.all_posts(&campaign_id, cli.limit)?;

    if posts.is_empty() {
        eprintln!("No accessible posts found.");
        return Ok(());
    }

    // Count total media
    let total_media: usize = posts.iter().map(|(_, m)| m.len()).sum();
    eprintln!("Found {} posts with {} media files", posts.len(), total_media);

    if total_media == 0 {
        eprintln!("No media files found in posts.");
        return Ok(());
    }

    eprintln!();

    // 4. Download media
    let mut manifest = download::download_all(
        &creator_name,
        &posts,
        &cli.output,
        cli.skip_existing,
        cli.dry_run,
        &cli.types,
    )?;

    // Fill in campaign_id
    manifest.campaign_id = campaign_id;

    // 5. Save manifest
    if !cli.dry_run {
        download::save_manifest(&manifest, &cli.output)?;
    }

    Ok(())
}
