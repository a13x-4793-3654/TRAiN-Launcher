//! Windows環境で、GUIサブシステムの親プロセス(TRAiN Launcher本体)から
//! コンソールサブシステムの子プロセス(`java.exe`等)を起動すると、親プロセスに
//! コンソールが存在しないため、Windowsが子プロセス用に新しいコンソールウィンドウを
//! 自動的に割り当ててしまう。この黒いコンソールウィンドウはユーザーからは何をしている
//! ものか分からず、Minecraft/Modローダーインストーラの起動中ずっと表示され続けて
//! しまう問題があった。
//!
//! `CREATE_NO_WINDOW` フラグを立てて起動することで、この余計なコンソールウィンドウの
//! 出現を防ぐ。標準出力/標準エラーの取得(`Stdio::piped()`等)には一切影響しない。

use tokio::process::Command;

/// Windows環境で子プロセス起動時にコンソールウィンドウが表示されないようにする
/// (`CREATE_NO_WINDOW`)。Windows以外のOSでは何もしない(元々コンソールウィンドウが
/// 勝手に開くことはないため)。
pub fn suppress_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        /// [Process Creation Flags](https://learn.microsoft.com/windows/win32/procthread/process-creation-flags)
        /// の `CREATE_NO_WINDOW`。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}
