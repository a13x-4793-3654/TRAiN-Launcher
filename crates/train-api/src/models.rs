//! TRAiN API のダミーデータモデル。
//!
//! API仕様確定後、実際のレスポンス形式に合わせて更新する想定。

use serde::{Deserialize, Serialize};

/// ユーザーが所属しているTRAiN管理サーバー。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServer {
    pub id: String,
    pub name: String,
}

/// サーバーごとの設定情報(接続先・Mod構成・リソースパックなど)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server_id: String,
    pub address: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    pub mod_urls: Vec<String>,
    pub resource_pack_urls: Vec<String>,
}
