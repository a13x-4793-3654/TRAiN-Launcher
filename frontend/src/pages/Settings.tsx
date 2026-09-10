import { useEffect, useState } from "react";
import {
  Body1,
  Caption1,
  Title2,
  Title3,
  Field,
  Input,
  InfoLabel,
  Button,
  Spinner,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { SaveRegular, FolderOpenRegular } from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

const TOASTER_ID = "settings-toaster";

interface AuthStatus {
  discord_display_name: string | null;
  microsoft_display_name: string | null;
}

interface SignInResult {
  display_name: string;
}

interface AppSettings {
  java_path: string | null;
  game_directory: string | null;
}

const useStyles = makeStyles({
  section: {
    marginTop: tokens.spacingVerticalXXL,
  },
  accountRow: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: tokens.spacingHorizontalM,
    marginTop: tokens.spacingVerticalM,
    padding: tokens.spacingVerticalM,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground2,
  },
  accountInfo: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXXS,
  },
  form: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalM,
    marginTop: tokens.spacingVerticalM,
    maxWidth: "480px",
  },
  pathRow: {
    display: "flex",
    alignItems: "center",
    gap: tokens.spacingHorizontalXS,
  },
  pathInput: {
    flexGrow: 1,
    minWidth: 0,
  },
  actions: {
    display: "flex",
    gap: tokens.spacingHorizontalS,
    marginTop: tokens.spacingVerticalS,
  },
});

/**
 * 設定画面。
 *
 * - アカウント管理: Discord / Microsoftのサインイン状態表示・サインイン・サインアウト。
 * - 起動設定: 既定のJavaパス、ゲームディレクトリ(ダウンロード先)の上書き。
 *   いずれもプロファイル個別の設定が優先され、ここでの値は「未指定時の既定値」として使われる。
 */
export function SettingsPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [authStatus, setAuthStatus] = useState<AuthStatus>({
    discord_display_name: null,
    microsoft_display_name: null,
  });
  const [discordSigningIn, setDiscordSigningIn] = useState(false);
  const [microsoftSigningIn, setMicrosoftSigningIn] = useState(false);

  const [javaPath, setJavaPath] = useState("");
  const [gameDirectory, setGameDirectory] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const refreshAuthStatus = () => {
    invoke<AuthStatus>("get_auth_status")
      .then(setAuthStatus)
      .catch((err) => console.error("failed to load auth status", err));
  };

  useEffect(() => {
    refreshAuthStatus();
    invoke<AppSettings>("get_app_settings")
      .then((settings) => {
        setJavaPath(settings.java_path ?? "");
        setGameDirectory(settings.game_directory ?? "");
      })
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>設定の読み込みに失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const notifyError = (title: string, err: unknown) =>
    dispatchToast(
      <Toast>
        <ToastTitle>{title}</ToastTitle>
        <ToastBody>{String(err)}</ToastBody>
      </Toast>,
      { intent: "error" },
    );

  const handleDiscordSignIn = () => {
    setDiscordSigningIn(true);
    invoke<SignInResult>("sign_in_with_discord")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Discordサインインに失敗しました", err))
      .finally(() => setDiscordSigningIn(false));
  };

  const handleMicrosoftSignIn = () => {
    setMicrosoftSigningIn(true);
    invoke<SignInResult>("sign_in_with_microsoft")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Microsoftサインインに失敗しました", err))
      .finally(() => setMicrosoftSigningIn(false));
  };

  const handleDiscordSignOut = () => {
    invoke("sign_out_discord")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Discordサインアウトに失敗しました", err));
  };

  const handleMicrosoftSignOut = () => {
    invoke("sign_out_microsoft")
      .then(() => refreshAuthStatus())
      .catch((err) => notifyError("Microsoftサインアウトに失敗しました", err));
  };

  const handleBrowseJavaPath = () => {
    open({
      multiple: false,
      directory: false,
      title: "Javaの実行ファイルを選択",
      filters:
        navigator.platform.toLowerCase().includes("win")
          ? [{ name: "Java実行ファイル", extensions: ["exe"] }]
          : undefined,
    })
      .then((selected) => {
        if (typeof selected === "string") {
          setJavaPath(selected);
        }
      })
      .catch((err) => notifyError("ファイルの選択に失敗しました", err));
  };

  const handleBrowseGameDirectory = () => {
    open({
      multiple: false,
      directory: true,
      title: "ゲームディレクトリを選択",
    })
      .then((selected) => {
        if (typeof selected === "string") {
          setGameDirectory(selected);
        }
      })
      .catch((err) => notifyError("フォルダーの選択に失敗しました", err));
  };

  const handleSave = () => {
    setSaving(true);
    invoke("save_app_settings", {
      settings: {
        java_path: javaPath.trim() || null,
        game_directory: gameDirectory.trim() || null,
      },
    })
      .then(() =>
        dispatchToast(
          <Toast>
            <ToastTitle>設定を保存しました</ToastTitle>
          </Toast>,
          { intent: "success" },
        ),
      )
      .catch((err) => notifyError("設定の保存に失敗しました", err))
      .finally(() => setSaving(false));
  };

  return (
    <div>
      <Title2 as="h2" block>
        設定
      </Title2>
      <Body1 as="p" block>
        アカウント管理、Javaランタイム、ダウンロード先を設定します。
      </Body1>

      <div className={styles.section}>
        <Title3 as="h3" block>
          アカウント
        </Title3>
        <Body1 as="p" block>
          サーバーへの参加にはDiscord、Minecraftの起動にはMicrosoftのサインインが必要です。
        </Body1>

        <div className={styles.accountRow}>
          <div className={styles.accountInfo}>
            <Body1 block>Discord</Body1>
            <Caption1 block>
              {authStatus.discord_display_name ?? "未サインイン"}
            </Caption1>
          </div>
          {authStatus.discord_display_name ? (
            <Button appearance="secondary" onClick={handleDiscordSignOut}>
              サインアウト
            </Button>
          ) : (
            <Button
              appearance="secondary"
              onClick={handleDiscordSignIn}
              disabled={discordSigningIn}
              icon={discordSigningIn ? <Spinner size="tiny" /> : undefined}
            >
              サインイン
            </Button>
          )}
        </div>

        <div className={styles.accountRow}>
          <div className={styles.accountInfo}>
            <Body1 block>Microsoft</Body1>
            <Caption1 block>
              {authStatus.microsoft_display_name ?? "未サインイン"}
            </Caption1>
          </div>
          {authStatus.microsoft_display_name ? (
            <Button appearance="secondary" onClick={handleMicrosoftSignOut}>
              サインアウト
            </Button>
          ) : (
            <Button
              appearance="primary"
              onClick={handleMicrosoftSignIn}
              disabled={microsoftSigningIn}
              icon={microsoftSigningIn ? <Spinner size="tiny" /> : undefined}
            >
              サインイン
            </Button>
          )}
        </div>
      </div>

      <div className={styles.section}>
        <Title3 as="h3" block>
          起動設定
        </Title3>
        <Body1 as="p" block>
          プロファイル個別に指定がない場合の既定値です。プロファイル側の設定がある場合はそちらが優先されます。
        </Body1>

        <div className={styles.form}>
          <Field
            label={
              <InfoLabel info="未指定の場合はPATH上のjavaを使用します">
                既定のJavaパス(任意)
              </InfoLabel>
            }
          >
            <div className={styles.pathRow}>
              <Input
                className={styles.pathInput}
                value={javaPath}
                onChange={(_event, data) => setJavaPath(data.value)}
                placeholder="例: C:\Program Files\Java\jdk-17\bin\java.exe"
                disabled={loading}
              />
              <Button
                appearance="secondary"
                icon={<FolderOpenRegular />}
                disabled={loading}
                onClick={handleBrowseJavaPath}
              >
                参照...
              </Button>
            </div>
          </Field>
          <Field
            label={
              <InfoLabel info="未指定の場合は公式Minecraft Launcherと共有する.minecraftフォルダを使用します(推奨)">
                ゲームディレクトリ(任意)
              </InfoLabel>
            }
          >
            <div className={styles.pathRow}>
              <Input
                className={styles.pathInput}
                value={gameDirectory}
                onChange={(_event, data) => setGameDirectory(data.value)}
                placeholder="例: D:\Games\minecraft"
                disabled={loading}
              />
              <Button
                appearance="secondary"
                icon={<FolderOpenRegular />}
                disabled={loading}
                onClick={handleBrowseGameDirectory}
              >
                参照...
              </Button>
            </div>
          </Field>
          <div className={styles.actions}>
            <Button
              appearance="primary"
              icon={saving ? <Spinner size="tiny" /> : <SaveRegular />}
              disabled={loading || saving}
              onClick={handleSave}
            >
              保存
            </Button>
          </div>
        </div>
      </div>
      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}
