//! 起動コマンド(JVM引数・クラスパス・ゲーム引数)の構築とプロセス起動。

use crate::profile::Profile;
use crate::CoreError;

/// プロファイルからMinecraft起動コマンド(実行ファイル + 引数)を構築する。
///
/// TODO: クラスパス組み立て・JVM引数・認証情報(アクセストークン等)の注入を実装する。
pub fn build_launch_command(_profile: &Profile) -> Result<Vec<String>, CoreError> {
    Err(CoreError::NotImplemented("launch::build_launch_command"))
}

/// 構築したコマンドでMinecraftプロセスを起動する。
///
/// TODO: `tokio::process::Command` での起動・標準出力ログ収集を実装する。
pub async fn launch(_profile: &Profile) -> Result<(), CoreError> {
    Err(CoreError::NotImplemented("launch::launch"))
}
