//! Microsoftアカウント (MSA) OAuth2 認証(デバイスコードフロー)。
//!
//! Xbox Live / Minecraftサービスは個人用Microsoftアカウントのみ対応のため、
//! Entra IDの `consumers` テナントのエンドポイントを使用する。
//! ブラウザリダイレクトを受け取るローカルサーバが不要なため、CLIやゲームランチャー向けに
//! 広く使われているデバイスコードフロー([RFC 8628](https://tools.ietf.org/html/rfc8628))を採用する。
//!
//! このクレートはUI(Tauri)に依存しないよう設計されているため、verification_uri/user_codeを
//! ユーザーに提示するタイミングでは戻り値ではなくコールバック `on_device_code` を呼び出す。
//! 呼び出し側(Tauri command層)はこのコールバック内でダイアログ表示やイベントemitを行う想定。

use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, ClientId, DeviceAuthorizationUrl, Scope, TokenResponse, TokenUrl};

use crate::AuthError;

/// Microsoft Entra ID (consumers テナント) のデバイスコードフロー関連エンドポイント。
pub const MSA_AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const MSA_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const MSA_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";

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

/// ユーザーに提示すべきデバイスコード情報。
#[derive(Debug, Clone)]
pub struct DeviceCodeInfo {
    /// ユーザーがブラウザで開くべきURL。
    pub verification_uri: String,
    /// ユーザーが入力するコード。
    pub user_code: String,
    /// このコードの有効期限(秒)。
    pub expires_in_secs: u64,
}

/// デバイスコードフローでMSAサインインを行い、Microsoftアクセストークンを取得する。
///
/// `on_device_code` はverification_uri/user_codeが取得できた時点(ポーリング開始前)に
/// 一度だけ呼び出される。呼び出し側はここでユーザーにコードとURLを提示する。
/// 取得したアクセストークンは `crate::xbox::exchange_microsoft_token` に渡して
/// Minecraftトークンへ変換する。
pub async fn sign_in<F>(
    config: &crate::config::MicrosoftConfig,
    on_device_code: F,
) -> Result<MsaToken, AuthError>
where
    F: FnOnce(DeviceCodeInfo) + Send,
{
    let client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(AuthUrl::new(MSA_AUTH_URL.to_string()).expect("static URL is valid"))
        .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string()).expect("static URL is valid"))
        .set_device_authorization_url(
            DeviceAuthorizationUrl::new(MSA_DEVICE_CODE_URL.to_string())
                .expect("static URL is valid"),
        );

    // oauth2 crateが内部で使うreqwestのバージョンとこのクレートが依存するreqwestの
    // バージョンがずれる可能性があるため、`oauth2::reqwest` 経由で提供されるクライアントを使う。
    let http_client = oauth2::reqwest::Client::new();

    let details: oauth2::StandardDeviceAuthorizationResponse = client
        .exchange_device_code()
        .add_scope(Scope::new(MSA_SCOPES.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|err| AuthError::Oauth(err.to_string()))?;

    on_device_code(DeviceCodeInfo {
        verification_uri: details.verification_uri().to_string(),
        user_code: details.user_code().secret().to_string(),
        expires_in_secs: details.expires_in().as_secs(),
    });

    let token = client
        .exchange_device_access_token(&details)
        .request_async(&http_client, tokio::time::sleep, None)
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
