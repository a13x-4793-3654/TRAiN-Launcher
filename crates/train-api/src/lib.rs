//! train-launcher-server-api
//!
//! TRAiN独自バックエンドAPIのクライアント。Discordサインイン後、ユーザーが所属している
//! サーバー一覧・各サーバーの設定(接続先・Mod構成・リソースパックなど)を取得するために使う。
//!
//! **現時点でTRAiN側のAPI仕様は未定**のため、このクレートは [`TrainApiClient`] トレイトと
//! ダミーデータを返す [`MockTrainApiClient`] のみを提供する。API仕様確定後、reqwestベースの
//! 実装をこのトレイトに対して追加する想定。

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
///
/// TODO: API仕様確定後、`reqwest` を使った実装 (例: `HttpTrainApiClient`) をこのトレイトに
/// 対して追加する。
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
