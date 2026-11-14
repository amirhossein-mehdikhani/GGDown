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
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64, // Unix timestamp in seconds
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub user_picture: Option<String>,
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
    pub token_data: Arc<Mutex<Option<TokenData>>>,
}

impl AuthManager {
    pub fn new() -> Self {
        let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        config_dir.push("ggdown");
        let _ = fs::create_dir_all(&config_dir);

        let creds_file = config_dir.join("credentials.json");
        let tokens_file = config_dir.join("tokens.json");

        let credentials = if creds_file.exists() {
            fs::read_to_string(&creds_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };

        let token_data = if tokens_file.exists() {
            fs::read_to_string(&tokens_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };

        Self {
            config_dir,
            credentials: Arc::new(Mutex::new(credentials)),
            token_data: Arc::new(Mutex::new(token_data)),
        }
    }

    pub async fn save_credentials(&self, creds: AuthCredentials) -> Result<(), String> {
        let file = self.config_dir.join("credentials.json");
        let json = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| e.to_string())?;
        *self.credentials.lock().await = Some(creds);
        Ok(())
    }

    pub async fn save_token(&self, token: TokenData) -> Result<(), String> {
        let file = self.config_dir.join("tokens.json");
        let json = serde_json::to_string_pretty(&token).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| e.to_string())?;
        *self.token_data.lock().await = Some(token);
        Ok(())
    }

    pub async fn clear_session(&self) -> Result<(), String> {
        let tokens_file = self.config_dir.join("tokens.json");
        if tokens_file.exists() {
            let _ = fs::remove_file(tokens_file);
        }
        *self.token_data.lock().await = None;
        Ok(())
    }

    pub async fn get_valid_token(&self) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut token_lock = self.token_data.lock().await;
        let token = token_lock.as_mut().ok_or_else(|| "Not logged in".to_string())?;

        // If token expires in less than 60 seconds and refresh token is available, refresh it
        if token.expires_at <= now + 60 {
            if let Some(ref refresh) = token.refresh_token {
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

                token.access_token = new_token.access_token.clone();
                token.expires_at = now + new_token.expires_in;

                let file = self.config_dir.join("tokens.json");
                let _ = fs::write(&file, serde_json::to_string_pretty(&token).unwrap_or_default());

                return Ok(new_token.access_token);
            }
        }

        Ok(token.access_token.clone())
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
        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=https://www.googleapis.com/auth/drive%20https://www.googleapis.com/auth/userinfo.email%20https://www.googleapis.com/auth/userinfo.profile&access_type=offline&prompt=consent&include_granted_scopes=true",
            urlencoding::encode(&client_id),
            urlencoding::encode(&redirect_uri)
        );

        let token_data_clone = Arc::clone(&self.token_data);
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

                                let data = TokenData {
                                    access_token: token_obj.access_token,
                                    refresh_token: token_obj.refresh_token,
                                    expires_at: now + token_obj.expires_in,
                                    user_email,
                                    user_name,
                                    user_picture,
                                };

                                let file = config_dir_clone.join("tokens.json");
                                let _ = fs::write(&file, serde_json::to_string_pretty(&data).unwrap_or_default());
                                *token_data_clone.lock().await = Some(data);
                                success = true;
                            }
                        }

                        let html_response = if success {
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<!DOCTYPE html><html><head><meta charset='utf-8'><title>GGDown Login</title><style>body{background:#090d16;color:#e2e8f0;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;}.card{background:#1e293b;padding:36px;border-radius:14px;text-align:center;box-shadow:0 10px 25px rgba(0,0,0,0.5);max-width:440px;}h1{color:#38bdf8;margin-bottom:12px;font-size:22px;}p{color:#94a3b8;font-size:14px;line-height:1.6;}</style></head><body><div class='card'><h1>Authentication Successful!</h1><p>Google Drive has been connected. You can now close this tab and return to the GGDown application.</p></div></body></html>"
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

