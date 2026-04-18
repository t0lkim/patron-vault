# Changelog

## v0.1.0 — 2026-03-03

Initial release.

### Added

- Cookie-based authentication using `session_id` from browser session
- Campaign resolution from creator URL, vanity name, or full URL
- Paginated post fetching via Patreon's internal JSON:API v1 endpoints
- Media extraction from all attachment types (images, video, audio, files)
- Organised output structure: `output/CreatorName/YYYY-MM-DD_Post-Title/`
- `manifest.json` per creator for download tracking and resume support
- `--dry-run` mode to list media without downloading
- `--skip-existing` for resuming interrupted downloads
- `--types` filter (image, video, audio)
- `--limit` to cap number of posts processed
- Rate limiting between API requests (500ms) and downloads (200ms)
- Human-readable progress output with file sizes
