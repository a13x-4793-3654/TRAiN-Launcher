import { useEffect, useState } from "react";
import {
  Body1,
  Title2,
  Field,
  Input,
  Button,
  Spinner,
  ProgressBar,
  Caption1,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const TOASTER_ID = "profiles-toaster";
const GAME_EXITED_EVENT = "game://exited";
const LAUNCH_PROGRESS_EVENT = "launch://progress";

interface GameExitedPayload {
  exit_code: number | null;
}

interface LaunchProgressPayload {
  phase: string;
  phase_label: string;
  completed: number;
  total: number;
}

const useStyles = makeStyles({
  launchForm: {
    display: "flex",
    alignItems: "flex-end",
    gap: tokens.spacingHorizontalM,
    maxWidth: "480px",
    marginTop: tokens.spacingVerticalL,
  },
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    maxWidth: "480px",
    marginTop: tokens.spacingVerticalL,
  },
});

/**
 * プロファイル画面。
 *
 * 現時点ではプロファイルの一覧/作成/編集UIは未実装(TODO)だが、バージョンIDを直接指定して
 * Minecraftを起動する簡易フォームを提供する(`launch_minecraft` Tauri command)。
 * 起動にはMicrosoftアカウントでのサインインが必須。初回起動時はライブラリ/アセットの
 * ダウンロードが発生するため、数分〜十数分程度かかる場合がある。ダウンロード/起動の
 * 進行状況は `launch://progress` イベントを購読し、進捗バーと状況テキストで表示する。
 */
export function ProfilesPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);
  const [versionId, setVersionId] = useState("");
  const [launching, setLaunching] = useState(false);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(
    null,
  );

  useEffect(() => {
    const unlisten = listen<GameExitedPayload>(GAME_EXITED_EVENT, (event) => {
      dispatchToast(
        <Toast>
          <ToastTitle>Minecraftが終了しました</ToastTitle>
          <ToastBody>
            終了コード: {event.payload.exit_code ?? "不明"}
          </ToastBody>
        </Toast>,
        { intent: "info" },
      );
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [dispatchToast]);

  useEffect(() => {
    const unlisten = listen<LaunchProgressPayload>(
      LAUNCH_PROGRESS_EVENT,
      (event) => setProgress(event.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleLaunch = () => {
    if (!versionId.trim()) {
      return;
    }
    setLaunching(true);
    setProgress(null);
    invoke("launch_minecraft", { versionId: versionId.trim() })
      .then(() => {
        dispatchToast(
          <Toast>
            <ToastTitle>起動しました</ToastTitle>
            <ToastBody>
              バージョン {versionId.trim()} のダウンロード・起動を開始しました
            </ToastBody>
          </Toast>,
          { intent: "success" },
        );
      })
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>起動に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => {
        setLaunching(false);
        setProgress(null);
      });
  };

  return (
    <div>
      <Title2 as="h2">プロファイル</Title2>
      <Body1 as="p">
        Minecraftのバージョン・Mod構成ごとのプロファイルを管理します。
        （一覧/作成/編集は今後実装予定です）
      </Body1>
      <div className={styles.launchForm}>
        <Field label="MinecraftバージョンID" style={{ flex: 1 }}>
          <Input
            value={versionId}
            onChange={(_event, data) => setVersionId(data.value)}
            placeholder="例: 1.20.4, 1.12.2"
            disabled={launching}
          />
        </Field>
        <Button
          appearance="primary"
          onClick={handleLaunch}
          disabled={launching || !versionId.trim()}
          icon={launching ? <Spinner size="tiny" /> : undefined}
        >
          起動
        </Button>
      </div>
      {launching && (
        <div className={styles.progressArea}>
          <ProgressBar
            value={
              progress && progress.total > 0
                ? progress.completed / progress.total
                : undefined
            }
          />
          <Caption1>{progress?.phase_label ?? "準備中..."}</Caption1>
        </div>
      )}
      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}

