//! train-launcher-server-api
//!
//! TRAiN独自バックエンドAPIのクライアント。Discordサインイン後、ユーザーが所属している
//! サーバー一覧・各サーバーの設定(接続先・Mod構成・リソースパックなど)を取得するために使う。
//!
//! エンドポイント形式・認証方式・エラーコードはTRAiNバックエンド側の
//! `docs/LAUNCHER-API.md` で確定済み(`GET {base}/api/members/{discord_user_id}/servers` /
//! `GET {base}/api/servers/{server_id}/config`、`Authorization: Bearer <Discordアクセス
//! トークン>`)。[`create_client`] は環境変数 [`API_BASE_URL_ENV_VAR`]
//! (`TRAIN_LAUNCHER_API_BASE_URL`) が未設定の場合、[`MockTrainApiClient`] を返す
//! (実サーバーが用意できない開発・デモ環境向けのフォールバック)。

pub mod models;

use async_trait::async_trait;
use serde::Deserialize;

pub use models::{MemberServer, ServerConfig};

/// train-launcher-server-api 全体で使用するエラー型。
///
/// バリアントのメッセージはそのままフロントエンドのエラー表示に使われるため
/// (`Result<_, String>` を返すTauriコマンドが`err.to_string()`するだけ)、
/// 日本語かつユーザーが次に何をすればよいか分かる文面にする。
#[derive(Debug, thiserror::Error)]
pub enum TrainApiError {
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// `401 unauthorized`: トークンが無い、またはDiscordがトークンを認めなかった。
    #[error("Discordで再サインインしてください(認証が期限切れ、または無効です)")]
    Unauthorized,
    /// `403 forbidden`: トークンの持ち主とリクエスト対象のDiscordユーザーが一致しない。
    #[error("このDiscordアカウントでは所属サーバー情報を取得できません")]
    Forbidden,
    /// `400`: パスパラメータの形式が不正(Discordスノーフレーク/UUIDでない等)。
    #[error("リクエスト内容が正しくありません")]
    InvalidRequest,
    /// `404 not_found`: サーバーが存在しない、または本人がそのギルドのメンバーでない。
    #[error("サーバーが見つからないか、参加資格がありません")]
    NotFound,
    /// `404 not_configured`: サーバーは登録されているが、`minecraft-mods manifest`
    /// (または`apply`)がTRAiN Link経由で一度も送られておらず、接続先/Mod構成が無い。
    #[error(
        "このサーバーはまだ配布設定(接続先・Mod構成)が登録されていません。サーバー管理者に確認してください"
    )]
    NotConfigured,
    /// `405 method_not_allowed`: 想定外のHTTPメソッド(通常は発生しない)。
    #[error("このAPIリクエストは許可されていません")]
    MethodNotAllowed,
    /// 上記に当てはまらない未知のエラーコード。TRAiN側の仕様変更などを検知できるよう、
    /// status/codeをそのまま表示する。
    #[error("APIエラーが発生しました(status: {status}, code: {code})")]
    Api { status: u16, code: String },
}

/// TRAiN APIのエラーレスポンス本文(`{ "error": "..." }`)。
#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

/// 失敗レスポンスを [`TrainApiError`] へ変換する。
///
/// `docs/LAUNCHER-API.md` に列挙されたエラーコードに基づいてマッピングする。
/// 本文が読めない/期待した形式でない場合でも、statusから可能な範囲で判定する。
async fn map_error_response(response: reqwest::Response) -> TrainApiError {
    let status = response.status().as_u16();
    let code = response
        .json::<ErrorBody>()
        .await
        .map(|body| body.error)
        .unwrap_or_default();
    match (status, code.as_str()) {
        (401, _) => TrainApiError::Unauthorized,
        (403, _) => TrainApiError::Forbidden,
        (404, "not_configured") => TrainApiError::NotConfigured,
        (404, _) => TrainApiError::NotFound,
        (405, _) => TrainApiError::MethodNotAllowed,
        (400, _) => TrainApiError::InvalidRequest,
        _ => TrainApiError::Api {
            status,
            code,
        },
    }
}

/// TRAiNバックエンドAPIクライアントの振る舞いを定義するトレイト。
#[async_trait]
pub trait TrainApiClient: Send + Sync {
    /// Discordサインイン後、ユーザーが所属しているTRAiN管理サーバー一覧を取得する。
    async fn get_member_servers(
        &self,
        discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError>;

    /// 指定サーバーの設定(接続先・Mod構成・リソースパックなど)を取得する。
    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError>;
}

/// 開発・テスト用のモック実装。常にダミーデータを返す。
pub struct MockTrainApiClient;

#[async_trait]
impl TrainApiClient for MockTrainApiClient {
    async fn get_member_servers(
        &self,
        _discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError> {
        Ok(vec![MemberServer {
            id: "mock-server-1".to_string(),
            name: "TRAiN Mock Server".to_string(),
        }])
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        Ok(ServerConfig {
            server_id: server_id.to_string(),
            address: "play.example.com:25565".to_string(),
            minecraft_version: "1.21".to_string(),
            mod_loader: Some("fabric".to_string()),
            mod_urls: vec![],
            resource_pack_urls: vec![],
        })
    }
}

/// TRAiN APIのベースURLを指定する環境変数名。
///
/// この環境変数が未設定(または空文字列)の場合、[`create_client`] は [`MockTrainApiClient`]
/// を返す(実サーバー未提供時のフォールバック)。設定する場合はスキーム込みで指定する
/// (例: `https://api.train.example.com`)。
pub const API_BASE_URL_ENV_VAR: &str = "TRAIN_LAUNCHER_API_BASE_URL";

/// TRAiNバックエンドAPIの実HTTPクライアント実装。
///
/// エンドポイント形式(`GET {base}/api/members/{discord_user_id}/servers` /
/// `GET {base}/api/servers/{server_id}/config`)・認証方式・エラーコードは
/// TRAiN側の`docs/LAUNCHER-API.md`で確定済み。TRAiN側は新規のOAuthを実装せず、
/// 渡されたDiscordアクセストークンをそのまま`GET https://discord.com/api/v10/users/@me`
/// へ投げて本人確認を行うため、ここでは受け取ったトークンをBearerとして渡すだけでよい。
pub struct HttpTrainApiClient {
    base_url: String,
    /// Discordアクセストークン。設定されていれば認証ヘッダー(Bearerスキーム)として
    /// 各リクエストに付与する(TRAiN側がDiscord APIへ問い合わせて本人確認を行う)。
    discord_access_token: Option<String>,
    client: reqwest::Client,
}

impl HttpTrainApiClient {
    pub fn new(base_url: impl Into<String>, discord_access_token: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            discord_access_token,
            client: reqwest::Client::new(),
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let request = self.client.get(format!("{}{path}", self.base_url));
        match &self.discord_access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

#[async_trait]
impl TrainApiClient for HttpTrainApiClient {
    async fn get_member_servers(
        &self,
        discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError> {
        let response = self
            .get(&format!("/api/members/{discord_user_id}/servers"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<Vec<MemberServer>>().await?)
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        let response = self
            .get(&format!("/api/servers/{server_id}/config"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<ServerConfig>().await?)
    }
}

/// 環境変数 [`API_BASE_URL_ENV_VAR`] が設定されていれば [`HttpTrainApiClient`]、未設定なら
/// [`MockTrainApiClient`] を返す。
///
/// `discord_access_token` は [`HttpTrainApiClient`] 使用時、認証ヘッダーとして付与される
/// (詳細は [`HttpTrainApiClient`] のドキュメント参照)。
pub fn create_client(discord_access_token: Option<String>) -> Box<dyn TrainApiClient> {
    match std::env::var(API_BASE_URL_ENV_VAR) {
        Ok(base_url) if !base_url.trim().is_empty() => {
            Box::new(HttpTrainApiClient::new(base_url, discord_access_token))
        }
        _ => Box::new(MockTrainApiClient),
    }
}
