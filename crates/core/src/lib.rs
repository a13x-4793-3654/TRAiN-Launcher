//! train-launcher-core
//!
//! プロファイル管理、Minecraftバージョンマニフェスト/ライブラリ/アセットのダウンロード、
//! 起動コマンドの構築を担当するコアクレート。

pub mod download;
pub mod launch;
pub mod launcher_profiles;
pub mod paths;
pub mod profile;
pub mod rules;
pub mod settings;
pub mod version_manifest;

pub use error::CoreError;

mod error {
    /// train-launcher-core 全体で使用するエラー型。
    #[derive(Debug, thiserror::Error)]
    pub enum CoreError {
        #[error("not implemented yet: {0}")]
        NotImplemented(&'static str),
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),
        #[error("json error: {0}")]
        Json(#[from] serde_json::Error),
        #[error("zip error: {0}")]
        Zip(#[from] zip::result::ZipError),
        #[error("version not found in manifest: {0}")]
        VersionNotFound(String),
        #[error("local version json is incomplete and has no inheritsFrom: {0}")]
        IncompleteVersionJson(String),
        #[error("downloaded file hash mismatch for {url}: expected {expected}, actual {actual}")]
        HashMismatch {
            url: String,
            expected: String,
            actual: String,
        },
        #[error("invalid library name/path: {0}")]
        InvalidLibraryName(String),
        #[error("invalid launch command: {0}")]
        InvalidLaunchCommand(String),
        #[error("profile not found: {0}")]
        ProfileNotFound(String),
        #[error("profile already exists: {0}")]
        ProfileAlreadyExists(String),
    }
}
