//! Patreon internal API client using cookie-based authentication.

use anyhow::{Context, Result};
use std::time::Duration;

use crate::models::*;

/// Client for Patreon's internal JSON:API v1 endpoints.
pub struct PatreonClient {
    agent: ureq::Agent,
    session_id: String,
}

/// Include parameter for the posts endpoint — requests all media types.
const POST_INCLUDES: &str =
    "campaign,attachments,audio,images,media,native_video_insights,user";

impl PatreonClient {
    pub fn new(session_id: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .new_agent();

        Self { agent, session_id }
    }

    /// Resolve a creator URL to a campaign ID.
    ///
    /// Accepts URLs like:
    /// - `https://www.patreon.com/CreatorName`
    /// - `patreon.com/CreatorName`
    /// - `CreatorName` (vanity name only)
    pub fn resolve_campaign(&self, creator_url: &str) -> Result<(String, String)> {
        let vanity = extract_vanity(creator_url);
        eprintln!("[api] Resolving campaign for vanity: {vanity}");

        // Fetch the creator's page and look for campaign data in the HTML
        let page_url = format!("https://www.patreon.com/{vanity}");
        let resp = self
            .agent
            .get(&page_url)
            .header("Cookie", &format!("session_id={}", self.session_id))
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .call()
            .context("Failed to fetch creator page")?;

        let body = resp.into_body().read_to_string()?;

        // Look for campaign ID in the page HTML (embedded in JSON data)
        // Pattern: "campaign_id":12345 or "id":"12345","type":"campaign"
        if let Some(campaign_id) = extract_campaign_id_from_html(&body) {
            let name = vanity.to_string();
            eprintln!("[api] Found campaign ID: {campaign_id} for {name}");
            return Ok((campaign_id, name));
        }

        // Fallback: try the API with the vanity name
        let api_url = format!(
            "https://www.patreon.com/api/campaigns?filter[vanity]={vanity}&json-api-version=1.0"
        );
        let resp: SingleApiResponse = self
            .agent
            .get(&api_url)
            .header("Cookie", &format!("session_id={}", self.session_id))
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .call()
            .context("Failed to query campaigns API")?
            .into_body()
            .read_json()
            .context("Failed to parse campaigns response")?;

        let attrs: CampaignAttributes = resp.data.parse_attrs()?;
        let name = attrs.name.unwrap_or_else(|| vanity.to_string());
        Ok((resp.data.id, name))
    }

    /// Fetch a single page of posts for a campaign.
    pub fn fetch_posts_page(
        &self,
        campaign_id: &str,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<ApiResponse> {
        let mut url = format!(
            "https://www.patreon.com/api/posts\
             ?include={POST_INCLUDES}\
             &filter[campaign_id]={campaign_id}\
             &filter[is_draft]=false\
             &sort=-published_at\
             &json-api-version=1.0\
             &page[count]={page_size}"
        );

        if let Some(c) = cursor {
            url.push_str(&format!("&page[cursor]={c}"));
        }

        let resp: ApiResponse = self
            .agent
            .get(&url)
            .header("Cookie", &format!("session_id={}", self.session_id))
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .call()
            .with_context(|| format!("Failed to fetch posts (cursor: {cursor:?})"))?
            .into_body()
            .read_json()
            .context("Failed to parse posts response")?;

        Ok(resp)
    }

    /// Iterate through all posts for a campaign, handling pagination.
    pub fn all_posts(&self, campaign_id: &str, limit: Option<usize>) -> Result<Vec<(PostAttributes, Vec<MediaInfo>)>> {
        let mut all_posts = Vec::new();
        let mut cursor: Option<String> = None;
        let page_size = 20u32;
        let mut page = 0u32;

        loop {
            page += 1;
            eprintln!("[api] Fetching page {page}...");

            let resp = self.fetch_posts_page(campaign_id, cursor.as_deref(), page_size)?;

            for post in &resp.data {
                if post.resource_type != "post" {
                    continue;
                }

                let attrs: PostAttributes = match post.parse_attrs() {
                    Ok(a) => a,
                    Err(e) => {
                        eprintln!("[api] Warning: skipping post {}: {e}", post.id);
                        continue;
                    }
                };

                if !attrs.current_user_can_view {
                    let title = attrs.title.as_deref().unwrap_or("(untitled)");
                    eprintln!("[api] Skipping locked post: {title}");
                    continue;
                }

                let media = extract_media_for_post(post, &resp);
                all_posts.push((attrs, media));

                if let Some(lim) = limit {
                    if all_posts.len() >= lim {
                        eprintln!("[api] Reached limit of {lim} posts");
                        return Ok(all_posts);
                    }
                }
            }

            // Check for next page
            cursor = resp
                .meta
                .pagination
                .cursors
                .and_then(|c| c.next);

            if cursor.is_none() {
                break;
            }

            // Rate limit: small delay between pages
            std::thread::sleep(Duration::from_millis(500));
        }

        eprintln!("[api] Fetched {} posts across {page} pages", all_posts.len());
        Ok(all_posts)
    }
}

/// Structured media info extracted from the API response.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub download_url: String,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub mimetype: Option<String>,
    pub media_type: String, // "attachment", "media", "image", etc.
}

/// Extract media items linked to a post from the included resources.
fn extract_media_for_post(post: &Resource, response: &ApiResponse) -> Vec<MediaInfo> {
    let mut media = Vec::new();

    // Check all relationship types that can contain media
    for rel_name in &["attachments", "audio", "images", "media"] {
        let refs = post.related_ids(rel_name);
        for r in refs {
            if let Some(included) = response.find_included(&r.resource_type, &r.id) {
                if let Ok(attrs) = included.parse_attrs::<MediaAttributes>() {
                    // Prefer download_url, fall back to image_urls.original
                    let url = attrs
                        .download_url
                        .or_else(|| attrs.image_urls.as_ref().and_then(|u| u.original.clone()));

                    if let Some(url) = url {
                        media.push(MediaInfo {
                            download_url: url,
                            file_name: attrs.file_name,
                            size_bytes: attrs.size_bytes,
                            mimetype: attrs.mimetype,
                            media_type: rel_name.to_string(),
                        });
                    }
                }
            }
        }
    }

    media
}

/// Extract vanity name from a Patreon URL or bare name.
fn extract_vanity(input: &str) -> &str {
    let s = input
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_start_matches("patreon.com/");
    // Remove trailing slashes and query strings
    s.split(&['/', '?'][..]).next().unwrap_or(s)
}

/// Try to extract campaign_id from Patreon page HTML.
fn extract_campaign_id_from_html(html: &str) -> Option<String> {
    // Pattern 1: "campaign_id":12345
    if let Some(idx) = html.find("\"campaign_id\":") {
        let rest = &html[idx + 15..];
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let id = &rest[..end];
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    // Pattern 2: "id":"12345","type":"campaign"
    let pattern = "\"type\":\"campaign\"";
    if let Some(type_idx) = html.find(pattern) {
        // Look backward for "id":"..."
        let before = &html[..type_idx];
        if let Some(id_start) = before.rfind("\"id\":\"") {
            let rest = &before[id_start + 6..];
            if let Some(end) = rest.find('"') {
                let id = &rest[..end];
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }

    None
}
