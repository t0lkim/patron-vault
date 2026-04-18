# PatronVault

Download and archive media content from Patreon creators you subscribe to. Cookie-based authentication against Patreon's internal JSON:API.

## Usage

```bash
patron-vault https://www.patreon.com/CreatorName --cookie "session_id=abc123"
```

## Features

- Campaign resolution with HTML + API fallback
- Paginated post enumeration via JSON:API v1 compound documents
- Media extraction from relationship-linked included resources
- Date-prefixed directory structure (`YYYY-MM-DD_Post-Title/`)
- Manifest-based resume for interrupted downloads
- Conservative rate limiting (500ms API, 200ms downloads)

## Language

Rust

## License

MIT
