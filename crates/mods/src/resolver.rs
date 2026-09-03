//! Mod依存関係の自動解決。
//!
//! `libium`/`ferium` (<https://github.com/gorilla-devs/ferium>) の依存解決ロジックを
//! 参考に、Modrinth/CurseForgeの依存関係APIを再帰的に辿ってインストール対象を
//! 確定させる処理を実装する想定。

use crate::ModsError;

/// 解決対象のMod参照(URL指定)。
#[derive(Debug, Clone)]
pub struct ModReference {
    pub url: String,
}

/// 依存関係を再帰的に解決し、インストールすべきMod一覧(依存Modを含む)を返す。
///
/// TODO: Modrinth/CurseForgeの依存関係APIを辿って再帰的に解決する処理を実装する
/// (参考実装: <https://github.com/gorilla-devs/ferium>)。
pub async fn resolve_dependencies(
    _requested: Vec<ModReference>,
) -> Result<Vec<ModReference>, ModsError> {
    Err(ModsError::NotImplemented("resolver::resolve_dependencies"))
}
