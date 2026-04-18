# patreon-dl — Technical Documentation

Version: 0.1.0 | Language: Rust | Binary: `patreon-dl`

## Overview

`patreon-dl` is a command-line tool that downloads media content (images, videos, audio) from Patreon creators you're subscribed to. It uses Patreon's internal JSON:API v1 endpoints with cookie-based authentication — the same API the Patreon web application calls when you browse posts in your browser.

### Why not the official Patreon API v2?

The official Patreon API v2 does **not** support media/attachment access. This is a known limitation that has persisted for years. The API is creator-focused (managing campaigns, members, posts) and provides no subscriber-facing endpoints for downloading patron-only media content. All community tools that download Patreon media (gallery-dl, patreon-dl-node, PatreonDownloader) use the internal API.

## Architecture

```
patreon-dl
├── src/
│   ├── main.rs          CLI entry point (clap argument parsing, orchestration)
│   ├── api.rs           Patreon internal API client (auth, pagination, media extraction)
│   ├── models.rs        Serde types for JSON:API v1 responses
│   └── download.rs      File download, directory organisation, manifest management
└── Cargo.toml
```

### Data Flow

```
1. User provides creator URL + session_id cookie
                    │
2. resolve_campaign()
   ├── Fetch creator page HTML
   ├── Extract campaign_id from embedded JSON
   └── Fallback: query /api/campaigns?filter[vanity]=NAME
                    │
3. all_posts(campaign_id)
   ├── GET /api/posts?filter[campaign_id]=ID&include=attachments,images,media,audio
   ├── Parse JSON:API response (data[] + included[])
   ├── Resolve relationships: post → media items via included array
   ├── Filter: skip posts where current_user_can_view = false
   ├── Paginate via cursor until no more pages
   └── Return Vec<(PostAttributes, Vec<MediaInfo>)>
                    │
4. download_all()
   ├── Create output/CreatorName/ directory
   ├── For each post: create YYYY-MM-DD_Post-Title/ subdirectory
   ├── For each media item: HTTP GET download_url → write to file
   ├── Track progress, skip existing files if --skip-existing
   └── Build manifest data
                    │
5. save_manifest()
   └── Write manifest.json to output/CreatorName/
```

## Patreon Internal API

### Authentication

A single cookie is required: `session_id`. This is extracted from the user's browser after logging into patreon.com.

All API requests include:
- `Cookie: session_id=VALUE`
- `User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ...` (Chrome UA)

### Endpoints Used

#### Campaign Resolution

**Primary method:** Fetch `https://www.patreon.com/{vanity}` and extract `campaign_id` from embedded page data. Two HTML patterns are checked:

1. `"campaign_id":12345` — numeric ID directly in page JSON
2. `"id":"12345","type":"campaign"` — JSON:API resource reference

**Fallback:** `GET https://www.patreon.com/api/campaigns?filter[vanity]={name}&json-api-version=1.0`

#### Post Listing

```
GET https://www.patreon.com/api/posts
  ?include=campaign,attachments,audio,images,media,native_video_insights,user
  &filter[campaign_id]={campaign_id}
  &filter[is_draft]=false
  &sort=-published_at
  &json-api-version=1.0
  &page[count]=20
  &page[cursor]={cursor}          (for subsequent pages)
```

Returns JSON:API v1 format:
```json
{
  "data": [                        // Post resources
    {
      "id": "12345",
      "type": "post",
      "attributes": {
        "title": "Post Title",
        "published_at": "2026-03-01T12:00:00.000+00:00",
        "current_user_can_view": true,
        ...
      },
      "relationships": {
        "images": { "data": [{"id": "111", "type": "media"}] },
        "attachments": { "data": [{"id": "222", "type": "attachment"}] },
        ...
      }
    }
  ],
  "included": [                    // Related resources (media objects live here)
    {
      "id": "111",
      "type": "media",
      "attributes": {
        "download_url": "https://...",
        "file_name": "image.jpg",
        "size_bytes": 1048576,
        "mimetype": "image/jpeg",
        "image_urls": {
          "original": "https://...",
          "default": "https://...",
          "default_small": "https://..."
        }
      }
    }
  ],
  "meta": {
    "pagination": {
      "total": 150,
      "cursors": {
        "next": "CURSOR_STRING"    // null when no more pages
      }
    }
  }
}
```

### JSON:API Resource Resolution

Media objects are **not** embedded in post objects. They're in the top-level `included` array, linked via `relationships`:

1. Post has `relationships.images.data = [{"id": "111", "type": "media"}]`
2. Find matching resource in `included` where `id == "111"` and `type == "media"`
3. Extract `download_url` or `image_urls.original` from that resource's attributes

This is the standard JSON:API v1 "compound document" pattern.

### Media Download URLs

Download URLs are direct HTTPS links to Patreon's CDN. They:
- Contain an embedded auth token in the URL itself
- Are valid for approximately 24 hours
- Do **not** require the session cookie for the actual download
- Support standard HTTP range requests

### Pagination

Cursor-based. The `meta.pagination.cursors.next` field contains an opaque cursor string. When it's `null` or absent, there are no more pages.

### Rate Limiting

No documented rate limits exist for the internal API. The tool implements conservative delays:
- 500ms between API page requests
- 200ms between file downloads

## Source Modules

### `main.rs`

CLI orchestration. Parses arguments via clap derive macros, creates the API client, and chains the pipeline: resolve → fetch → download → manifest.

**Key types:**
- `Cli` — clap-derived argument struct

### `api.rs`

Patreon API client. Handles authentication, campaign resolution, post pagination, and media extraction from JSON:API responses.

**Key types:**
- `PatreonClient` — main client struct holding ureq agent and session cookie
- `MediaInfo` — extracted media metadata (URL, filename, size, mimetype, type)

**Key functions:**
- `resolve_campaign(url)` → `(campaign_id, creator_name)` — resolves creator URL/vanity to campaign ID
- `all_posts(campaign_id, limit)` → `Vec<(PostAttributes, Vec<MediaInfo>)>` — paginated fetch of all posts with media
- `extract_media_for_post(post, response)` — resolves JSON:API relationships to media items from included array

### `models.rs`

Serde types for JSON:API v1. Handles the polymorphic nature of JSON:API where resources have generic `attributes` (deserialized as `serde_json::Value`) that are parsed into typed structs on demand.

**Key types:**
- `ApiResponse` — top-level envelope (`data`, `included`, `meta`)
- `Resource` — generic JSON:API resource with `id`, `type`, `attributes`, `relationships`
- `PostAttributes` — typed post fields (title, published_at, current_user_can_view)
- `MediaAttributes` — typed media fields (download_url, file_name, size_bytes, mimetype, image_urls)
- `Manifest` / `ManifestPost` / `ManifestFile` — download tracking for resume support
- `RelationshipData` — enum handling single, many, or absent relationships

**Key methods:**
- `Resource::parse_attrs<T>()` — deserialize generic attributes into a typed struct
- `Resource::related_ids(rel_name)` — extract related resource references
- `ApiResponse::find_included(type, id)` — look up a resource in the included array

### `download.rs`

File download and directory organisation.

**Key functions:**
- `download_all(creator, posts, output_dir, skip_existing, dry_run, type_filter)` → `Manifest` — main download loop
- `save_manifest(manifest, output_dir)` — persist manifest.json
- `download_file(url, path)` → `u64` (bytes written) — single file download with 5-minute timeout
- `sanitise_filename(name)` — filesystem-safe name (replaces `/\:*?"<>|`, truncates to 200 chars)

## Output Structure

```
output/
└── CreatorName/
    ├── 2026-03-01_Post-Title/
    │   ├── image1.jpg
    │   ├── image2.png
    │   └── video.mp4
    ├── 2026-02-28_Another-Post/
    │   └── audio.mp3
    └── manifest.json
```

### manifest.json

Tracks download state for resume support:

```json
{
  "creator": "CreatorName",
  "campaign_id": "12345",
  "last_updated": "2026-03-03T14:30:00Z",
  "posts": [
    {
      "id": "",
      "title": "Post Title",
      "published_at": "2026-03-01T12:00:00.000+00:00",
      "url": "https://www.patreon.com/posts/12345",
      "files": [
        {
          "filename": "image1.jpg",
          "url": "https://...",
          "size_bytes": 1048576,
          "mimetype": "image/jpeg",
          "downloaded": true
        }
      ]
    }
  ]
}
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4.x | CLI argument parsing with derive macros and env var support |
| `serde` / `serde_json` | 1.x | JSON serialisation/deserialisation for API responses and manifest |
| `ureq` | 3.x | Synchronous HTTP client (API requests and file downloads) |
| `anyhow` | 1.x | Error handling with context chains |
| `chrono` | 0.4.x | Timestamp formatting for manifest |

No async runtime needed — ureq is synchronous, and sequential downloads are deliberate for rate limiting.

## Error Handling

- **Expired session:** API returns 401/403. The tool reports the HTTP error and exits. User must re-extract `session_id` from browser.
- **Locked posts:** Posts where `current_user_can_view = false` are logged and skipped (not an error).
- **Download failures:** Individual file failures are logged but don't stop the batch. The manifest records `downloaded: false` for failed files.
- **Network errors:** ureq surfaces connection/timeout errors via anyhow context chains.
- **Malformed responses:** JSON parse failures are logged per-post and skipped.

## Known Limitations

- **Session expiry:** The `session_id` cookie expires periodically. There is no automatic refresh mechanism — the user must re-extract the cookie from their browser.
- **Cloudflare challenges:** If Patreon serves a Cloudflare challenge page instead of API responses, the tool cannot solve it. This is rare with a valid session cookie.
- **Embedded video:** Videos hosted on external platforms (YouTube, Vimeo) embedded in posts are not downloaded. Only Patreon-hosted media is fetched.
- **No incremental sync:** The tool re-fetches all post metadata on each run. Only file downloads are skipped via `--skip-existing`. A future version could use the manifest to skip already-processed posts entirely.
- **Undocumented API:** The internal API may change without notice. If Patreon modifies their JSON:API response structure, the tool will need updating.
