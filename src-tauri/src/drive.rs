use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(rename = "iconLink")]
    pub icon_link: Option<String>,
    #[serde(default)]
    pub is_folder: bool,
}

#[derive(Debug, Deserialize)]
struct DriveFileListResponse {
    files: Option<Vec<DriveFile>>,
    #[serde(rename = "nextPageToken")]
    _next_page_token: Option<String>,
}

pub struct DriveManager {
    client: reqwest::Client,
}

impl DriveManager {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn auth_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
            headers.insert(AUTHORIZATION, val);
        }
        headers
    }

    pub async fn list_files(
        &self,
        token: &str,
        parent_id: Option<String>,
        search_query: Option<String>,
    ) -> Result<Vec<DriveFile>, String> {
        let parent = parent_id.unwrap_or_else(|| "root".to_string());
        
        let mut q = format!("'{}' in parents and trashed = false", parent);
        if let Some(search) = search_query {
            if !search.trim().is_empty() {
                q = format!("name contains '{}' and trashed = false", search.replace('\'', "\\'"));
            }
        }

        let url = "https://www.googleapis.com/drive/v3/files";
        let res = self
            .client
            .get(url)
            .headers(Self::auth_headers(token))
            .query(&[
                ("q", q.as_str()),
                ("fields", "files(id, name, mimeType, size, modifiedTime, iconLink)"),
                ("orderBy", "folder,name"),
                ("pageSize", "100"),
            ])
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            if err_text.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT") {
                return Err("Google Drive access permission was not granted. Please sign out and sign in again, making sure to check the box: 'See, edit, create, and delete all of your Google Drive files' on the Google authorization page.".to_string());
            }
            return Err(format!("Drive API error: {}", err_text));
        }

        let resp_json: DriveFileListResponse = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let files = resp_json.files.unwrap_or_default().into_iter().map(|mut f| {
            f.is_folder = f.mime_type == "application/vnd.google-apps.folder";
            f
        }).collect();

        Ok(files)
    }

    pub async fn create_folder(
        &self,
        token: &str,
        name: &str,
        parent_id: Option<String>,
    ) -> Result<DriveFile, String> {
        let parent = parent_id.unwrap_or_else(|| "root".to_string());
        let body = serde_json::json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [parent]
        });

        let res = self
            .client
            .post("https://www.googleapis.com/drive/v3/files")
            .headers(Self::auth_headers(token))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !res.status().is_success() {
            return Err(format!("Failed to create folder: {}", res.status()));
        }

        let mut file: DriveFile = res.json().await.map_err(|e| e.to_string())?;
        file.is_folder = true;
        Ok(file)
    }

    pub async fn delete_file(&self, token: &str, file_id: &str) -> Result<(), String> {
        let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
        let res = self
            .client
            .delete(&url)
            .headers(Self::auth_headers(token))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !res.status().is_success() {
            return Err(format!("Failed to delete: {}", res.status()));
        }

        Ok(())
    }

    pub async fn rename_file(&self, token: &str, file_id: &str, new_name: &str) -> Result<(), String> {
        let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
        let body = serde_json::json!({
            "name": new_name
        });

        let res = self
            .client
            .patch(&url)
            .headers(Self::auth_headers(token))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !res.status().is_success() {
            return Err(format!("Failed to rename: {}", res.status()));
        }

        Ok(())
    }
}

