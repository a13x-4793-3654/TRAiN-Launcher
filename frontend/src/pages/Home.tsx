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
  Card,
  CardHeader,
  Badge,
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
import type { NavKey } from "../App";
import { useLaunchStatus } from "../LaunchStatus";

const TOASTER_ID = "home-toaster";
const GAME_EXITED_EVENT = "game://exited";
const LAUNCH_PROGRESS_EVENT = "launch://progress";
// ホームには直近使用した分だけを抜粋表示する(全件は「プロファイル」画面で確認できる)。
const RECENT_PROFILE_LIMIT = 3;

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
}

interface Profile {
  id: string;
  name: string;
  minecraft_version: string;
  last_launched_at: string | null;
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
  hero: {
    display: "flex",
    justifyContent: "center",
    marginBottom: tokens.spacingVerticalXL,
  },
  heroLogo: {
    width: "100%",
    maxWidth: "640px",
    height: "auto",
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow16,
  },
  section: {
    marginTop: tokens.spacingVerticalXXL,
  },
  accountRow: {
    display: "flex",
    flexWrap: "wrap",
    gap: tokens.spacingHorizontalM,
    marginTop: tokens.spacingVerticalM,
  },
  accountItem: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalXS,
  },
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

const dateTimeFormatter = new Intl.DateTimeFormat("ja-JP", {
  dateStyle: "medium",
  timeStyle: "short",
});

/** ISO8601文字列を "2026年9月11日 2:34" のような表示用文字列に変換する。解釈できない場合はそのまま返す。 */
function formatLastLaunched(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : dateTimeFormatter.format(date);
}

/**
 * ホーム画面。
 *
 * サインイン状態のサマリーと、最近起動したプロファイルのクイック起動を表示する
 * ダッシュボード。プロファイルの起動そのものは`Profiles.tsx`と同じ
 * `launch_minecraft`/`launch://progress`/`game://exited` を使い、挙動を揃えている。
 */
export function HomePage({ onNavigate }: { onNavigate: (key: NavKey) => void }) {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);
  const { beginLaunch, endLaunch } = useLaunchStatus();

  const [authStatus, setAuthStatus] = useState<AuthStatus>({
    discord_display_name: null,
    microsoft_display_name: null,
  });
  const [authLoading, setAuthLoading] = useState(true);

  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(true);

  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(
    null,
  );

  useEffect(() => {
    setAuthLoading(true);
    invoke<AuthStatus>("get_auth_status")
      .then(setAuthStatus)
      .catch((err) => console.error("failed to load auth status", err))
      .finally(() => setAuthLoading(false));
  }, []);

  useEffect(() => {
    setProfilesLoading(true);
    invoke<Profile[]>("list_profiles")
      .then(setProfiles)
      .catch((err) => console.error("failed to load profiles", err))
      .finally(() => setProfilesLoading(false));
  }, []);

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

  const recentProfiles = profiles
    .filter((profile) => profile.last_launched_at !== null)
    .sort((a, b) =>
      (b.last_launched_at ?? "").localeCompare(a.last_launched_at ?? ""),
    )
    .slice(0, RECENT_PROFILE_LIMIT);

  const handleLaunch = (profile: Profile) => {
    setLaunchingId(profile.id);
    setProgress(null);
    beginLaunch();
    invoke("launch_minecraft", { profileId: profile.id })
      .then(() => {
        dispatchToast(
          <Toast>
            <ToastTitle>起動しました</ToastTitle>
            <ToastBody>
              {profile.name}({profile.minecraft_version})
              のダウンロード・起動を開始しました
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
        endLaunch();
      });
  };

  return (
    <div>
      <div className={styles.hero}>
        <img
          src="/branding/train-launcher-logo.svg"
          alt="TRAiN Launcher"
          className={styles.heroLogo}
        />
      </div>
      <Title2 as="h2" block>
        ホーム
      </Title2>
      <Body1 as="p" block>
        TRAiN Launcherへようこそ。
      </Body1>

      <div className={styles.section}>
        <Title3 as="h3" block>
          アカウント状況
        </Title3>
        {authLoading ? (
          <Spinner size="small" label="読み込み中..." />
        ) : (
          <div className={styles.accountRow}>
            <div className={styles.accountItem}>
              <Text weight="semibold">Discord:</Text>
              {authStatus.discord_display_name ? (
                <Badge appearance="tint" color="success">
                  {authStatus.discord_display_name}
                </Badge>
              ) : (
                <Badge appearance="tint" color="informative">
                  未サインイン
                </Badge>
              )}
            </div>
            <div className={styles.accountItem}>
              <Text weight="semibold">Microsoft:</Text>
              {authStatus.microsoft_display_name ? (
                <Badge appearance="tint" color="success">
                  {authStatus.microsoft_display_name}
                </Badge>
              ) : (
                <Badge appearance="tint" color="informative">
                  未サインイン
                </Badge>
              )}
            </div>
          </div>
        )}
        <Caption1 as="p" block>
          サインインはヘッダー右上のボタンから行えます。サーバーへの参加には
          Discord、Minecraftの起動にはMicrosoftのサインインが必要です。
        </Caption1>
      </div>

      <div className={styles.section}>
        <Title3 as="h3" block>
          最近使ったプロファイル
        </Title3>
        {profilesLoading ? (
          <Spinner size="small" label="読み込み中..." />
        ) : recentProfiles.length === 0 ? (
          <>
            <Body1 as="p" block>
              まだプロファイルを起動していません。
            </Body1>
            <Button
              appearance="secondary"
              onClick={() => onNavigate("profiles")}
            >
              プロファイル画面を開く
            </Button>
          </>
        ) : (
          <div className={styles.list}>
            {recentProfiles.map((profile) => (
              <Card key={profile.id}>
                <CardHeader
                  header={<Text weight="semibold">{profile.name}</Text>}
                  description={
                    <Caption1>
                      {profile.minecraft_version} ・ 最終起動:{" "}
                      {formatLastLaunched(profile.last_launched_at as string)}
                    </Caption1>
                  }
                  action={
                    <div className={styles.cardActions}>
                      <Button
                        appearance="primary"
                        size="small"
                        icon={
                          launchingId === profile.id ? (
                            <Spinner size="tiny" />
                          ) : (
                            <PlayRegular />
                          )
                        }
                        disabled={launchingId !== null}
                        onClick={() => handleLaunch(profile)}
                      >
                        起動
                      </Button>
                    </div>
                  }
                />
                {launchingId === profile.id && (
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
      </div>

      <div className={styles.section}>
        <Title3 as="h3" block>
          お知らせ
        </Title3>
        <Body1 as="p" block>
          現在お知らせはありません。
        </Body1>
      </div>

      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}
