import { Body1, Title2, makeStyles, tokens } from "@fluentui/react-components";

const useStyles = makeStyles({
  hero: {
    display: "flex",
    justifyContent: "center",
    marginBottom: tokens.spacingVerticalXL,
  },
  heroLogo: {
    width: "100%",
    maxWidth: "640px",
    height: "auto",
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow16,
  },
});

/**
 * ホーム画面(プレースホルダー)。
 *
 * TODO: サインイン状態のサマリー、最近使用したプロファイル、
 * お知らせなどのダッシュボードをここに実装する。
 */
export function HomePage() {
  const styles = useStyles();
  return (
    <div>
      <div className={styles.hero}>
        <img
          src="/branding/train-launcher-logo.svg"
          alt="TRAiN Launcher"
          className={styles.heroLogo}
        />
      </div>
      <Title2 as="h2" block>
        ホーム
      </Title2>
      <Body1 as="p" block>
        TRAiN Launcherへようこそ。ここにダッシュボード(最近使ったプロファイル、
        お知らせなど)を表示する予定です。
      </Body1>
    </div>
  );
}
