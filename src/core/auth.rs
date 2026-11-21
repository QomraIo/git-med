use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

/// Credential entry stored for a specific server origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCredential {
    pub username: String,
    pub token: String,
    pub token_type: String,
    pub created_at: String,
}

/// Persistent credential store file format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialStore {
    #[serde(default)]
    pub servers: HashMap<String, ServerCredential>,
}

/// Resolve the path to the credentials file (`~/.git-med/credentials` or `$GIT_MED_CREDENTIALS_FILE`).
pub fn get_credentials_path() -> PathBuf {
    if let Ok(custom) = env::var("GIT_MED_CREDENTIALS_FILE") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom.trim());
        }
    }

    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".git-med").join("credentials")
}

/// Load credential store from disk.
pub fn load_credentials() -> CredentialStore {
    let path = get_credentials_path();
    if !path.is_file() {
        return CredentialStore::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => CredentialStore::default(),
    }
}

/// Save credential store to disk.
pub fn save_credentials(store: &CredentialStore) -> Result<(), String> {
    let path = get_credentials_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create credentials directory: {}", e))?;
    }

    let json = serde_json::to_string_pretty(store)
        .map_err(|e| format!("failed to serialize credentials: {}", e))?;

    fs::write(&path, json).map_err(|e| format!("failed to write credentials file: {}", e))?;

    Ok(())
}

/// Extract base server origin from URL (e.g. `http://localhost:3000/alice/repo.git` -> `http://localhost:3000`).
pub fn extract_server_origin(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(pos) = trimmed.find("://") {
        let after_scheme = &trimmed[pos + 3..];
        let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
        format!("{}://{}", &trimmed[..pos], host_port)
    } else {
        trimmed.split('/').next().unwrap_or(trimmed).to_string()
    }
}

/// Resolve Bearer token for a given remote URL.
///
/// Precedence:
/// 1. Environment variable `GIT_MED_TOKEN` or `GIT_MED_AUTH_TOKEN`
/// 2. Stored credential matching normalized server origin
/// 3. Stored credential matching full URL prefix
pub fn get_token_for_url(url: &str) -> Option<String> {
    // 1. Check environment variables
    if let Ok(tok) = env::var("GIT_MED_TOKEN").or_else(|_| env::var("GIT_MED_AUTH_TOKEN")) {
        let trimmed = tok.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let store = load_credentials();
    let origin = extract_server_origin(url);

    // 2. Exact origin match
    if let Some(cred) = store.servers.get(&origin) {
        return Some(cred.token.clone());
    }

    // 3. Normalized trailing slash match
    let origin_slash = format!("{}/", origin.trim_end_matches('/'));
    if let Some(cred) = store.servers.get(&origin_slash) {
        return Some(cred.token.clone());
    }

    // 4. Prefix match
    for (srv, cred) in &store.servers {
        if url.starts_with(srv) || srv.starts_with(&origin) {
            return Some(cred.token.clone());
        }
    }

    None
}

/// Save a token credential for a specific server origin.
pub fn save_token_for_server(
    server_url: &str,
    username: &str,
    token: &str,
    token_type: &str,
) -> Result<(), String> {
    let mut store = load_credentials();
    let origin = extract_server_origin(server_url);

    store.servers.insert(
        origin,
        ServerCredential {
            username: username.to_string(),
            token: token.to_string(),
            token_type: token_type.to_string(),
            created_at: chrono_now_iso(),
        },
    );

    save_credentials(&store)
}

/// Remove stored credentials for a specific server origin.
pub fn remove_token_for_server(server_url: &str) -> Result<bool, String> {
    let mut store = load_credentials();
    let origin = extract_server_origin(server_url);

    let removed = store.servers.remove(&origin).is_some()
        || store.servers.remove(&format!("{}/", origin)).is_some();

    save_credentials(&store)?;
    Ok(removed)
}

fn chrono_now_iso() -> String {
    // Standard timestamp string without requiring chrono dependency in client
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix-{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}

// ==================== CLI Commands ====================

#[derive(Debug, Deserialize)]
struct LoginResponsePayload {
    status: Option<String>,
    token_type: Option<String>,
    access_token: Option<String>,
    message: Option<String>,
    error: Option<String>,
}

/// Handler for `git-med login [options]`.
pub fn login(args: &[String]) {
    let mut server = String::new();
    let mut username = String::new();
    let mut password = String::new();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--server" | "-s" if i + 1 < args.len() => {
                server = args[i + 1].clone();
                i += 2;
            }
            "--username" | "-u" if i + 1 < args.len() => {
                username = args[i + 1].clone();
                i += 2;
            }
            "--password" | "-p" if i + 1 < args.len() => {
                password = args[i + 1].clone();
                i += 2;
            }
            other if other.starts_with("--server=") => {
                server = other.strip_prefix("--server=").unwrap().to_string();
                i += 1;
            }
            other if other.starts_with("--username=") => {
                username = other.strip_prefix("--username=").unwrap().to_string();
                i += 1;
            }
            other if other.starts_with("--password=") => {
                password = other.strip_prefix("--password=").unwrap().to_string();
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if server.is_empty() {
        print!("Server [http://localhost:3000]: ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() && !input.trim().is_empty() {
            server = input.trim().to_string();
        } else {
            server = "http://localhost:3000".to_string();
        }
    }

    if username.is_empty() {
        print!("Username: ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            username = input.trim().to_string();
        }
    }

    if username.is_empty() {
        eprintln!("fatal: username cannot be empty");
        std::process::exit(1);
    }

    if password.is_empty() {
        print!("Password: ");
        let _ = io::stdout().flush();
        match rpassword::read_password() {
            Ok(pass) => password = pass.trim().to_string(),
            Err(_) => {
                let mut input = String::new();
                let _ = io::stdin().read_line(&mut input);
                password = input.trim().to_string();
            }
        }
    }

    if password.is_empty() {
        eprintln!("fatal: password cannot be empty");
        std::process::exit(1);
    }

    println!("==> Authenticating with {}...", server);

    let server_clean = server.trim_end_matches('/').to_string();
    let login_url = format!("{}/api/v1/auth/login", server_clean);
    let u_login = username.clone();

    let res = crate::core::block_on(async move {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "username": u_login,
            "password": password,
            "server": server_clean
        });
        let body_bytes = serde_json::to_vec(&payload)
            .map_err(|e| format!("Serialization error: {}", e))?;

        let resp = match client
            .post(&login_url)
            .header("Content-Type", "application/json")
            .body(body_bytes)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return Err(format!("Network connection error: {}", e)),
        };

        let status = resp.status();
        let resp_bytes = resp.bytes().await.map_err(|e| e.to_string())?;

        let parsed: LoginResponsePayload = serde_json::from_slice(&resp_bytes)
            .map_err(|_| format!("Invalid response from server: {}", String::from_utf8_lossy(&resp_bytes)))?;

        if status.is_success() {
            if let Some(token) = parsed.access_token {
                let token_type = parsed.token_type.unwrap_or_else(|| "Bearer".to_string());
                Ok((token, token_type))
            } else {
                Err("Missing access token in server response".to_string())
            }
        } else {
            let msg = parsed
                .message
                .or(parsed.error)
                .unwrap_or_else(|| format!("HTTP status {}", status));
            Err(msg)
        }
    });

    match res {
        Ok((token, token_type)) => {
            if let Err(e) = save_token_for_server(&server, &username, &token, &token_type) {
                eprintln!("warning: failed to save credentials to disk: {}", e);
            } else {
                let creds_file = get_credentials_path();
                println!("  [PASS] Successfully logged in as {}!", username);
                println!("  Credentials saved to {}", creds_file.display());
            }
        }
        Err(e) => {
            eprintln!("  [FAIL] Login failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Handler for `git-med logout [options]`.
pub fn logout(args: &[String]) {
    let mut server = "http://localhost:3000".to_string();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--server" | "-s" if i + 1 < args.len() => {
                server = args[i + 1].clone();
                i += 2;
            }
            other if other.starts_with("--server=") => {
                server = other.strip_prefix("--server=").unwrap().to_string();
                i += 1;
            }
            _ => i += 1,
        }
    }

    match remove_token_for_server(&server) {
        Ok(true) => println!("Logged out from {}", server),
        Ok(false) => println!("No stored credentials found for {}", server),
        Err(e) => eprintln!("Error removing credentials: {}", e),
    }
}

/// Handler for `git-med whoami [options]`.
pub fn whoami(args: &[String]) {
    let mut server = "http://localhost:3000".to_string();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--server" | "-s" if i + 1 < args.len() => {
                server = args[i + 1].clone();
                i += 2;
            }
            other if other.starts_with("--server=") => {
                server = other.strip_prefix("--server=").unwrap().to_string();
                i += 1;
            }
            _ => i += 1,
        }
    }

    let origin = extract_server_origin(&server);
    let store = load_credentials();

    if let Some(cred) = store.servers.get(&origin).or_else(|| store.servers.get(&format!("{}/", origin))) {
        println!("Logged in as: {} ({})", cred.username, origin);
    } else {
        println!("Not logged in to {}", origin);
    }
}
