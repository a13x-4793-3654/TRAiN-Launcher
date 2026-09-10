import { useEffect, useState } from "react";
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
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { ArrowLeftRegular, PlayRegular } from "@fluentui/react-icons";
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
}

interface LaunchProgressPayload {
  phase: string;
  phase_label: string;
  completed: number;
  total: number;
}

const useStyles = makeStyles({
  backRow: {
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
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    marginTop: tokens.spacingVerticalM,
  },
  joinRow: {
    marginTop: tokens.spacingVerticalL,
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

  useEffect(() => {
    setLoading(true);
    setError(null);
    setConfig(null);
    invoke<ServerConfig>("get_server_config", { serverId: props.server.id })
      .then(setConfig)
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, [props.server.id]);

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

          <div className={styles.joinRow}>
            <Button
              appearance="primary"
              icon={props.joining ? <Spinner size="tiny" /> : <PlayRegular />}
              disabled={props.joining}
              onClick={props.onJoin}
            >
              参加してMinecraftを起動
            </Button>
            {props.joining && (
              <div className={styles.progressArea}>
                <ProgressBar
                  value={
                    props.progress && props.progress.total > 0
                      ? props.progress.completed / props.progress.total
                      : undefined
                  }
                />
                <Caption1>
                  {props.progress?.phase_label ?? "準備中..."}
                </Caption1>
              </div>
            )}
          </div>
        </>
      ) : null}
    </div>
  );
}
