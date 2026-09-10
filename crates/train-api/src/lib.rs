//! train-launcher-server-api
//!
//! TRAiN独自バックエンドAPIのクライアント。Discordサインイン後、ユーザーが所属している
//! サーバー一覧・各サーバーの設定(接続先・Mod構成・リソースパックなど)を取得するために使う。
//!
//! **注意: 現時点でTRAiN側の正式なAPI仕様はまだ確定していない。**
//! [`HttpTrainApiClient`] はREST慣例に基づく暫定実装(`GET {base}/api/members/{id}/servers`
//! 等)であり、実際のTRAiNバックエンドのエンドポイント形式が確定次第、修正が必要になる
//! 可能性が高い。[`create_client`] は環境変数 [`API_BASE_URL_ENV_VAR`]
//! (`TRAIN_LAUNCHER_API_BASE_URL`) が未設定の場合、引き続き [`MockTrainApiClient`] を返す
//! (実サーバーが用意できるまでの開発・デモ用フォールバック)。

pub mod models;

use async_trait::async_trait;

pub use models::{MemberServer, ServerConfig};

/// train-launcher-server-api 全体で使用するエラー型。
#[derive(Debug, thiserror::Error)]
pub enum TrainApiError {
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
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
/// **エンドポイント形式は暫定。** TRAiN側の正式なAPI仕様が確定していないため、一般的な
/// REST慣例(`GET {base}/api/members/{discord_user_id}/servers` /
/// `GET {base}/api/servers/{server_id}/config`)に基づいて実装している。実際の仕様が判明
/// 次第、パス・認証方式を合わせて修正すること。
pub struct HttpTrainApiClient {
    base_url: String,
    /// Discordアクセストークン。設定されていれば認証ヘッダー(Bearerスキーム)として
    /// 各リクエストに付与する(TRAiN側でユーザーを識別するための暫定的な認証手段)。
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
        let servers = self
            .get(&format!("/api/members/{discord_user_id}/servers"))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<MemberServer>>()
            .await?;
        Ok(servers)
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        let config = self
            .get(&format!("/api/servers/{server_id}/config"))
            .send()
            .await?
            .error_for_status()?
            .json::<ServerConfig>()
            .await?;
        Ok(config)
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
