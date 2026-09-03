//! train-launcher-auth
//!
//! Microsoftアカウント(MSA)→XboxLive→XSTS→Minecraftトークンの認証チェーン、
//! およびDiscord OAuth2サインインを担当するクレート。
//!
//! 現時点ではスキャフォールドのみで、各関数はTODOスタブになっている。

pub mod discord;
pub mod msa;
pub mod xbox;

pub use error::AuthError;

mod error {
    /// train-launcher-auth 全体で使用するエラー型。
    #[derive(Debug, thiserror::Error)]
    pub enum AuthError {
        #[error("not implemented yet: {0}")]
        NotImplemented(&'static str),
        #[error("http error: {0}")]
        Http(#[from] reqwest::Error),
    }
}
