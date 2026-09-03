//! Discord OAuth2 サインイン(標準的な認可コードフロー + PKCE)。
//!
//! Discordはデバイスコードフローに対応していないため、PKCE付きの標準的な認可コードフローを
//! ローカルループバックHTTPサーバでのリダイレクト受信と組み合わせて実装する。
//! Discord Developer Portal でのアプリ登録(client_id / (任意)client_secret / redirect_uri)が
//! 別途必要。サインイン後、Discordユーザーの所属サーバー情報を
//! `train_launcher_server_api::TrainApiClient` 経由でTRAiNバックエンドに問い合わせる想定
//! (TODO: TRAiN API仕様確定後に実装)。
//!
//! このクレートはUI(Tauri)に依存しないよう設計されているため、認可URLが構築できた時点で
//! 戻り値ではなくコールバック `on_authorize_url` を呼び出す。呼び出し側(Tauri command層)は
//! このコールバック内でシステムブラウザを開く想定。

use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, TokenUrl};
use oauth2::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenResponse,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config::DiscordConfig;
use crate::AuthError;

/// Discord OAuth2 のエンドポイント。
pub const DISCORD_AUTH_URL: &str = "https://discord.com/api/oauth2/authorize";
pub const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const DISCORD_USER_URL: &str = "https://discord.com/api/users/@me";

/// 認可コードのリダイレクト待ちのタイムアウト(ユーザーがブラウザで操作を放棄した場合に備える)。
const REDIRECT_WAIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Discordサインインで得られるトークンとユーザー情報。
#[derive(Debug, Clone)]
pub struct DiscordToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// アクセストークンの有効期限(UNIX epoch秒)。
    pub expires_at: Option<i64>,
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
struct DiscordUserResponse {
    id: String,
    username: String,
}

/// Discord OAuth2の認可コードフロー(PKCE)でサインインを行う。
///
/// `on_authorize_url` は認可URLが構築できた時点で一度だけ呼び出される。呼び出し側は
/// ここでシステムブラウザを開いてユーザーにDiscordの認可画面を表示する。
pub async fn sign_in<F>(
    config: &DiscordConfig,
    on_authorize_url: F,
) -> Result<DiscordToken, AuthError>
where
    F: FnOnce(String) + Send,
{
    let redirect_uri = config.redirect_uri();

    let mut client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(AuthUrl::new(DISCORD_AUTH_URL.to_string()).expect("static URL is valid"))
        .set_token_uri(TokenUrl::new(DISCORD_TOKEN_URL.to_string()).expect("static URL is valid"))
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri)
                .map_err(|err| AuthError::Oauth(format!("invalid redirect uri: {err}")))?,
        );
    if let Some(secret) = &config.client_secret {
        client = client.set_client_secret(ClientSecret::new(secret.clone()));
    }

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (authorize_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("identify".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // ブラウザにリダイレクトされるより前にリスナーを起動しておく(取りこぼし防止)。
    let listener = TcpListener::bind(("127.0.0.1", config.callback_port)).await?;

    on_authorize_url(authorize_url.to_string());

    let code = tokio::time::timeout(
        REDIRECT_WAIT_TIMEOUT,
        accept_redirect(&listener, csrf_token.secret()),
    )
    .await
    .map_err(|_| AuthError::Oauth("サインインがタイムアウトしました".to_string()))??;

    let http_client = oauth2::reqwest::Client::new();
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&http_client)
        .await
        .map_err(|err| AuthError::Oauth(err.to_string()))?;

    let expires_at = token.expires_in().map(|duration| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (now + duration).as_secs() as i64
    });

    let access_token = token.access_token().secret().to_string();
    let user = fetch_discord_user(&access_token).await?;

    Ok(DiscordToken {
        access_token,
        refresh_token: token.refresh_token().map(|t| t.secret().to_string()),
        expires_at,
        user_id: user.id,
        username: user.username,
    })
}

async fn fetch_discord_user(access_token: &str) -> Result<DiscordUserResponse, AuthError> {
    reqwest::Client::new()
        .get(DISCORD_USER_URL)
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<DiscordUserResponse>()
        .await
        .map_err(AuthError::from)
}

/// ローカルループバックサーバで1回だけリダイレクトを受け付け、`code`/`state` クエリ
/// パラメータを取り出す。生のHTTP/1.1リクエストラインのみをパースする最小実装であり、
/// 汎用的なHTTPサーバとしての堅牢性は意図していない(あくまでOAuth2リダイレクト受信専用)。
async fn accept_redirect(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<String, AuthError> {
    let (mut stream, _) = listener.accept().await?;

    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 8192 {
            break;
        }
    }

    let request = String::from_utf8_lossy(&buf);
    let request_line = request.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    let url = oauth2::url::Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|err| AuthError::Oauth(format!("invalid redirect request: {err}")))?;

    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }

    let (body, success) = match (&code, &state) {
        (Some(_), Some(s)) if s == expected_state => (
            "<html><body><h1>TRAiN Launcherへのサインインが完了しました</h1>\
             <p>このタブを閉じてランチャーに戻ってください。</p></body></html>",
            true,
        ),
        (Some(_), Some(_)) => (
            "<html><body><h1>サインインに失敗しました</h1><p>不正なリクエストです(state不一致)。</p></body></html>",
            false,
        ),
        _ => (
            "<html><body><h1>サインインに失敗しました</h1><p>認可コードを受け取れませんでした。</p></body></html>",
            false,
        ),
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    if !success {
        return match (code, state) {
            (Some(_), Some(_)) => Err(AuthError::StateMismatch),
            _ => Err(AuthError::Oauth(
                "リダイレクトに認可コードが含まれていません".to_string(),
            )),
        };
    }

    Ok(code.expect("checked above"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uri_matches_configured_port() {
        let config = DiscordConfig {
            client_id: "id".to_string(),
            client_secret: None,
            callback_port: 12345,
        };
        assert_eq!(config.redirect_uri(), "http://127.0.0.1:12345/callback");
    }
}
