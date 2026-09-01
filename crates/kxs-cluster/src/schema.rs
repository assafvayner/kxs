use kube::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// OpenAPI v3 documents by group/version path, keyed on the content-hashed
/// URL the API server advertised for them.
#[derive(Default)]
pub struct OpenApiCache {
    docs: Mutex<HashMap<String, (String, Arc<str>)>>,
}

#[derive(Deserialize)]
struct Index {
    paths: HashMap<String, IndexEntry>,
}

#[derive(Deserialize)]
struct IndexEntry {
    #[serde(rename = "serverRelativeURL")]
    server_relative_url: String,
}

/// `api/v1` for the core group, `apis/<group>/<version>` otherwise.
pub fn group_version_path(group: &str, version: &str) -> String {
    if group.is_empty() {
        format!("api/{version}")
    } else {
        format!("apis/{group}/{version}")
    }
}

async fn get_text(client: &Client, url: &str) -> Result<String, String> {
    let req = http::Request::get(url)
        .header(http::header::ACCEPT, "application/json")
        .body(Vec::new())
        .map_err(|e| e.to_string())?;
    client.request_text(req).await.map_err(|e| e.to_string())
}

/// Raw OpenAPI v3 JSON for a group/version, or `None` when the server does
/// not publish one. Re-reads the small index on every call so a changed hash
/// invalidates the cached document.
pub async fn openapi_document(
    client: &Client,
    cache: &OpenApiCache,
    group: &str,
    version: &str,
) -> Result<Option<Arc<str>>, String> {
    let path = group_version_path(group, version);
    let index: Index =
        serde_json::from_str(&get_text(client, "/openapi/v3").await?).map_err(|e| e.to_string())?;
    let Some(entry) = index.paths.get(&path) else {
        return Ok(None);
    };
    let mut docs = cache.docs.lock().await;
    if let Some((url, doc)) = docs.get(&path) {
        if *url == entry.server_relative_url {
            return Ok(Some(doc.clone()));
        }
    }
    let doc: Arc<str> = get_text(client, &entry.server_relative_url).await?.into();
    docs.insert(path, (entry.server_relative_url.clone(), doc.clone()));
    Ok(Some(doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_group_uses_api_prefix() {
        assert_eq!(group_version_path("", "v1"), "api/v1");
    }

    #[test]
    fn named_group_uses_apis_prefix() {
        assert_eq!(group_version_path("apps", "v1"), "apis/apps/v1");
    }
}
