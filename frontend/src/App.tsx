import { useEffect, useRef, useState } from "react";
import {
  FluentProvider,
  makeStyles,
  tokens,
  Title1,
  Button,
  TabList,
  Tab,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  Spinner,
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
} from "@fluentui/react-components";
import type { SelectTabEventHandler } from "@fluentui/react-components";
import {
  HomeRegular,
  ServerRegular,
  PersonRegular,
  AppsRegular,
  SettingsRegular,
  ArchiveRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { useSystemTheme } from "./useSystemTheme";
import { trainLightTheme, trainDarkTheme } from "./theme";
import { HomePage } from "./pages/Home";
import { ServersPage } from "./pages/Servers";
import { ProfilesPage } from "./pages/Profiles";
import { BackupsPage } from "./pages/Backups";
import { ModsPage } from "./pages/Mods";
import { SettingsPage } from "./pages/Settings";
import { LaunchStatusProvider, LaunchingModal } from "./LaunchStatus";
import { AppUpdateProvider, useAppUpdate } from "./AppUpdate";
import { UpdateBanner } from "./UpdateBanner";
import { GameDataOperationModal } from "./GameActivity";
import { CrashReportConsentModal } from "./CrashReportConsent";

const TOASTER_ID = "train-launcher-toaster";

interface SignInResult {
  display_name: string;
}

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
  discord_reauthentication_required: boolean;
}

const useStyles = makeStyles({
  root: {
    display: "flex",
    flexDirection: "column",
    height: "100%",
    width: "100%",
  },
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: `${tokens.spacingVerticalM} ${tokens.spacingHorizontalL}`,
    borderBottomWidth: "1px",
    borderBottomStyle: "solid",
    borderBottomColor: tokens.colorNeutralStroke2,
    gap: tokens.spacingHorizontalM,
  },
  brand: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalS,
  },
  brandLogo: {
    height: "32px",
    width: "32px",
    borderRadius: tokens.borderRadiusMedium,
  },
  headerActions: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalS,
  },
  body: {
    display: "flex",
    flex: 1,
    minHeight: 0,
  },
  nav: {
    borderRightWidth: "1px",
    borderRightStyle: "solid",
    borderRightColor: tokens.colorNeutralStroke2,
    paddingTop: tokens.spacingVerticalM,
    paddingBottom: tokens.spacingVerticalM,
    minWidth: "200px",
  },
  content: {
    flex: 1,
    overflow: "auto",
    padding: tokens.spacingHorizontalXXL,
  },
});

type NavKey = "home" | "servers" | "profiles" | "mods" | "backups" | "settings";
export type { NavKey };

// 「所属サーバー詳細」は`ServersPage`内のサブ画面(`ServerDetail.tsx`)として、
// 「Mod導入ウィザード」は`ModsPage`から開くダイアログ(`ModWizard.tsx`)として実装済み。
// どちらもナビゲーションタブ自体は増やさず、既存画面内で完結する。
function AppShell() {
  const styles = useStyles();
  const [selected, setSelected] = useState<NavKey>("home");
  const { dispatchToast } = useToastController(TOASTER_ID);
  const { checkForUpdates } = useAppUpdate();

  const [authStatus, setAuthStatus] = useState<AuthStatus>({
    discord_display_name: null,
    microsoft_display_name: null,
    discord_reauthentication_required: false,
  });
  const [discordSigningIn, setDiscordSigningIn] = useState(false);
  const [microsoftSigningIn, setMicrosoftSigningIn] = useState(false);
  const [reauthenticationOpen, setReauthenticationOpen] = useState(false);
  const reauthenticationNotified = useRef(false);
  const lastAuthError = useRef<string | null>(null);

  const refreshAuthStatus = () => {
    invoke<AuthStatus>("get_auth_status")
      .then((status) => {
        setAuthStatus(status);
        lastAuthError.current = null;
        if (status.discord_reauthentication_required && !reauthenticationNotified.current) {
          setReauthenticationOpen(true);
        }
        reauthenticationNotified.current = status.discord_reauthentication_required;
      })
      .catch((err) => {
        console.error("failed to load auth status", err);
        const message = String(err);
        if (lastAuthError.current !== message) {
          dispatchToast(
            <Toast>
              <ToastTitle>認証状態を確認できませんでした</ToastTitle>
              <ToastBody>{message} 保存済みの認証情報は保持し、自動的に再試行します。</ToastBody>
            </Toast>,
            { intent: "warning" },
          );
          lastAuthError.current = message;
        }
      });
  };

  // 起動時に保存済みセッション(keyring)からサインイン状態を復元する。
  useEffect(() => {
    refreshAuthStatus();
    const timer = window.setInterval(refreshAuthStatus, 60_000);
    return () => window.clearInterval(timer);
  }, []);

  // 起動直後に一度だけランチャー自体のアップデートをサイレントに確認する。
  // 見つかった場合は`UpdateBanner`に表示されるが、最新版でもエラーでも
  // ここではトースト等は出さない(ユーザーの起動体験を妨げないため)。
  useEffect(() => {
    checkForUpdates().catch((err) =>
      console.error("failed to check for launcher updates", err),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onTabSelect: SelectTabEventHandler = (_event, data) => {
    setSelected(data.value as NavKey);
  };

  const notifySuccess = (provider: string, displayName: string) =>
    dispatchToast(
      <Toast>
        <ToastTitle>{provider}でサインイン</ToastTitle>
        <ToastBody>{displayName} としてサインインしました</ToastBody>
      </Toast>,
      { intent: "success" },
    );

  const notifyError = (provider: string, err: unknown) =>
    dispatchToast(
      <Toast>
        <ToastTitle>{provider}でサインイン</ToastTitle>
        <ToastBody>{String(err)}</ToastBody>
      </Toast>,
      { intent: "error" },
    );

  const handleDiscordSignIn = () => {
    setDiscordSigningIn(true);
    invoke<SignInResult>("sign_in_with_discord")
      .then((result) => {
        notifySuccess("Discord", result.display_name);
        refreshAuthStatus();
      })
      .catch((err) => notifyError("Discord", err))
      .finally(() => setDiscordSigningIn(false));
  };

  const handleMicrosoftSignIn = () => {
    setMicrosoftSigningIn(true);
    invoke<SignInResult>("sign_in_with_microsoft")
      .then((result) => {
        notifySuccess("Microsoft", result.display_name);
        refreshAuthStatus();
      })
      .catch((err) => notifyError("Microsoft", err))
      .finally(() => setMicrosoftSigningIn(false));
  };

  const handleDiscordSignOut = () => {
    invoke("sign_out_discord")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Discord", err));
  };

  const handleMicrosoftSignOut = () => {
    invoke("sign_out_microsoft")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Microsoft", err));
  };

  return (
    <div className={styles.root}>
      <header className={styles.header}>
        <div className={styles.brand}>
          <img
            src="/branding/train-launcher-icon.svg"
            alt=""
            className={styles.brandLogo}
          />
          <Title1 as="h1" block>
            TRAiN Launcher
          </Title1>
        </div>
        <div className={styles.headerActions}>
          {authStatus.discord_display_name ? (
            <Button appearance="secondary" onClick={handleDiscordSignOut}>
              Discord: {authStatus.discord_display_name} (サインアウト)
            </Button>
          ) : (
            <Button
              appearance="secondary"
              onClick={handleDiscordSignIn}
              disabled={discordSigningIn}
              icon={discordSigningIn ? <Spinner size="tiny" /> : undefined}
            >
              Discordでサインイン
            </Button>
          )}
          {authStatus.microsoft_display_name ? (
            <Button appearance="primary" onClick={handleMicrosoftSignOut}>
              {authStatus.microsoft_display_name} (サインアウト)
            </Button>
          ) : (
            <Button
              appearance="primary"
              onClick={handleMicrosoftSignIn}
              disabled={microsoftSigningIn}
              icon={microsoftSigningIn ? <Spinner size="tiny" /> : undefined}
            >
              Microsoftでサインイン
            </Button>
          )}
        </div>
      </header>
      <UpdateBanner />
      <div className={styles.body}>
        <nav className={styles.nav}>
          <TabList
            selectedValue={selected}
            onTabSelect={onTabSelect}
            vertical
          >
            <Tab value="home" icon={<HomeRegular />}>
              ホーム
            </Tab>
            <Tab value="servers" icon={<ServerRegular />}>
              サーバー
            </Tab>
            <Tab value="profiles" icon={<PersonRegular />}>
              プロファイル
            </Tab>
            <Tab value="mods" icon={<AppsRegular />}>
              Mod / リソースパック
            </Tab>
            <Tab value="backups" icon={<ArchiveRegular />}>
              バックアップ
            </Tab>
            <Tab value="settings" icon={<SettingsRegular />}>
              設定
            </Tab>
          </TabList>
        </nav>
        <main className={styles.content}>
          {selected === "home" && <HomePage onNavigate={setSelected} />}
          {selected === "servers" && <ServersPage />}
          {selected === "profiles" && <ProfilesPage />}
          {selected === "mods" && <ModsPage />}
          {selected === "backups" && <BackupsPage />}
          {selected === "settings" && <SettingsPage />}
        </main>
      </div>
      <Toaster toasterId={TOASTER_ID} />
      <LaunchingModal />
      <GameDataOperationModal />
      <CrashReportConsentModal />
      <Dialog open={reauthenticationOpen} onOpenChange={(_, data) => setReauthenticationOpen(data.open)}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Discordの再認証が必要です</DialogTitle>
            <DialogContent>
              保存済みのDiscord認証を更新できませんでした。長期間ランチャーを起動していなかった場合や、Discordの連携が解除された場合は、再度サインインしてください。
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setReauthenticationOpen(false)}>後で</Button>
              <Button appearance="primary" disabled={discordSigningIn} onClick={() => {
                setReauthenticationOpen(false);
                handleDiscordSignIn();
              }}>Discordでサインイン</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}

export default function App() {
  // OSのライト/ダーク設定に自動追従する(TODO: 将来的に設定画面から手動上書きできるようにする)。
  // ブランドカラーはロゴ(assets/branding/train-launcher-logo.svg)に合わせたチール系。
  const scheme = useSystemTheme();
  const theme = scheme === "dark" ? trainDarkTheme : trainLightTheme;

  return (
    <FluentProvider theme={theme} style={{ height: "100%" }}>
      <AppUpdateProvider>
        <LaunchStatusProvider>
          <AppShell />
        </LaunchStatusProvider>
      </AppUpdateProvider>
    </FluentProvider>
  );
}
