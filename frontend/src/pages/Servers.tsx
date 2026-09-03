import { Body1, Title2 } from "@fluentui/react-components";

/**
 * サーバー画面(プレースホルダー)。
 *
 * TODO: Discordサインイン後に `list_member_servers` Tauri commandを呼び出し、
 * 所属サーバーの一覧・設定(接続先/推奨Modなど)を表示する。
 * サインインしていない場合は通常のMinecraftサーバーリストとして機能させる。
 */
export function ServersPage() {
  return (
    <div>
      <Title2 as="h2">サーバー</Title2>
      <Body1 as="p">
        Discordでサインインすると、所属しているTRAiNサーバーの設定を自動取得します。
        サインインしていない場合は、通常のMinecraftサーバー一覧として利用できます。
      </Body1>
    </div>
  );
}
