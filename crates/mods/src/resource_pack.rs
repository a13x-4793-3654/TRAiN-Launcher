//! リソースパックの自動導入。
//!
//! Modと同様に、Modrinth/CurseForgeのURLを指定するだけでダウンロード・設置できる
//! ようにする想定。

use crate::ModsError;

/// URL指定でリソースパックを導入する。
///
/// TODO: `modrinth`/`curseforge` モジュールを利用したダウンロード・設置処理を実装する。
pub async fn install_from_url(_url: &str) -> Result<(), ModsError> {
    Err(ModsError::NotImplemented(
        "resource_pack::install_from_url",
    ))
}
