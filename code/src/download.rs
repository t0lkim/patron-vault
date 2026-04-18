//! Media download and file organisation.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::api::MediaInfo;
use crate::models::*;

/// Download media files for a list of posts.
pub fn download_all(
    creator: &str,
    posts: &[(PostAttributes, Vec<MediaInfo>)],
    output_dir: &Path,
    skip_existing: bool,
    dry_run: bool,
    type_filter: &Option<Vec<String>>,
) -> Result<Manifest> {
    let creator_dir = output_dir.join(sanitise_filename(creator));
    if !dry_run {
        fs::create_dir_all(&creator_dir)
            .with_context(|| format!("Failed to create output directory: {}", creator_dir.display()))?;
    }

    let total_posts = posts.len();
    let mut manifest_posts = Vec::new();
    let mut total_downloaded = 0u64;
    let mut total_skipped = 0u64;
    let mut total_bytes = 0u64;

    for (i, (attrs, media_items)) in posts.iter().enumerate() {
        let title = attrs.title.as_deref().unwrap_or("untitled");
        let published = attrs.published_at.as_deref().unwrap_or("unknown-date");
        let date_prefix = &published[..10.min(published.len())]; // YYYY-MM-DD

        let post_dir_name = format!("{}_{}", date_prefix, sanitise_filename(title));
        let post_dir = creator_dir.join(&post_dir_name);

        // Filter media by type if specified
        let filtered_media: Vec<&MediaInfo> = media_items
            .iter()
            .filter(|m| match type_filter {
                Some(types) => {
                    let mime = m.mimetype.as_deref().unwrap_or("");
                    types.iter().any(|t| match t.as_str() {
                        "image" => mime.starts_with("image/"),
                        "video" => mime.starts_with("video/"),
                        "audio" => mime.starts_with("audio/"),
                        _ => true,
                    })
                }
                None => true,
            })
            .collect();

        if filtered_media.is_empty() {
            continue;
        }

        eprintln!(
            "[{}/{}] {} — {} file(s)",
            i + 1,
            total_posts,
            title,
            filtered_media.len()
        );

        if !dry_run {
            fs::create_dir_all(&post_dir)
                .with_context(|| format!("Failed to create post directory: {}", post_dir.display()))?;
        }

        let mut manifest_files = Vec::new();

        for media in &filtered_media {
            let filename = media
                .file_name
                .clone()
                .unwrap_or_else(|| filename_from_url(&media.download_url));

            let file_path = post_dir.join(&filename);

            if dry_run {
                let size_str = media
                    .size_bytes
                    .map(|s| format_bytes(s))
                    .unwrap_or_else(|| "unknown size".to_string());
                let mime = media.mimetype.as_deref().unwrap_or("unknown");
                eprintln!("  → {filename} ({size_str}, {mime})");
                manifest_files.push(ManifestFile {
                    filename: filename.clone(),
                    url: media.download_url.clone(),
                    size_bytes: media.size_bytes,
                    mimetype: media.mimetype.clone(),
                    downloaded: false,
                });
                continue;
            }

            if skip_existing && file_path.exists() {
                eprintln!("  ⏭ {filename} (exists, skipping)");
                total_skipped += 1;
                manifest_files.push(ManifestFile {
                    filename: filename.clone(),
                    url: media.download_url.clone(),
                    size_bytes: media.size_bytes,
                    mimetype: media.mimetype.clone(),
                    downloaded: true,
                });
                continue;
            }

            match download_file(&media.download_url, &file_path) {
                Ok(bytes) => {
                    let size_str = format_bytes(bytes);
                    eprintln!("  ✓ {filename} ({size_str})");
                    total_downloaded += 1;
                    total_bytes += bytes;
                    manifest_files.push(ManifestFile {
                        filename: filename.clone(),
                        url: media.download_url.clone(),
                        size_bytes: Some(bytes),
                        mimetype: media.mimetype.clone(),
                        downloaded: true,
                    });
                }
                Err(e) => {
                    eprintln!("  ✗ {filename}: {e}");
                    manifest_files.push(ManifestFile {
                        filename: filename.clone(),
                        url: media.download_url.clone(),
                        size_bytes: media.size_bytes,
                        mimetype: media.mimetype.clone(),
                        downloaded: false,
                    });
                }
            }

            // Rate limit between downloads
            std::thread::sleep(Duration::from_millis(200));
        }

        manifest_posts.push(ManifestPost {
            id: String::new(), // filled by caller if needed
            title: title.to_string(),
            published_at: published.to_string(),
            url: attrs.url.clone().unwrap_or_default(),
            files: manifest_files,
        });
    }

    if !dry_run {
        eprintln!();
        eprintln!("Done: {total_downloaded} downloaded, {total_skipped} skipped, {} total", format_bytes(total_bytes));
    }

    let manifest = Manifest {
        creator: creator.to_string(),
        campaign_id: String::new(), // filled by caller
        last_updated: chrono::Utc::now().to_rfc3339(),
        posts: manifest_posts,
    };

    Ok(manifest)
}

/// Save manifest to disk.
pub fn save_manifest(manifest: &Manifest, output_dir: &Path) -> Result<()> {
    let creator_dir = output_dir.join(sanitise_filename(&manifest.creator));
    fs::create_dir_all(&creator_dir)?;
    let manifest_path = creator_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(&manifest_path, json)?;
    eprintln!("[manifest] Saved to {}", manifest_path.display());
    Ok(())
}

/// Download a single file.
fn download_file(url: &str, path: &Path) -> Result<u64> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .new_agent();

    let resp = agent
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .call()
        .with_context(|| format!("Failed to download: {url}"))?;

    let buf = resp
        .into_body()
        .read_to_vec()
        .context("Failed to read response body")?;

    let bytes = buf.len() as u64;

    let mut file = fs::File::create(path)
        .with_context(|| format!("Failed to create file: {}", path.display()))?;
    file.write_all(&buf)?;

    Ok(bytes)
}

/// Sanitise a string for use as a filesystem name.
pub fn sanitise_filename(name: &str) -> String {
    let sanitised: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Trim leading/trailing dots and spaces, truncate to 200 chars
    let trimmed = sanitised.trim_matches(|c| c == '.' || c == ' ');
    if trimmed.len() > 200 {
        trimmed[..200].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Extract a filename from a URL.
fn filename_from_url(url: &str) -> String {
    url.split('/')
        .last()
        .and_then(|s| s.split('?').next())
        .unwrap_or("unknown")
        .to_string()
}

/// Format bytes into human-readable form.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Get the path where a manifest would be saved.
pub fn manifest_path(output_dir: &Path, creator: &str) -> PathBuf {
    output_dir.join(sanitise_filename(creator)).join("manifest.json")
}
