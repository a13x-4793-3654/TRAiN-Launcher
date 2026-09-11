import { useCallback, useEffect, useState } from "react";
import {
  Body1,
  Caption1,
  Title2,
  Title3,
  Text,
  Button,
  Spinner,
  ProgressBar,
  Field,
  MessageBar,
  MessageBarBody,
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
  Card,
  CardHeader,
  Badge,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import {
  ArrowLeftRegular,
  PlayRegular,
  ArrowSyncRegular,
  ArrowResetRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";

interface MemberServer {
  id: string;
  name: string;
}

interface ServerConfig {
  server_id: string;
  address: string;
  minecraft_version: string;
  mod_loader: string | null;
  mod_urls: string[];
  resource_pack_urls: string[];
  linked: boolean | null;
  test_mode: boolean | null;
}

interface LaunchProgressPayload {
  phase: string;
  phase_label: string;
  completed: number;
  total: number;
}

interface Announcement {
  id: string;
  title: string;
  body: string;
  severity: "info" | "warning" | "critical";
  published_at: string;
}

const dateTimeFormatter = new Intl.DateTimeFormat("ja-JP", {
  dateStyle: "medium",
  timeStyle: "short",
});

/** ISO8601文字列を "2026年9月11日 2:34" のような表示用文字列に変換する。解釈できない場合はそのまま返す。 */
function formatPublishedAt(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : dateTimeFormatter.format(date);
}

// お知らせのseverityに応じたBadgeの色・ラベル。未知の値が来た場合は"informative"扱い。
const SEVERITY_BADGE_COLOR: Record<string, "informative" | "warning" | "danger"> = {
  info: "informative",
  warning: "warning",
  critical: "danger",
};
const SEVERITY_LABEL: Record<string, string> = {
  info: "情報",
  warning: "注意",
  critical: "重要",
};

const useStyles = makeStyles({
  backRow: {
    marginBottom: tokens.spacingVerticalM,
  },
  commandBar: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalS,
    flexWrap: "wrap",
    marginTop: tokens.spacingVerticalM,
    marginBottom: tokens.spacingVerticalM,
  },
  section: {
    marginTop: tokens.spacingVerticalXL,
  },
  urlList: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXXS,
    marginTop: tokens.spacingVerticalS,
  },
  announcementList: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalS,
    marginTop: tokens.spacingVerticalM,
  },
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    marginTop: tokens.spacingVerticalM,
    width: "100%",
  },
});

/**
 * 所属サーバー詳細画面。
 *
 * `ServersPage` から特定のサーバーの「詳細」ボタンを押すと表示される。参加(起動)前に、
 * `get_server_config` で取得した接続先・Minecraftバージョン・Modローダー・導入される
 * Mod/リソースパックのURL一覧を確認できる。実際の参加・起動処理(`join_train_server`の
 * 呼び出し、進捗表示、完了/失敗トースト)は `ServersPage` 側で一元管理しており、
 * このコンポーネントは表示と `onJoin` の呼び出しのみを担当する。
 *
 * 「起動」「更新」「初期化」は画面上部のコマンドバーにまとめている。Mod一覧が多い
 * サーバーだと接続情報・Mod一覧が縦に長くなり、ボタンを最下部に置くとスクロールが
 * 必要になっていたため、常に見える位置に移動した。
 */
export function ServerDetailPage(props: {
  server: MemberServer;
  onBack: () => void;
  onJoin: () => void;
  joining: boolean;
  progress: LaunchProgressPayload | null;
}) {
  const styles = useStyles();

  const [config, setConfig] = useState<ServerConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [announcements, setAnnouncements] = useState<Announcement[]>([]);
  const [announcementsLoading, setAnnouncementsLoading] = useState(true);

  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [resetMessage, setResetMessage] = useState<
    { intent: "success" | "error"; text: string } | null
  >(null);

  const loadConfig = useCallback(() => {
    setLoading(true);
    setError(null);
    invoke<ServerConfig>("get_server_config", { serverId: props.server.id })
      .then(setConfig)
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, [props.server.id]);

  useEffect(() => {
    setConfig(null);
    loadConfig();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.server.id]);

  useEffect(() => {
    setAnnouncements([]);
    setAnnouncementsLoading(true);
    invoke<Announcement[]>("get_server_announcements", {
      serverId: props.server.id,
    })
      .then(setAnnouncements)
      .catch((err) => console.error("failed to load server announcements", err))
      .finally(() => setAnnouncementsLoading(false));
  }, [props.server.id]);

  const handleReset = () => {
    setResetting(true);
    setResetMessage(null);
    invoke("reset_server_profile_mods", { serverId: props.server.id })
      .then(() => {
        setResetMessage({
          intent: "success",
          text: "導入済みのMod・リソースパックを削除しました。次回「起動」時に再ダウンロードします。",
        });
      })
      .catch((err) =>
        setResetMessage({ intent: "error", text: `初期化に失敗しました: ${String(err)}` }),
      )
      .finally(() => {
        setResetting(false);
        setResetDialogOpen(false);
      });
  };

  return (
    <div>
      <div className={styles.backRow}>
        <Button appearance="subtle" icon={<ArrowLeftRegular />} onClick={props.onBack}>
          サーバー一覧に戻る
        </Button>
      </div>

      <Title2 as="h2" block>
        {props.server.name}
      </Title2>
      <Caption1 as="p" block>
        {props.server.id}
      </Caption1>

      <div className={styles.commandBar}>
        <Button
          appearance="primary"
          icon={props.joining ? <Spinner size="tiny" /> : <PlayRegular />}
          disabled={props.joining || loading || !config}
          onClick={props.onJoin}
        >
          起動
        </Button>
        <Button
          appearance="secondary"
          icon={loading ? <Spinner size="tiny" /> : <ArrowSyncRegular />}
          disabled={props.joining || loading}
          onClick={loadConfig}
        >
          更新
        </Button>
        <Button
          appearance="secondary"
          icon={<ArrowResetRegular />}
          disabled={props.joining || resetting}
          onClick={() => setResetDialogOpen(true)}
        >
          初期化
        </Button>
      </div>

      {props.joining && (
        <div className={styles.progressArea}>
          <ProgressBar
            value={
              props.progress && props.progress.total > 0
                ? props.progress.completed / props.progress.total
                : undefined
            }
          />
          <Caption1>{props.progress?.phase_label ?? "準備中..."}</Caption1>
        </div>
      )}

      {resetMessage && (
        <MessageBar intent={resetMessage.intent} className={styles.section}>
          <MessageBarBody>{resetMessage.text}</MessageBarBody>
        </MessageBar>
      )}

      {announcementsLoading ? (
        <div className={styles.section}>
          <Spinner size="small" label="お知らせを取得中..." />
        </div>
      ) : announcements.length > 0 ? (
        <div className={styles.section}>
          <Title3 as="h3" block>
            お知らせ
          </Title3>
          <div className={styles.announcementList}>
            {announcements.map((announcement) => (
              <Card key={announcement.id}>
                <CardHeader
                  header={<Text weight="semibold">{announcement.title}</Text>}
                  description={
                    <Caption1>
                      {formatPublishedAt(announcement.published_at)}
                    </Caption1>
                  }
                  action={
                    <Badge
                      appearance="tint"
                      color={
                        SEVERITY_BADGE_COLOR[announcement.severity] ??
                        "informative"
                      }
                    >
                      {SEVERITY_LABEL[announcement.severity] ??
                        announcement.severity}
                    </Badge>
                  }
                />
                <Body1 as="p" block>
                  {announcement.body}
                </Body1>
              </Card>
            ))}
          </div>
        </div>
      ) : null}

      {loading ? (
        <Spinner size="small" label="サーバー設定を取得中..." />
      ) : error ? (
        <MessageBar intent="error">
          <MessageBarBody>サーバー設定の取得に失敗しました: {error}</MessageBarBody>
        </MessageBar>
      ) : config ? (
        <>
          <div className={styles.section}>
            <Title3 as="h3" block>
              接続情報
            </Title3>
            <Field label="接続先アドレス">
              <Text>{config.address}</Text>
            </Field>
            <Field label="Minecraftバージョン">
              <Text>{config.minecraft_version}</Text>
            </Field>
            <Field label="Modローダー">
              <Text>{config.mod_loader ?? "なし(バニラ)"}</Text>
            </Field>
            <Field label="Discordアカウントとの連携">
              <Text>
                {config.linked === false
                  ? "未連携(初回参加時に連携の案内が表示されます)"
                  : config.linked === true
                    ? "連携済み"
                    : "不明"}
              </Text>
            </Field>
            {config.test_mode === true && (
              <MessageBar intent="warning">
                <MessageBarBody>
                  試験モードが有効です。Administrator権限を持つあなたが「起動」すると、
                  本番ではなく試験用に構成されたMod/リソースパックが導入されます。
                </MessageBarBody>
              </MessageBar>
            )}
          </div>

          <div className={styles.section}>
            <Title3 as="h3" block>
              導入されるMod({config.mod_urls.length}件)
            </Title3>
            {config.mod_urls.length === 0 ? (
              <Body1 as="p" block>
                このサーバーで指定されているModはありません。
              </Body1>
            ) : (
              <div className={styles.urlList}>
                {config.mod_urls.map((url) => (
                  <Caption1 key={url}>{url}</Caption1>
                ))}
              </div>
            )}
          </div>

          <div className={styles.section}>
            <Title3 as="h3" block>
              導入されるリソースパック({config.resource_pack_urls.length}件)
            </Title3>
            {config.resource_pack_urls.length === 0 ? (
              <Body1 as="p" block>
                このサーバーで指定されているリソースパックはありません。
              </Body1>
            ) : (
              <div className={styles.urlList}>
                {config.resource_pack_urls.map((url) => (
                  <Caption1 key={url}>{url}</Caption1>
                ))}
              </div>
            )}
          </div>
        </>
      ) : null}

      <Dialog
        open={resetDialogOpen}
        onOpenChange={(_event, data) => {
          if (!resetting) {
            setResetDialogOpen(data.open);
          }
        }}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Mod・リソースパックの初期化</DialogTitle>
            <DialogContent>
              <Body1>
                このサーバー専用プロファイルに導入済みのMod・リソースパックを
                すべて削除します。ワールドデータや設定は削除されません。
                次回「起動」時にサーバー設定に基づいて再ダウンロードします。
                よろしいですか?
              </Body1>
            </DialogContent>
            <DialogActions>
              <Button
                appearance="secondary"
                onClick={() => setResetDialogOpen(false)}
                disabled={resetting}
              >
                キャンセル
              </Button>
              <Button
                appearance="primary"
                icon={resetting ? <Spinner size="tiny" /> : undefined}
                disabled={resetting}
                onClick={handleReset}
              >
                初期化する
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
