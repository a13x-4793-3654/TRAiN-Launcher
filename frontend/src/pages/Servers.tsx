import { useEffect, useState } from "react";
import {
  Body1,
  Title2,
  Button,
  Spinner,
  ProgressBar,
  Caption1,
  Text,
  Card,
  CardHeader,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { PlayRegular } from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const TOASTER_ID = "servers-toaster";
const GAME_EXITED_EVENT = "game://exited";
const LAUNCH_PROGRESS_EVENT = "launch://progress";

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
}

interface MemberServer {
  id: string;
  name: string;
}

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
  list: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalS,
    marginTop: tokens.spacingVerticalM,
  },
  cardActions: {
    display: "flex",
    gap: tokens.spacingHorizontalXS,
  },
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    marginTop: tokens.spacingVerticalS,
  },
});

/**
 * サーバー画面。
 *
 * Discordでサインイン済みの場合、所属しているTRAiN管理サーバーの一覧を
 * `list_member_servers` から取得して表示する。各サーバーの「起動」ボタンを押すと
 * `join_train_server` を呼び出し、サーバー専用プロファイルの自動作成/更新、
 * Mod・リソースパックの自動導入、Minecraftの起動までを一括で行う。
 * 進行状況は `launch://progress` イベントを購読して進捗バーで表示する
 * (`Profiles.tsx` の起動フローと同じパターン)。
 */
export function ServersPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [authLoading, setAuthLoading] = useState(true);
  const [discordSignedIn, setDiscordSignedIn] = useState(false);

  const [servers, setServers] = useState<MemberServer[]>([]);
  const [serversLoading, setServersLoading] = useState(false);
  const [serversError, setServersError] = useState<string | null>(null);

  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(
    null,
  );

  useEffect(() => {
    setAuthLoading(true);
    invoke<AuthStatus>("get_auth_status")
      .then((status) => setDiscordSignedIn(status.discord_display_name !== null))
      .catch((err) => console.error("failed to get auth status", err))
      .finally(() => setAuthLoading(false));
  }, []);

  useEffect(() => {
    if (!discordSignedIn) {
      return;
    }
    setServersLoading(true);
    setServersError(null);
    invoke<MemberServer[]>("list_member_servers")
      .then(setServers)
      .catch((err) => setServersError(String(err)))
      .finally(() => setServersLoading(false));
  }, [discordSignedIn]);

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

  const handleJoin = (server: MemberServer) => {
    setLaunchingId(server.id);
    setProgress(null);
    invoke("join_train_server", {
      serverId: server.id,
      serverName: server.name,
    })
      .then(() => {
        dispatchToast(
          <Toast>
            <ToastTitle>起動しました</ToastTitle>
            <ToastBody>
              {server.name} 用の設定を適用し、Minecraftを起動しました
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
        setLaunchingId(null);
        setProgress(null);
      });
  };

  return (
    <div>
      <Title2 as="h2" block>
        サーバー
      </Title2>
      <Body1 as="p" block>
        Discordでサインインすると、所属しているTRAiNサーバーの設定を自動取得します。
      </Body1>

      {authLoading ? (
        <Spinner size="small" label="読み込み中..." />
      ) : !discordSignedIn ? (
        <Body1 as="p" block>
          Discordでサインインしてください。サインインすると、所属しているTRAiN
          サーバーの一覧が表示されます。
        </Body1>
      ) : serversLoading ? (
        <Spinner size="small" label="サーバー一覧を取得中..." />
      ) : serversError ? (
        <Body1 as="p" block>
          サーバー一覧の取得に失敗しました: {serversError}
        </Body1>
      ) : servers.length === 0 ? (
        <Body1 as="p" block>
          所属しているTRAiNサーバーが見つかりませんでした。
        </Body1>
      ) : (
        <div className={styles.list}>
          {servers.map((server) => (
            <Card key={server.id}>
              <CardHeader
                header={<Text weight="semibold">{server.name}</Text>}
                description={<Caption1>{server.id}</Caption1>}
                action={
                  <div className={styles.cardActions}>
                    <Button
                      appearance="primary"
                      size="small"
                      icon={
                        launchingId === server.id ? (
                          <Spinner size="tiny" />
                        ) : (
                          <PlayRegular />
                        )
                      }
                      disabled={launchingId !== null}
                      onClick={() => handleJoin(server)}
                    >
                      起動
                    </Button>
                  </div>
                }
              />
              {launchingId === server.id && (
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
            </Card>
          ))}
        </div>
      )}

      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}
