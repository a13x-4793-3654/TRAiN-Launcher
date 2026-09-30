import { useEffect, useState } from "react";
import {
  Body1,
  Text,
  Button,
  Spinner,
  MessageBar,
  MessageBarBody,
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** `apps/desktop/src/crash_reports.rs` の `PendingCrashReport` と対応する。 */
export interface PendingCrashReport {
  id: string;
  serverId: string;
  profileName: string;
  kind: string;
  fileName: string;
  crashedAt: string;
  exitCode: number | null;
  minecraftVersion: string;
  modLoader: string | null;
  launcherVersion: string;
  content: string;
  logExcerpt: string | null;
}

const useStyles = makeStyles({
  section: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    marginTop: tokens.spacingVerticalS,
  },
  preview: {
    maxHeight: "30vh",
    overflow: "auto",
    margin: 0,
    padding: tokens.spacingHorizontalS,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    borderRadius: tokens.borderRadiusMedium,
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase200,
    whiteSpace: "pre",
  },
  hint: {
    marginTop: tokens.spacingVerticalS,
  },
});

/**
 * TRAiNサーバーへ接続中にMinecraftがクラッシュした場合に、クラッシュレポートを
 * TRAiNへ送信してよいか同意を得るモーダル。同意ボタンを押さない限り送信しない。
 */
export function CrashReportConsentModal() {
  const styles = useStyles();
  const [reports, setReports] = useState<PendingCrashReport[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showContent, setShowContent] = useState(false);

  useEffect(() => {
    invoke<PendingCrashReport[]>("list_pending_crash_reports")
      .then((list) => setReports(Array.isArray(list) ? list : []))
      .catch(() => {});
    const unlisten = listen<PendingCrashReport>(
      "crash-report://detected",
      (event) => {
        setReports((current) =>
          current.some((item) => item.id === event.payload.id)
            ? current
            : [...current, event.payload],
        );
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const report = reports[0];

  const close = (id: string) => {
    setReports((current) => current.filter((item) => item.id !== id));
    setError(null);
    setShowContent(false);
  };

  const handleSubmit = async () => {
    if (!report) return;
    setSending(true);
    setError(null);
    try {
      await invoke<string>("submit_crash_report", { id: report.id });
      close(report.id);
    } catch (err) {
      setError(String(err));
    } finally {
      setSending(false);
    }
  };

  const handleDismiss = async () => {
    if (!report) return;
    try {
      await invoke("dismiss_crash_report", { id: report.id });
    } catch {
      // 破棄に失敗しても画面上は閉じる(ランチャー終了時に破棄される)
    }
    close(report.id);
  };

  if (!report) return null;

  const crashedAt = new Date(report.crashedAt).toLocaleString();

  return (
    <Dialog open modalType="alert">
      <DialogSurface>
        <DialogBody>
          <DialogTitle>クラッシュレポートの送信</DialogTitle>
          <DialogContent>
            <Body1 as="p" block>
              「{report.profileName}」のサーバーへ接続中に Minecraft
              がクラッシュしました({crashedAt})。原因の調査のため、クラッシュ
              レポートをサーバー管理者(TRAiN)へ送信できます。
            </Body1>

            <div className={styles.section}>
              <Text weight="semibold">送信される内容</Text>
              <Body1 as="p" block>
                ・クラッシュレポート本文({report.fileName})
                <br />
                {report.logExcerpt && (
                  <>
                    ・直前のゲームログ(logs/latest.log の末尾)
                    <br />
                  </>
                )}
                ・Minecraft {report.minecraftVersion}
                {report.modLoader ? ` / ${report.modLoader}` : ""} / ランチャー{" "}
                {report.launcherVersion} の各バージョン
                <br />
                ・サインイン中の Discord アカウントと、紐づけ済みの Minecraft
                アカウント
              </Body1>
              <Body1 as="p" block>
                アクセストークンやユーザー名を含むフォルダーパスなどは送信前に
                伏せ字にしています。IPアドレスは受信時に TRAiN 側で伏せられます。
                送信したレポートはサーバーの管理者が閲覧でき、90日後に削除されます。
              </Body1>
            </div>

            <div className={styles.section}>
              <Button
                appearance="subtle"
                size="small"
                onClick={() => setShowContent((value) => !value)}
              >
                {showContent ? "送信内容を隠す" : "送信内容を確認する"}
              </Button>
              {showContent && (
                <>
                  <pre className={styles.preview}>{report.content}</pre>
                  {report.logExcerpt && (
                    <pre className={styles.preview}>{report.logExcerpt}</pre>
                  )}
                </>
              )}
            </div>

            {error && (
              <MessageBar intent="error" className={styles.hint}>
                <MessageBarBody>{error}</MessageBarBody>
              </MessageBar>
            )}
            {reports.length > 1 && (
              <Body1 as="p" block className={styles.hint}>
                ほかに {reports.length - 1} 件のクラッシュレポートがあります。
              </Body1>
            )}
          </DialogContent>
          <DialogActions>
            <Button
              appearance="secondary"
              onClick={handleDismiss}
              disabled={sending}
            >
              送信しない
            </Button>
            <Button
              appearance="primary"
              icon={sending ? <Spinner size="tiny" /> : undefined}
              disabled={sending}
              onClick={handleSubmit}
            >
              同意して送信する
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
