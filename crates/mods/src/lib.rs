//! train-launcher-mods
//!
//! Modrinth/CurseForgeからのMod解決・依存関係自動解決・インストール、
//! およびリソースパックの自動導入を担当するクレート。

pub mod config;
pub mod curseforge;
pub mod modrinth;
pub mod resolver;
pub mod resource_pack;

pub use error::ModsError;

mod error {
    /// train-launcher-mods 全体で使用するエラー型。
    #[derive(Debug, thiserror::Error)]
    pub enum ModsError {
        #[error("not implemented yet: {0}")]
        NotImplemented(&'static str),
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
        #[error("Modrinth API error: {0}")]
        Modrinth(#[from] ferinth::Error),
        #[error("CurseForge API error: {0}")]
        CurseForge(#[from] furse::Error),
        #[error("invalid or unsupported mod/resource pack URL: {0}")]
        InvalidUrl(String),
        #[error("mod/version/file not found: {0}")]
        NotFound(String),
        #[error(
            "CurseForge APIキーが設定されていません(環境変数 TRAIN_LAUNCHER_CURSEFORGE_API_KEY を設定してください)"
        )]
        ApiKeyMissing,
        #[error("downloaded file hash mismatch for {url}: expected {expected}, actual {actual}")]
        HashMismatch {
            url: String,
            expected: String,
            actual: String,
        },
    }
}
