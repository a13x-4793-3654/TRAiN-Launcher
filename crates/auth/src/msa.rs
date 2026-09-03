//! Microsoftアカウント (MSA) OAuth2 認証(デバイスコードフロー想定)。
//!
//! 実際のフローは `oauth2` crateのデバイスコードフローAPIを用いて、概ね以下のような
//! 形になる想定(TODO、`minecraft-msa-auth` crateのドキュメント例を参照):
//!
//! ```ignore
//! let client = BasicClient::new(ClientId::new(client_id))
//!     .set_auth_uri(AuthUrl::new(MSA_AUTH_URL.to_string())?)
//!     .set_token_uri(TokenUrl::new(MSA_TOKEN_URL.to_string())?)
//!     .set_device_authorization_url(DeviceAuthorizationUrl::new(MSA_DEVICE_CODE_URL.to_string())?);
//! let http_client = oauth2::reqwest::Client::new();
//! let details = client
//!     .exchange_device_code()?
//!     .add_scope(Scope::new("XboxLive.signin offline_access".to_string()))
//!     .request_async(&http_client)
//!     .await?;
//! // details.verification_uri() / details.user_code() をユーザーに提示する
//! let token = client
//!     .exchange_device_access_token(&details)?
//!     .request_async(&http_client, tokio::time::sleep, None)
//!     .await?;
//! ```

use oauth2::ClientId;

use crate::AuthError;

/// Microsoft Entra ID (consumers テナント) のデバイスコードフロー関連エンドポイント。
pub const MSA_AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const MSA_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const MSA_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";

/// MSAサインインで得られるMicrosoftアクセストークン(スタブ)。
#[derive(Debug, Clone)]
pub struct MsaToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// デバイスコードフローでMSAサインインを行い、Microsoftアクセストークンを取得する。
///
/// TODO: `oauth2` crateのデバイスコードフローAPI(`exchange_device_code` /
/// `exchange_device_access_token`)を実装し、verification_uri/user_codeを
/// ユーザーに提示するUIと連携する。取得したアクセストークンは
/// `crate::xbox::exchange_microsoft_token` に渡してMinecraftトークンへ変換する。
pub async fn sign_in(client_id: &str) -> Result<MsaToken, AuthError> {
    let _client_id = ClientId::new(client_id.to_string());
    Err(AuthError::NotImplemented("msa::sign_in"))
}
