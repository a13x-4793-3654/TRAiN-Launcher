//! Modrinth API クライアント (`ferinth` crateを使用)。

use ferinth::Ferinth;

use crate::ModsError;

/// Modrinth APIクライアント(未認証)を構築する。
pub fn build_client() -> Ferinth<()> {
    Ferinth::<()>::new(
        env!("CARGO_PKG_NAME"),
        Some(env!("CARGO_PKG_VERSION")),
        None,
    )
}

/// URL(またはプロジェクトID/バージョンID)からModのバージョン情報を解決する。
///
/// TODO: `Ferinth` の project/version 取得APIを利用し、URLからプロジェクトID・
/// バージョンIDを抽出して解決する処理を実装する。
pub async fn resolve_from_url(_url: &str) -> Result<(), ModsError> {
    let _client = build_client();
    Err(ModsError::NotImplemented("modrinth::resolve_from_url"))
}
