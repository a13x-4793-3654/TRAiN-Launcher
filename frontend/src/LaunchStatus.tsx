import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  ProgressBar,
  Spinner,
  Caption1,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { listen } from "@tauri-apps/api/event";

const LAUNCH_PROGRESS_EVENT = "launch://progress";

export interface LaunchProgressPayload {
  phase: string;
  phase_label: string;
  completed: number;
  total: number;
}

interface LaunchStatusContextValue {
  launching: boolean;
  progress: LaunchProgressPayload | null;
  /** 起動/参加処理を開始する直前に呼び出す。呼び出し中はアプリ全体を操作不能にする。 */
  beginLaunch: () => void;
  /** 起動/参加処理が成功・失敗いずれで終わっても(`finally`で)呼び出す。 */
  endLaunch: () => void;
}

const LaunchStatusContext = createContext<LaunchStatusContextValue | null>(null);

/**
 * 起動(`launch_minecraft`)・TRAiNサーバー参加(`join_train_server`)の進行状況を
 * アプリ全体で共有するプロバイダ。
 *
 * `launch://progress` イベントはページ単位ではなくここで一度だけ購読し、どの画面から
 * 起動を開始しても進行状況モーダル(`LaunchingModal`)に反映されるようにする。
 * これにより、起動処理中に別画面へ切り替えても進捗表示が失われない
 * (各ページ内で個別に購読していた従来方式では、切り替えるとその画面の購読が
 * 解除されてしまっていた)。
 */
export function LaunchStatusProvider({ children }: { children: ReactNode }) {
  const [launching, setLaunching] = useState(false);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(null);

  useEffect(() => {
    const unlisten = listen<LaunchProgressPayload>(
      LAUNCH_PROGRESS_EVENT,
      (event) => setProgress(event.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const value = useMemo<LaunchStatusContextValue>(
    () => ({
      launching,
      progress,
      beginLaunch: () => {
        setProgress(null);
        setLaunching(true);
      },
      endLaunch: () => {
        setLaunching(false);
        setProgress(null);
      },
    }),
    [launching, progress],
  );

  return (
    <LaunchStatusContext.Provider value={value}>
      {children}
    </LaunchStatusContext.Provider>
  );
}

export function useLaunchStatus(): LaunchStatusContextValue {
  const context = useContext(LaunchStatusContext);
  if (!context) {
    throw new Error("useLaunchStatus must be used within a LaunchStatusProvider");
  }
  return context;
}

const useStyles = makeStyles({
  content: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: tokens.spacingVerticalM,
    paddingTop: tokens.spacingVerticalM,
    paddingBottom: tokens.spacingVerticalL,
  },
  progressArea: {
    width: "100%",
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
  },
});

/**
 * 起動処理中はアプリ全体をブロックするモーダル。
 *
 * `modalType="alert"` により、背景クリックやEscキーでは閉じられず(閉じるボタンも
 * 表示しない)、処理完了(`endLaunch`)まで他の操作を受け付けない。ナビゲーションタブ等
 * 個別要素を`disabled`にして回るのではなく、オーバーレイで一括してブロックすることで
 * 起動中に他画面へ切り替えて進捗購読が途切れる、といった不整合も防ぐ。
 */
export function LaunchingModal() {
  const styles = useStyles();
  const { launching, progress } = useLaunchStatus();

  return (
    <Dialog open={launching} modalType="alert">
      <DialogSurface>
        <DialogBody>
          <DialogTitle>起動処理中です</DialogTitle>
          <DialogContent>
            <div className={styles.content}>
              <Spinner size="large" />
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
              <Caption1>完了するまでお待ちください。</Caption1>
            </div>
          </DialogContent>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
