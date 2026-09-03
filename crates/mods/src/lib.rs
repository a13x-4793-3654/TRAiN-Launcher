//! train-launcher-mods
//!
//! Modrinth/CurseForgeからのMod解決・依存関係自動解決・インストール、
//! およびリソースパックの自動導入を担当するクレート。
//!
//! 現時点ではスキャフォールドのみで、各関数はTODOスタブになっている。

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
    }
}
