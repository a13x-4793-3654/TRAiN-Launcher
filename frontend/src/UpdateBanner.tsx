import {
  Text,
  Caption1,
  Button,
  ProgressBar,
  Spinner,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { ArrowSyncRegular, DismissRegular } from "@fluentui/react-icons";
import { useAppUpdate } from "./AppUpdate";

const useStyles = makeStyles({
  banner: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: tokens.spacingHorizontalM,
    padding: `${tokens.spacingVerticalS} ${tokens.spacingHorizontalL}`,
    backgroundColor: tokens.colorBrandBackground2,
    borderBottomWidth: "1px",
    borderBottomStyle: "solid",
    borderBottomColor: tokens.colorNeutralStroke2,
  },
  text: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXXS,
    minWidth: 0,
  },
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXXS,
    width: "280px",
    maxWidth: "100%",
  },
  actions: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalS,
    flexShrink: 0,
  },
});

/**
 * ランチャー自体のアップデートが見つかった際に、画面上部(ヘッダー直下・全タブ共通)に
 * 表示する通知バー。`AppUpdateProvider`の状態(`App.tsx`が起動時に一度サイレントで
 * `checkForUpdates()`を呼び出して更新する)をそのまま表示するだけで、実際のチェック処理は
 * 持たない。「後で」を押しても次回起動時には再度表示される(`dismissed`はメモリ上のみ)。
 */
export function UpdateBanner() {
  const styles = useStyles();
  const { status, updateInfo, progress, dismissed, dismiss, installUpdate } =
    useAppUpdate();

  if (dismissed && status === "available") return null;
  if (status !== "available" && status !== "downloading" && status !== "installed") {
    return null;
  }

  const percent =
    progress && progress.contentLength
      ? progress.downloaded / progress.contentLength
      : undefined;

  return (
    <div className={styles.banner}>
      <div className={styles.text}>
        <Text weight="semibold">
          新しいバージョン{updateInfo ? ` v${updateInfo.version} ` : ""}
          が利用可能です
        </Text>
        {status === "downloading" && (
          <div className={styles.progressArea}>
            <ProgressBar value={percent} />
            <Caption1>ダウンロード中...</Caption1>
          </div>
        )}
        {status === "installed" && (
          <Caption1>インストールが完了しました。再起動しています...</Caption1>
        )}
      </div>
      {status === "available" && (
        <div className={styles.actions}>
          <Button
            appearance="primary"
            icon={<ArrowSyncRegular />}
            onClick={installUpdate}
          >
            今すぐアップデート
          </Button>
          <Button
            appearance="transparent"
            icon={<DismissRegular />}
            onClick={dismiss}
          >
            後で
          </Button>
        </div>
      )}
      {status === "downloading" && <Spinner size="tiny" />}
    </div>
  );
}
