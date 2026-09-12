import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { check as checkForUpdate } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type AppUpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installed"
  | "error";

export interface AppUpdateInfo {
  version: string;
  body: string | null;
  date: string | null;
}

export interface AppUpdateProgress {
  downloaded: number;
  contentLength: number | null;
}

interface AppUpdateContextValue {
  status: AppUpdateStatus;
  updateInfo: AppUpdateInfo | null;
  progress: AppUpdateProgress | null;
  error: string | null;
  /** ホーム画面のバナーをユーザーが「後で」で閉じたかどうか。設定画面には影響しない。 */
  dismissed: boolean;
  checkForUpdates: () => Promise<void>;
  installUpdate: () => Promise<void>;
  dismiss: () => void;
}

const AppUpdateContext = createContext<AppUpdateContextValue | null>(null);

/**
 * ランチャー自体のセルフアップデート機能(`tauri-plugin-updater`)の状態をアプリ全体で
 * 共有するプロバイダ。
 *
 * `App.tsx`が起動直後に一度`checkForUpdates()`を呼び出し(サイレントチェック)、
 * 見つかったアップデートはホーム画面上部のバナー(`UpdateBanner`)と設定画面
 * (`Settings.tsx`、手動再チェック用)の両方から同じ状態を参照する。
 */
export function AppUpdateProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AppUpdateStatus>("idle");
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [progress, setProgress] = useState<AppUpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  // `check()`が返す`Update`ハンドルは実際にインストールする際に必要になるため保持しておく。
  // stateにすると余計な再レンダーを招くうえシリアライズ不要なオブジェクトなのでrefで持つ。
  const updateRef = useRef<Update | null>(null);

  useEffect(() => {
    return () => {
      // アンマウント時、インストールに使わなかった`Update`ハンドルのリソースを解放する。
      updateRef.current?.close();
    };
  }, []);

  const checkForUpdates = async () => {
    setStatus("checking");
    setError(null);
    try {
      updateRef.current?.close();
      updateRef.current = null;
      const update = await checkForUpdate();
      updateRef.current = update;
      if (update) {
        setUpdateInfo({
          version: update.version,
          body: update.body ?? null,
          date: update.date ?? null,
        });
        setDismissed(false);
        setStatus("available");
      } else {
        setUpdateInfo(null);
        setStatus("up-to-date");
      }
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  };

  const installUpdate = async () => {
    const update = updateRef.current;
    if (!update) return;
    setStatus("downloading");
    setError(null);
    setProgress({ downloaded: 0, contentLength: null });
    try {
      let downloaded = 0;
      let contentLength: number | null = null;
      await update.downloadAndInstall((downloadEvent) => {
        switch (downloadEvent.event) {
          case "Started":
            contentLength = downloadEvent.data.contentLength ?? null;
            setProgress({ downloaded: 0, contentLength });
            break;
          case "Progress":
            downloaded += downloadEvent.data.chunkLength;
            setProgress({ downloaded, contentLength });
            break;
          case "Finished":
            setProgress({
              downloaded: contentLength ?? downloaded,
              contentLength,
            });
            break;
        }
      });
      setStatus("installed");
      // Windowsではインストーラ実行のためアプリは自動終了する仕様だが、
      // macOS/Linuxでは明示的な再起動が必要なため、プラットフォーム問わず呼び出す。
      await relaunch();
    } catch (err) {
      setError(String(err));
      setStatus("error");
    }
  };

  const dismiss = () => setDismissed(true);

  const value = useMemo<AppUpdateContextValue>(
    () => ({
      status,
      updateInfo,
      progress,
      error,
      dismissed,
      checkForUpdates,
      installUpdate,
      dismiss,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [status, updateInfo, progress, error, dismissed],
  );

  return (
    <AppUpdateContext.Provider value={value}>
      {children}
    </AppUpdateContext.Provider>
  );
}

export function useAppUpdate(): AppUpdateContextValue {
  const context = useContext(AppUpdateContext);
  if (!context) {
    throw new Error("useAppUpdate must be used within an AppUpdateProvider");
  }
  return context;
}
