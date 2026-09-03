import { Body1, Title2 } from "@fluentui/react-components";

/**
 * ホーム画面(プレースホルダー)。
 *
 * TODO: サインイン状態のサマリー、最近使用したプロファイル、
 * お知らせなどのダッシュボードをここに実装する。
 */
export function HomePage() {
  return (
    <div>
      <Title2 as="h2">ホーム</Title2>
      <Body1 as="p">
        TRAiN Launcherへようこそ。ここにダッシュボード(最近使ったプロファイル、
        お知らせなど)を表示する予定です。
      </Body1>
    </div>
  );
}
