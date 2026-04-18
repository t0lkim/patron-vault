//! Serde types for Patreon's internal JSON:API v1 responses.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =========================================================================
// Top-level JSON:API envelope
// =========================================================================

#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    pub data: Vec<Resource>,
    #[serde(default)]
    pub included: Vec<Resource>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Deserialize)]
pub struct SingleApiResponse {
    pub data: Resource,
    #[serde(default)]
    pub included: Vec<Resource>,
}

// =========================================================================
// JSON:API resource (polymorphic — post, media, campaign, etc.)
// =========================================================================

#[derive(Debug, Deserialize)]
pub struct Resource {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
    #[serde(default)]
    pub relationships: HashMap<String, Relationship>,
}

#[derive(Debug, Deserialize)]
pub struct Relationship {
    #[serde(default)]
    pub data: RelationshipData,
}

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum RelationshipData {
    Single(ResourceRef),
    Many(Vec<ResourceRef>),
    #[default]
    None,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ResourceRef {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
}

// =========================================================================
// Pagination
// =========================================================================

#[derive(Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub pagination: Pagination,
}

#[derive(Debug, Default, Deserialize)]
pub struct Pagination {
    #[serde(default)]
    pub cursors: Option<Cursors>,
    #[serde(default)]
    pub total: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct Cursors {
    pub next: Option<String>,
}

// =========================================================================
// Typed attribute structs (extracted from serde_json::Value)
// =========================================================================

#[derive(Debug, Deserialize)]
pub struct PostAttributes {
    pub title: Option<String>,
    pub published_at: Option<String>,
    pub post_type: Option<String>,
    pub url: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub current_user_can_view: bool,
}

#[derive(Debug, Deserialize)]
pub struct MediaAttributes {
    pub download_url: Option<String>,
    pub image_urls: Option<ImageUrls>,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub mimetype: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImageUrls {
    pub original: Option<String>,
    pub default: Option<String>,
    pub default_small: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CampaignAttributes {
    pub name: Option<String>,
    pub vanity: Option<String>,
    pub url: Option<String>,
}

// =========================================================================
// Manifest (for resume support)
// =========================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub creator: String,
    pub campaign_id: String,
    pub last_updated: String,
    pub posts: Vec<ManifestPost>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestPost {
    pub id: String,
    pub title: String,
    pub published_at: String,
    pub url: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestFile {
    pub filename: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub mimetype: Option<String>,
    pub downloaded: bool,
}

// =========================================================================
// Helper methods
// =========================================================================

impl Resource {
    /// Parse attributes as a typed struct.
    pub fn parse_attrs<T: serde::de::DeserializeOwned>(&self) -> anyhow::Result<T> {
        serde_json::from_value(self.attributes.clone())
            .map_err(|e| anyhow::anyhow!("Failed to parse {}/{} attributes: {e}", self.resource_type, self.id))
    }

    /// Get related resource IDs for a given relationship name.
    pub fn related_ids(&self, rel_name: &str) -> Vec<ResourceRef> {
        match self.relationships.get(rel_name) {
            Some(rel) => match &rel.data {
                RelationshipData::Single(r) => vec![r.clone()],
                RelationshipData::Many(refs) => refs.clone(),
                RelationshipData::None => vec![],
            },
            None => vec![],
        }
    }
}

impl ApiResponse {
    /// Find an included resource by type and id.
    pub fn find_included(&self, resource_type: &str, id: &str) -> Option<&Resource> {
        self.included
            .iter()
            .find(|r| r.resource_type == resource_type && r.id == id)
    }
}
