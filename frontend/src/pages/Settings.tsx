import { Body1, Title2 } from "@fluentui/react-components";

/**
 * 設定画面(プレースホルダー)。
 *
 * TODO: サインイン管理(アカウント切り替え/サインアウト)、
 * Javaランタイム設定、ダウンロード先ディレクトリなどの設定項目を実装する。
 */
export function SettingsPage() {
  return (
    <div>
      <Title2 as="h2" block>
        設定
      </Title2>
      <Body1 as="p" block>
        アカウント管理、Javaランタイム、ダウンロード先などの設定をここに実装予定です。
      </Body1>
    </div>
  );
}
