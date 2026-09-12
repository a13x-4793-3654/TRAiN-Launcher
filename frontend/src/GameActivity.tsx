import { useEffect, useState, useSyncExternalStore } from "react";
import {
  Body1,
  Dialog,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  MessageBar,
  MessageBarBody,
  Spinner,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { GameActivity } from "./gameDataTypes";

interface ActivityState {
  activity: GameActivity | null;
  error: string | null;
}

let pendingOperations = 0;
const operationListeners = new Set<() => void>();

function subscribeToOperations(listener: () => void) {
  operationListeners.add(listener);
  return () => {
    operationListeners.delete(listener);
  };
}

function hasPendingOperation() {
  return pendingOperations > 0;
}

function notifyOperations() {
  operationListeners.forEach((listener) => listener());
}

// The final backend event can arrive before invoke resolves. Keep the modal
// blocking until the caller actually receives its result, including errors.
export async function runGameDataOperation<T>(
  operation: () => Promise<T>,
): Promise<T> {
  pendingOperations += 1;
  notifyOperations();
  try {
    return await operation();
  } finally {
    pendingOperations -= 1;
    notifyOperations();
  }
}

export function useGameActivity(): ActivityState {
  const [state, setState] = useState<ActivityState>({
    activity: null,
    error: null,
  });
  const pending = useSyncExternalStore(
    subscribeToOperations,
    hasPendingOperation,
    hasPendingOperation,
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    function acceptActivity(activity: GameActivity) {
      if (!disposed) {
        setState((current) =>
          current.activity && activity.revision < current.activity.revision
            ? current
            : { activity, error: null },
        );
      }
    }

    async function connect() {
      try {
        const stop = await listen<GameActivity>("game://activity", (event) => {
          acceptActivity(event.payload);
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        try {
          const activity = await invoke<GameActivity>("get_game_activity", {});
          acceptActivity(activity);
        } catch (error) {
          if (!disposed) {
            setState((current) => ({
              activity: current.activity,
              error: String(error),
            }));
          }
        }
      } catch (error) {
        if (!disposed) {
          setState({ activity: null, error: String(error) });
        }
      }
    }

    void connect();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return {
    activity: pending
      ? {
          revision: state.activity?.revision ?? 0,
          running_games: state.activity?.running_games ?? 0,
          file_operation: true,
        }
      : state.activity,
    error: state.error,
  };
}

export function GameActivityNotice({ activity, error }: ActivityState) {
  if (error) {
    return (
      <MessageBar intent="error">
        <MessageBarBody>
          Minecraftの状態を確認できないため操作できません。画面を開き直して
          再試行してください。{error}
        </MessageBarBody>
      </MessageBar>
    );
  }
  if (!activity) {
    return <Spinner size="tiny" label="Minecraftの起動状態を確認しています..." />;
  }
  if (activity.running_games > 0 || activity.file_operation) {
    return (
      <MessageBar intent="info">
        <MessageBarBody>
          {activity.running_games > 0
            ? `起動中または起動準備中のMinecraftが${activity.running_games.toLocaleString("ja-JP")}件あります。すべて終了してから操作してください。`
            : "ゲーム設定・データの処理中です。完了するまでお待ちください。"}
        </MessageBarBody>
      </MessageBar>
    );
  }
  return null;
}

const useStyles = makeStyles({
  content: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: tokens.spacingVerticalL,
    paddingTop: tokens.spacingVerticalL,
    paddingBottom: tokens.spacingVerticalL,
    overflowWrap: "anywhere",
  },
});

export function GameDataOperationModal() {
  const styles = useStyles();
  const { activity } = useGameActivity();

  return (
    <Dialog open={activity?.file_operation === true} modalType="alert">
      <DialogSurface>
        <DialogBody>
          <DialogTitle>ゲーム設定・データを処理しています</DialogTitle>
          <DialogContent>
            <div className={styles.content}>
              <Spinner size="large" label="処理中..." />
              <Body1>
                ゲームデータを読み書きしています。完了するまでアプリを閉じず、
                Minecraftを起動しないでください。
              </Body1>
            </div>
          </DialogContent>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
