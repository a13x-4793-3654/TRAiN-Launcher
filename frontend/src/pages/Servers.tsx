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
import { PlayRegular, InfoRegular } from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ServerDetailPage } from "./ServerDetail";
import { AccountLinkNoticeModal } from "./AccountLinkNoticeModal";
import { useLaunchStatus } from "../LaunchStatus";

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

interface ServerConfig {
  linked: boolean | null;
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
    flexShrink: 0,
    whiteSpace: "nowrap",
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
 * 「詳細」ボタンを押すと、参加前に接続先・Mod構成などを確認できる
 * 「所属サーバー詳細」画面(`ServerDetailPage`)に切り替わる(参加/起動処理自体は
 * 引き続きこのコンポーネントが一元管理し、詳細画面へは `onJoin` として渡す)。
 * 進行状況は `launch://progress` イベントを購読して進捗バーで表示する
 * (`Profiles.tsx` の起動フローと同じパターン)。
 *
 * 「起動」を押した際、`get_server_config` の `linked` フィールドが `false`
 * (=このDiscordアカウントが当該サーバーのDiscordギルドで未紐づけ、初回参加相当)
 * の場合は、実際の起動前に `AccountLinkNoticeModal` で注意事項への同意を求め、
 * 同意後に `link_train_account` でDiscord↔Minecraftアカウントの紐づけを完了して
 * から起動処理(`proceedJoin`)へ進む。既に紐づけ済み、またはTRAiN側が未対応で
 * 判定不能(`linked` が `true`/`null`)の場合は、従来どおり即座に起動する。
 */
export function ServersPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);
  const { beginLaunch, endLaunch } = useLaunchStatus();

  const [authLoading, setAuthLoading] = useState(true);
  const [discordSignedIn, setDiscordSignedIn] = useState(false);

  const [servers, setServers] = useState<MemberServer[]>([]);
  const [serversLoading, setServersLoading] = useState(false);
  const [serversError, setServersError] = useState<string | null>(null);

  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(
    null,
  );
  const [selectedServer, setSelectedServer] = useState<MemberServer | null>(
    null,
  );

  // 初回参加(未紐づけ)を検出した場合に表示する、注意事項モーダルの対象サーバー。
  // `null` の間はモーダルを表示しない。
  const [linkNoticeServer, setLinkNoticeServer] = useState<MemberServer | null>(
    null,
  );
  const [linking, setLinking] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);

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

  /**
   * 実際の参加・起動処理(サーバー設定取得〜Mod導入〜Minecraft起動)。
   * 紐づけ確認(`handleJoin`)を経た後に呼び出される。
   */
  const proceedJoin = (server: MemberServer) => {
    setProgress(null);
    beginLaunch();
    invoke<string[]>("join_train_server", {
      serverId: server.id,
      serverName: server.name,
    })
      .then((downloadWarnings) => {
        if (downloadWarnings.length > 0) {
          dispatchToast(
            <Toast>
              <ToastTitle>
                起動しました(一部のMod/リソースパックを導入できませんでした)
              </ToastTitle>
              <ToastBody>
                {downloadWarnings.map((warning) => (
                  <div key={warning}>{warning}</div>
                ))}
              </ToastBody>
            </Toast>,
            { intent: "warning" },
          );
        } else {
          dispatchToast(
            <Toast>
              <ToastTitle>起動しました</ToastTitle>
              <ToastBody>
                {server.name} 用の設定を適用し、Minecraftを起動しました
              </ToastBody>
            </Toast>,
            { intent: "success" },
          );
        }
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
        endLaunch();
      });
  };

  /**
   * 「起動」ボタンの起点。まずサーバー設定を取得し、このDiscordアカウントが
   * このサーバーで未紐づけ(初回参加)と判定できた場合は、注意事項モーダルを表示して
   * 同意・紐づけ完了を待ってから実際の参加処理(`proceedJoin`)へ進む。
   * 既に紐づけ済み、またはTRAiN側が未対応で判定できない場合は、従来どおり
   * 即座に参加処理を開始する(判定不能な場合に誤って毎回モーダルを出さないため)。
   */
  const handleJoin = (server: MemberServer) => {
    setLaunchingId(server.id);
    invoke<ServerConfig>("get_server_config", { serverId: server.id })
      .then((config) => {
        if (config.linked === false) {
          setLinkError(null);
          setLinkNoticeServer(server);
          return;
        }
        proceedJoin(server);
      })
      .catch((err) => {
        setLaunchingId(null);
        dispatchToast(
          <Toast>
            <ToastTitle>起動に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        );
      });
  };

  const handleLinkAgree = () => {
    if (!linkNoticeServer) {
      return;
    }
    const server = linkNoticeServer;
    setLinking(true);
    setLinkError(null);
    invoke("link_train_account", { serverId: server.id })
      .then(() => {
        setLinking(false);
        setLinkNoticeServer(null);
        proceedJoin(server);
      })
      .catch((err) => {
        setLinking(false);
        setLinkError(String(err));
      });
  };

  const handleLinkCancel = () => {
    setLinkNoticeServer(null);
    setLinkError(null);
    setLaunchingId(null);
  };

  return (
    <div>
      {selectedServer ? (
        <ServerDetailPage
          server={selectedServer}
          onBack={() => setSelectedServer(null)}
          onJoin={() => handleJoin(selectedServer)}
          joining={launchingId === selectedServer.id}
          progress={progress}
        />
      ) : (
        <>
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
                          appearance="secondary"
                          size="small"
                          icon={<InfoRegular />}
                          disabled={launchingId !== null}
                          onClick={() => setSelectedServer(server)}
                        >
                          詳細
                        </Button>
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
        </>
      )}

      <AccountLinkNoticeModal
        open={linkNoticeServer !== null}
        serverName={linkNoticeServer?.name ?? ""}
        linking={linking}
        error={linkError}
        onAgree={handleLinkAgree}
        onCancel={handleLinkCancel}
      />

      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}
