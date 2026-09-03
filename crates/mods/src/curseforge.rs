//! CurseForge API クライアント (`furse` crateを使用)。

use furse::Furse;

use crate::ModsError;

/// CurseForge APIクライアントを構築する。
///
/// TODO: APIキー(https://console.curseforge.com/#/api-keys で発行)の
/// 安全な保存・読み込みを実装する。
pub fn build_client(api_key: &str) -> Furse {
    Furse::new(api_key.to_string())
}

/// URL(またはmod ID/file ID)からModのファイル情報を解決する。
///
/// TODO: `Furse::get_mod_file` 等を利用した実装を行う。
pub async fn resolve_from_url(_url: &str, api_key: &str) -> Result<(), ModsError> {
    let _client = build_client(api_key);
    Err(ModsError::NotImplemented("curseforge::resolve_from_url"))
}
