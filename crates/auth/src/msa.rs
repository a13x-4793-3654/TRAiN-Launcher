//! Microsoftアカウント (MSA) OAuth2 認証(認可コードフロー + PKCE)。
//!
//! Xbox Live / Minecraftサービスは個人用Microsoftアカウントのみ対応のため、
//! Entra IDの `consumers` テナントのエンドポイントを使用する。
//!
//! Minecraft公式ランチャーと同様、埋め込みWebViewのポップアップでサインイン画面を表示する
//! UXにするため、リダイレクトURIにはローカルサーバを必要としないパブリッククライアント向けの
//! 特別な値 [`MSA_NATIVE_CLIENT_REDIRECT_URI`] を使用する(実際にはこのURLへのHTTP
//! リクエストは発生させず、WebViewのナビゲーションイベントを監視して `code`/`state`
//! クエリパラメータを横取りする)。
//!
//! このクレートはUI(Tauri)に依存しないよう設計されているため、実際に埋め込みWebViewを開く
//! 処理やナビゲーション監視は呼び出し側(Tauri command層)が担当する。このモジュールが提供する
//! のは (1) 表示すべき認可URLの構築 と (2) 横取りしたcode/stateをトークンへ交換する処理のみ。

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};

use crate::AuthError;

/// Microsoft Entra ID (consumers テナント) の認可コードフロー関連エンドポイント。
pub const MSA_AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const MSA_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

/// パブリッククライアント(ネイティブアプリ)向けの特別なリダイレクトURI。
/// Azure Entra IDのアプリ登録側で「モバイルとデスクトップアプリケーション」の
/// リダイレクトURIとして事前に追加登録しておく必要がある。
pub const MSA_NATIVE_CLIENT_REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";

/// Xbox Live / Minecraftサービスへのサインインに必要なスコープ。
const MSA_SCOPES: &str = "XboxLive.signin offline_access";

/// MSAサインインで得られるMicrosoftアクセストークン。
#[derive(Debug, Clone)]
pub struct MsaToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// アクセストークンの有効期限(UNIX epoch秒)。
    pub expires_at: Option<i64>,
}

/// 埋め込みWebViewで表示すべき認可URLと、コード交換に必要な付随情報(CSRF検証・PKCE検証用)。
pub struct AuthorizationRequest {
    /// 埋め込みWebViewでそのまま開くべきURL。
    pub authorize_url: String,
    pkce_verifier: PkceCodeVerifier,
    csrf_token: CsrfToken,
}

/// 指定のクライアントIDで認可URL(埋め込みWebView用)を構築する。
///
/// 呼び出し側は返り値の `authorize_url` を埋め込みWebViewで開き、
/// [`MSA_NATIVE_CLIENT_REDIRECT_URI`] へのナビゲーションを監視して `code`/`state`
/// クエリパラメータを取得したら [`exchange_authorization_code`] に渡す。
pub fn build_authorization_request(
    config: &crate::config::MicrosoftConfig,
) -> AuthorizationRequest {
    let client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(AuthUrl::new(MSA_AUTH_URL.to_string()).expect("static URL is valid"))
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string()).expect("static URL is valid"))
        .set_redirect_uri(
            RedirectUrl::new(MSA_NATIVE_CLIENT_REDIRECT_URI.to_string())
                .expect("static URL is valid"),
        );

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (authorize_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(MSA_SCOPES.to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    AuthorizationRequest {
        authorize_url: authorize_url.to_string(),
        pkce_verifier,
        csrf_token,
    }
}

/// 埋め込みWebViewのリダイレクトから取得した `code`/`state` を検証し、
/// Microsoftアクセストークンへ交換する。`state` が [`build_authorization_request`] で
/// 発行したCSRFトークンと一致しない場合は [`AuthError::StateMismatch`] を返す。
pub async fn exchange_authorization_code(
    config: &crate::config::MicrosoftConfig,
    request: AuthorizationRequest,
    received_code: String,
    received_state: String,
) -> Result<MsaToken, AuthError> {
    if received_state != *request.csrf_token.secret() {
        return Err(AuthError::StateMismatch);
    }

    let client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(AuthUrl::new(MSA_AUTH_URL.to_string()).expect("static URL is valid"))
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string()).expect("static URL is valid"))
        .set_redirect_uri(
            RedirectUrl::new(MSA_NATIVE_CLIENT_REDIRECT_URI.to_string())
                .expect("static URL is valid"),
        );
    let http_client = oauth2::reqwest::Client::new();

    let token = client
        .exchange_code(AuthorizationCode::new(received_code))
        .set_pkce_verifier(request.pkce_verifier)
        .request_async(&http_client)
        .await
        .map_err(|err| AuthError::Oauth(err.to_string()))?;

    let expires_at = token.expires_in().map(|duration| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (now + duration).as_secs() as i64
    });

    Ok(MsaToken {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token.refresh_token().map(|t| t.secret().to_string()),
        expires_at,
    })
}

/// 保存済みの `refresh_token` を使ってMicrosoftアクセストークンを再取得する。
///
/// MSAのアクセストークンは短命(数十分〜1時間程度)なため、サインイン時に一度だけ
/// 取得したものを使い続けることはできない。起動のたびに本関数で新しいアクセストークンへ
/// 交換し、それを [`crate::xbox::exchange_microsoft_token`] へ渡してMinecraftトークンを
/// 再取得する(公式ランチャーと同様、埋め込みWebViewでの再サインインなしに毎回の起動を
/// 成功させるための処理)。
///
/// Microsoft側は原則としてリフレッシュのたびに新しい `refresh_token` を発行し、古いものは
/// 無効化する(リフレッシュトークンローテーション)。返却値の `refresh_token` を都度
/// 保存し直す必要がある(応答に含まれない場合は渡した値をそのまま引き継ぐ)。
pub async fn refresh_access_token(
    config: &crate::config::MicrosoftConfig,
    refresh_token: &str,
) -> Result<MsaToken, AuthError> {
    let client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(AuthUrl::new(MSA_AUTH_URL.to_string()).expect("static URL is valid"))
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string()).expect("static URL is valid"));
    let http_client = oauth2::reqwest::Client::new();

    let token = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|err| AuthError::Oauth(err.to_string()))?;

    let expires_at = token.expires_in().map(|duration| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (now + duration).as_secs() as i64
    });

    Ok(MsaToken {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|t| t.secret().to_string())
            .or_else(|| Some(refresh_token.to_string())),
        expires_at,
    })
}
