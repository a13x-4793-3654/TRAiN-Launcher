import { Body1, Title2 } from "@fluentui/react-components";

/**
 * プロファイル画面(プレースホルダー)。
 *
 * TODO: `list_profiles` Tauri commandを呼び出してプロファイル一覧を表示し、
 * 作成/編集/起動(Minecraft本体の起動処理は次タスク)のUIを実装する。
 */
export function ProfilesPage() {
  return (
    <div>
      <Title2 as="h2">プロファイル</Title2>
      <Body1 as="p">
        Minecraftのバージョン・Mod構成ごとのプロファイルを管理します。
        （作成/編集/起動は今後実装予定です）
      </Body1>
    </div>
  );
}
