use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthCredentials {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GoogleAccount {
    pub id: String, // email
    pub email: String,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64, // Unix timestamp in seconds
    #[serde(default)]
    pub user_email: Option<String>,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub user_picture: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AccountsStore {
    pub active_account_id: Option<String>,
    pub accounts: Vec<GoogleAccount>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoogleUserInfo {
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

pub fn open_in_browser(url: &str) {
    println!("[GGDown OAuth] Opening browser for URL: {}", url);
    if std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
    {
        return;
    }

    if std::process::Command::new("google-chrome")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
    {
        return;
    }

    if std::process::Command::new("firefox")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
    {
        return;
    }

    let _ = open::that(url);
}

pub struct AuthManager {
    config_dir: PathBuf,
    pub credentials: Arc<Mutex<Option<AuthCredentials>>>,
    pub accounts_store: Arc<Mutex<AccountsStore>>,
}

impl AuthManager {
    pub fn new() -> Self {
        let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        config_dir.push("ggdown");
        let _ = fs::create_dir_all(&config_dir);

        let creds_file = config_dir.join("credentials.json");
        let accounts_file = config_dir.join("accounts.json");
        let tokens_file = config_dir.join("tokens.json");

        let credentials = if creds_file.exists() {
            fs::read_to_string(&creds_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };

        let mut accounts_store: AccountsStore = if accounts_file.exists() {
            fs::read_to_string(&accounts_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else if tokens_file.exists() {
            // Migrate single legacy token
            #[derive(Deserialize)]
            struct LegacyTokenData {
                access_token: String,
                refresh_token: Option<String>,
                expires_at: u64,
                user_email: Option<String>,
                user_name: Option<String>,
                user_picture: Option<String>,
            }

            if let Ok(content) = fs::read_to_string(&tokens_file) {
                if let Ok(legacy) = serde_json::from_str::<LegacyTokenData>(&content) {
                    let email = legacy
                        .user_email
                        .clone()
                        .unwrap_or_else(|| "account@gmail.com".to_string());
                    let acc = GoogleAccount {
                        id: email.clone(),
                        email: email.clone(),
                        name: legacy.user_name.clone(),
                        picture: legacy.user_picture.clone(),
                        access_token: legacy.access_token,
                        refresh_token: legacy.refresh_token,
                        expires_at: legacy.expires_at,
                        user_email: legacy.user_email,
                        user_name: legacy.user_name,
                        user_picture: legacy.user_picture,
                    };
                    AccountsStore {
                        active_account_id: Some(email),
                        accounts: vec![acc],
                    }
                } else {
                    AccountsStore::default()
                }
            } else {
                AccountsStore::default()
            }
        } else {
            AccountsStore::default()
        };

        // Ensure active_account_id is valid if accounts exist
        if accounts_store.active_account_id.is_none() && !accounts_store.accounts.is_empty() {
            accounts_store.active_account_id = Some(accounts_store.accounts[0].id.clone());
        }

        Self::sync_tokens_file_dir(&config_dir, &accounts_store);

        Self {
            config_dir,
            credentials: Arc::new(Mutex::new(credentials)),
            accounts_store: Arc::new(Mutex::new(accounts_store)),
        }
    }

    fn sync_tokens_file(&self, store: &AccountsStore) {
        Self::sync_tokens_file_dir(&self.config_dir, store);
    }

    fn sync_tokens_file_dir(config_dir: &std::path::Path, store: &AccountsStore) {
        let tokens_file = config_dir.join("tokens.json");
        let active = if let Some(ref id) = store.active_account_id {
            store.accounts.iter().find(|a| a.id == *id)
        } else {
            store.accounts.first()
        };

        if let Some(acc) = active {
            #[derive(Serialize)]
            struct LegacyToken {
                access_token: String,
                refresh_token: Option<String>,
                expires_at: u64,
                user_email: Option<String>,
                user_name: Option<String>,
                user_picture: Option<String>,
            }

            let legacy = LegacyToken {
                access_token: acc.access_token.clone(),
                refresh_token: acc.refresh_token.clone(),
                expires_at: acc.expires_at,
                user_email: Some(acc.email.clone()),
                user_name: acc.name.clone(),
                user_picture: acc.picture.clone(),
            };

            let _ = fs::write(tokens_file, serde_json::to_string_pretty(&legacy).unwrap_or_default());
        } else if tokens_file.exists() {
            let _ = fs::remove_file(tokens_file);
        }
    }

    pub async fn save_credentials(&self, creds: AuthCredentials) -> Result<(), String> {
        let file = self.config_dir.join("credentials.json");
        let json = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| e.to_string())?;
        *self.credentials.lock().await = Some(creds);
        Ok(())
    }

    pub async fn get_accounts(&self) -> AccountsStore {
        self.accounts_store.lock().await.clone()
    }

    pub async fn get_active_account(&self) -> Option<GoogleAccount> {
        let store = self.accounts_store.lock().await;
        if let Some(ref active_id) = store.active_account_id {
            store.accounts.iter().find(|a| a.id == *active_id).cloned()
        } else {
            store.accounts.first().cloned()
        }
    }

    pub async fn set_active_account(&self, account_id: &str) -> Result<(), String> {
        let mut store = self.accounts_store.lock().await;
        if store.accounts.iter().any(|a| a.id == account_id) {
            store.active_account_id = Some(account_id.to_string());
            let file = self.config_dir.join("accounts.json");
            let _ = fs::write(&file, serde_json::to_string_pretty(&*store).unwrap_or_default());
            self.sync_tokens_file(&store);
            Ok(())
        } else {
            Err(format!("Account '{}' not found", account_id))
        }
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), String> {
        let mut store = self.accounts_store.lock().await;
        store.accounts.retain(|a| a.id != account_id);
        if store.active_account_id.as_deref() == Some(account_id) {
            store.active_account_id = store.accounts.first().map(|a| a.id.clone());
        }
        let file = self.config_dir.join("accounts.json");
        let _ = fs::write(&file, serde_json::to_string_pretty(&*store).unwrap_or_default());
        self.sync_tokens_file(&store);
        Ok(())
    }

    pub async fn clear_session(&self) -> Result<(), String> {
        let mut store = self.accounts_store.lock().await;
        store.accounts.clear();
        store.active_account_id = None;
        let file = self.config_dir.join("accounts.json");
        let _ = fs::write(&file, serde_json::to_string_pretty(&*store).unwrap_or_default());
        let tokens_file = self.config_dir.join("tokens.json");
        if tokens_file.exists() {
            let _ = fs::remove_file(tokens_file);
        }
        Ok(())
    }

    pub async fn get_valid_token(&self) -> Result<String, String> {
        self.get_valid_token_for(None).await
    }

    pub async fn get_valid_token_for(&self, account_id: Option<&str>) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut store = self.accounts_store.lock().await;

        let target_id = match account_id {
            Some(id) => id.to_string(),
            None => store
                .active_account_id
                .clone()
                .ok_or_else(|| "No active Google account. Please log in.".to_string())?,
        };

        let account = store
            .accounts
            .iter_mut()
            .find(|a| a.id == target_id)
            .ok_or_else(|| format!("Account '{}' not found", target_id))?;

        // If token expires in less than 60 seconds and refresh token is available, refresh it
        if account.expires_at <= now + 60 {
            if let Some(ref refresh) = account.refresh_token {
                let creds_lock = self.credentials.lock().await;
                let creds = creds_lock.as_ref().ok_or_else(|| "Credentials missing".to_string())?;

                let client = reqwest::Client::new();
                let params = [
                    ("client_id", creds.client_id.as_str()),
                    ("client_secret", creds.client_secret.as_str()),
                    ("refresh_token", refresh.as_str()),
                    ("grant_type", "refresh_token"),
                ];

                let res = client
                    .post("https://oauth2.googleapis.com/token")
                    .form(&params)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to refresh token: {}", e))?;

                if !res.status().is_success() {
                    return Err(format!("Refresh token failed: {}", res.status()));
                }

                let new_token: OAuthTokenResponse = res
                    .json()
                    .await
                    .map_err(|e| format!("Invalid token response: {}", e))?;

                account.access_token = new_token.access_token.clone();
                account.expires_at = now + new_token.expires_in;

                let file = self.config_dir.join("accounts.json");
                let _ = fs::write(&file, serde_json::to_string_pretty(&*store).unwrap_or_default());

                return Ok(new_token.access_token);
            }
        }

        Ok(account.access_token.clone())
    }

    pub async fn start_oauth_flow(
        &self,
        client_id: String,
        client_secret: String,
    ) -> Result<String, String> {
        self.save_credentials(AuthCredentials {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        })
        .await?;

        // Bind ephemeral local loopback port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Could not bind local loopback port: {}", e))?;
        let port = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();

        let redirect_uri = format!("http://127.0.0.1:{}/callback", port);
        // prompt=select_account consent ensures user can pick which account to add
        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=https://www.googleapis.com/auth/drive%20https://www.googleapis.com/auth/userinfo.email%20https://www.googleapis.com/auth/userinfo.profile&access_type=offline&prompt=select_account%20consent&include_granted_scopes=true",
            urlencoding::encode(&client_id),
            urlencoding::encode(&redirect_uri)
        );

        let accounts_store_clone = Arc::clone(&self.accounts_store);
        let config_dir_clone = self.config_dir.clone();

        // Launch system browser
        open_in_browser(&auth_url);

        // Spawn listener in background
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 4096];
                if let Ok(n) = stream.read(&mut buffer).await {
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let mut code = None;

                    if let Some(first_line) = request.lines().next() {
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(url) = Url::parse(&format!("http://127.0.0.1{}", parts[1])) {
                                for (key, value) in url.query_pairs() {
                                    if key == "code" {
                                        code = Some(value.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if let Some(code) = code {
                        let client = reqwest::Client::new();
                        let params = [
                            ("client_id", client_id.as_str()),
                            ("client_secret", client_secret.as_str()),
                            ("code", code.as_str()),
                            ("grant_type", "authorization_code"),
                            ("redirect_uri", redirect_uri.as_str()),
                        ];

                        let token_res = client
                            .post("https://oauth2.googleapis.com/token")
                            .form(&params)
                            .send()
                            .await;

                        let mut success = false;
                        if let Ok(res) = token_res {
                            if let Ok(token_obj) = res.json::<OAuthTokenResponse>().await {
                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();

                                // Fetch user info
                                let user_info_res = client
                                    .get("https://www.googleapis.com/oauth2/v2/userinfo")
                                    .bearer_auth(&token_obj.access_token)
                                    .send()
                                    .await;

                                let mut user_email = None;
                                let mut user_name = None;
                                let mut user_picture = None;

                                if let Ok(u_res) = user_info_res {
                                    if let Ok(u_data) = u_res.json::<GoogleUserInfo>().await {
                                        user_email = u_data.email;
                                        user_name = u_data.name;
                                        user_picture = u_data.picture;
                                    }
                                }

                                let email = user_email
                                    .clone()
                                    .unwrap_or_else(|| format!("account-{}@gmail.com", &token_obj.access_token[..8.min(token_obj.access_token.len())]));
                                let account_id = email.clone();

                                let mut store = accounts_store_clone.lock().await;
                                if let Some(existing) = store.accounts.iter_mut().find(|a| a.id == account_id) {
                                    existing.access_token = token_obj.access_token;
                                    if let Some(ref r) = token_obj.refresh_token {
                                        existing.refresh_token = Some(r.clone());
                                    }
                                    existing.expires_at = now + token_obj.expires_in;
                                    existing.name = user_name.clone();
                                    existing.picture = user_picture.clone();
                                    existing.user_email = Some(email.clone());
                                    existing.user_name = user_name.clone();
                                    existing.user_picture = user_picture.clone();
                                } else {
                                    store.accounts.push(GoogleAccount {
                                        id: account_id.clone(),
                                        email: email.clone(),
                                        name: user_name.clone(),
                                        picture: user_picture.clone(),
                                        access_token: token_obj.access_token,
                                        refresh_token: token_obj.refresh_token,
                                        expires_at: now + token_obj.expires_in,
                                        user_email: Some(email.clone()),
                                        user_name: user_name.clone(),
                                        user_picture: user_picture.clone(),
                                    });
                                }
                                store.active_account_id = Some(account_id);

                                let file = config_dir_clone.join("accounts.json");
                                let _ = fs::write(&file, serde_json::to_string_pretty(&*store).unwrap_or_default());
                                Self::sync_tokens_file_dir(&config_dir_clone, &store);
                                success = true;
                            }
                        }

                        let html_response = if success {
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<!DOCTYPE html><html><head><meta charset='utf-8'><title>GGDown Login</title><style>body{background:#090d16;color:#e2e8f0;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;}.card{background:#1e293b;padding:36px;border-radius:14px;text-align:center;box-shadow:0 10px 25px rgba(0,0,0,0.5);max-width:440px;}h1{color:#38bdf8;margin-bottom:12px;font-size:22px;}p{color:#94a3b8;font-size:14px;line-height:1.6;}</style></head><body><div class='card'><h1>Account Connected!</h1><p>Google Account successfully connected to GGDown. You can close this tab and return to the application.</p></div></body></html>"
                        } else {
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<!DOCTYPE html><html><body><h2>Authentication failed. Unable to connect to Google.</h2></body></html>"
                        };

                        let _ = stream.write_all(html_response.as_bytes()).await;
                        let _ = stream.flush().await;
                    }
                }
            }
        });

        Ok(auth_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auth_manager_load() {
        let manager = AuthManager::new();
        let store = manager.get_accounts().await;
        println!("Loaded accounts: {}", store.accounts.len());
        for acc in &store.accounts {
            println!("Account: {}", acc.email);
        }
    }
}
