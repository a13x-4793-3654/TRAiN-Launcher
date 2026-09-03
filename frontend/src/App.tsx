import { useState } from "react";
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
} from "@fluentui/react-components";
import type { SelectTabEventHandler } from "@fluentui/react-components";
import {
  HomeRegular,
  ServerRegular,
  PersonRegular,
  SettingsRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { useSystemTheme } from "./useSystemTheme";
import { HomePage } from "./pages/Home";
import { ServersPage } from "./pages/Servers";
import { ProfilesPage } from "./pages/Profiles";
import { SettingsPage } from "./pages/Settings";

const TOASTER_ID = "train-launcher-toaster";

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

type NavKey = "home" | "servers" | "profiles" | "settings";

// TODO: サインイン実装後、この4画面に加えて「所属サーバー詳細」「Mod導入ウィザード」等の
// サブ画面を追加する。現時点ではプレースホルダーの切り替えのみ。
function AppShell() {
  const styles = useStyles();
  const [selected, setSelected] = useState<NavKey>("home");
  const { dispatchToast } = useToastController(TOASTER_ID);

  const onTabSelect: SelectTabEventHandler = (_event, data) => {
    setSelected(data.value as NavKey);
  };

  // TODO: 実際のOAuth2/MSA認証フロー完成後、`invoke` の戻り値でサインイン状態を
  // グローバルなアプリ状態(Contextやストア)に反映する。現時点ではトースト表示のみ。
  const notify = (provider: string, promise: Promise<string>) => {
    promise
      .then((message) =>
        dispatchToast(
          <Toast>
            <ToastTitle>{provider}でサインイン</ToastTitle>
            <ToastBody>{message}</ToastBody>
          </Toast>,
          { intent: "success" },
        ),
      )
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>{provider}でサインイン</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      );
  };

  const handleDiscordSignIn = () =>
    notify("Discord", invoke<string>("sign_in_with_discord"));
  const handleMicrosoftSignIn = () =>
    notify("Microsoft", invoke<string>("sign_in_with_microsoft"));

  return (
    <div className={styles.root}>
      <header className={styles.header}>
        <Title1 as="h1">TRAiN Launcher</Title1>
        <div className={styles.headerActions}>
          <Button appearance="secondary" onClick={handleDiscordSignIn}>
            Discordでサインイン
          </Button>
          <Button appearance="primary" onClick={handleMicrosoftSignIn}>
            Microsoftでサインイン
          </Button>
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
