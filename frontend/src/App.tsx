import { useEffect, useState } from "react";
import {
  FluentProvider,
  webLightTheme,
  webDarkTheme,
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
  Dialog,
  DialogSurface,
  DialogTitle,
  DialogBody,
  DialogContent,
  DialogActions,
  DialogTrigger,
  Spinner,
  Text,
  Body1Strong,
} from "@fluentui/react-components";
import type { SelectTabEventHandler } from "@fluentui/react-components";
import {
  HomeRegular,
  ServerRegular,
  PersonRegular,
  SettingsRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSystemTheme } from "./useSystemTheme";
import { HomePage } from "./pages/Home";
import { ServersPage } from "./pages/Servers";
import { ProfilesPage } from "./pages/Profiles";
import { SettingsPage } from "./pages/Settings";

const TOASTER_ID = "train-launcher-toaster";
const MSA_DEVICE_CODE_EVENT = "msa://device-code";

interface SignInResult {
  display_name: string;
}

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
}

interface MsaDeviceCodePayload {
  verification_uri: string;
  user_code: string;
  expires_in_secs: number;
}

const useStyles = makeStyles({
  root: {
    display: "flex",
    flexDirection: "column",
    height: "100vh",
    width: "100vw",
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
  deviceCode: {
    fontSize: tokens.fontSizeHero800,
    letterSpacing: "0.2em",
    textAlign: "center",
    padding: tokens.spacingVerticalM,
  },
});

type NavKey = "home" | "servers" | "profiles" | "settings";

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
  const [deviceCode, setDeviceCode] = useState<MsaDeviceCodePayload | null>(
    null,
  );

  const refreshAuthStatus = () => {
    invoke<AuthStatus>("get_auth_status")
      .then(setAuthStatus)
      .catch((err) => console.error("failed to load auth status", err));
  };

  // 起動時に保存済みセッション(keyring)からサインイン状態を復元する。
  useEffect(() => {
    refreshAuthStatus();
  }, []);

  // MSAデバイスコードフロー中、Rust側から届く verification_uri/user_code を表示する。
  useEffect(() => {
    const unlisten = listen<MsaDeviceCodePayload>(
      MSA_DEVICE_CODE_EVENT,
      (event) => setDeviceCode(event.payload),
    );
    return () => {
      unlisten.then((fn) => fn());
    };
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
      .finally(() => {
        setMicrosoftSigningIn(false);
        setDeviceCode(null);
      });
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
        <Title1 as="h1">TRAiN Launcher</Title1>
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
            <Tab value="settings" icon={<SettingsRegular />}>
              設定
            </Tab>
          </TabList>
        </nav>
        <main className={styles.content}>
          {selected === "home" && <HomePage />}
          {selected === "servers" && <ServersPage />}
          {selected === "profiles" && <ProfilesPage />}
          {selected === "settings" && <SettingsPage />}
        </main>
      </div>
      <Toaster toasterId={TOASTER_ID} />
      <Dialog open={microsoftSigningIn && deviceCode !== null}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>Microsoftアカウントでサインイン</DialogTitle>
            <DialogContent>
              <Text>
                ブラウザで以下のURLを開き、表示されたコードを入力してください。
              </Text>
              {deviceCode && (
                <>
                  <Body1Strong as="p">
                    {deviceCode.verification_uri}
                  </Body1Strong>
                  <div className={styles.deviceCode}>
                    {deviceCode.user_code}
                  </div>
                </>
              )}
              <Text>認可が完了するまでこのダイアログは自動的に閉じます。</Text>
            </DialogContent>
            <DialogActions>
              <DialogTrigger disableButtonEnhancement>
                <Button
                  appearance="secondary"
                  onClick={() => {
                    setMicrosoftSigningIn(false);
                    setDeviceCode(null);
                  }}
                >
                  キャンセル(表示を閉じる)
                </Button>
              </DialogTrigger>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}

export default function App() {
  // OSのライト/ダーク設定に自動追従する(TODO: 将来的に設定画面から手動上書きできるようにする)。
  const scheme = useSystemTheme();
  const theme = scheme === "dark" ? webDarkTheme : webLightTheme;

  return (
    <FluentProvider theme={theme} style={{ height: "100%" }}>
      <AppShell />
    </FluentProvider>
  );
}
