//! train-launcher-core
//!
//! プロファイル管理、Minecraftバージョンマニフェスト/ライブラリ/アセットのダウンロード、
//! 起動コマンドの構築を担当するコアクレート。
//!
//! 現時点ではスキャフォールドのみで、各関数はTODOスタブになっている。

pub mod download;
pub mod launch;
pub mod profile;
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
    }
}
