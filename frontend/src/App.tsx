import { useEffect, useState } from "react";
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
} from "@fluentui/react-components";
import type { SelectTabEventHandler } from "@fluentui/react-components";
import {
  HomeRegular,
  ServerRegular,
  PersonRegular,
  AppsRegular,
  SettingsRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { useSystemTheme } from "./useSystemTheme";
import { trainLightTheme, trainDarkTheme } from "./theme";
import { HomePage } from "./pages/Home";
import { ServersPage } from "./pages/Servers";
import { ProfilesPage } from "./pages/Profiles";
import { ModsPage } from "./pages/Mods";
import { SettingsPage } from "./pages/Settings";

const TOASTER_ID = "train-launcher-toaster";

interface SignInResult {
  display_name: string;
}

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
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

type NavKey = "home" | "servers" | "profiles" | "mods" | "settings";

// TODO: サインイン実装後、この4画面に加えて「所属サーバー詳細」「Mod導入ウィザード」等の
// サブ画面を追加する。現時点ではプレースホルダーの切り替えのみ。
function AppShell() {
  const styles = useStyles();
  const [selected, setSelected] = useState<NavKey>("home");
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [authStatus, setAuthStatus] = useState<AuthStatus>({
    discord_display_name: null,
    microsoft_display_name: null,
  });
  const [discordSigningIn, setDiscordSigningIn] = useState(false);
  const [microsoftSigningIn, setMicrosoftSigningIn] = useState(false);

  const refreshAuthStatus = () => {
    invoke<AuthStatus>("get_auth_status")
      .then(setAuthStatus)
      .catch((err) => console.error("failed to load auth status", err));
  };

  // 起動時に保存済みセッション(keyring)からサインイン状態を復元する。
  useEffect(() => {
    refreshAuthStatus();
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
            <Tab value="settings" icon={<SettingsRegular />}>
              設定
            </Tab>
          </TabList>
        </nav>
        <main className={styles.content}>
          {selected === "home" && <HomePage />}
          {selected === "servers" && <ServersPage />}
          {selected === "profiles" && <ProfilesPage />}
          {selected === "mods" && <ModsPage />}
          {selected === "settings" && <SettingsPage />}
        </main>
      </div>
      <Toaster toasterId={TOASTER_ID} />
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
      <AppShell />
    </FluentProvider>
  );
}
