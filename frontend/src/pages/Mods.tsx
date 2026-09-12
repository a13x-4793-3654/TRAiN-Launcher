import { useEffect, useState } from "react";
import {
  Body1,
  Title2,
  Title3,
  Field,
  Input,
  InfoLabel,
  Button,
  Spinner,
  Text,
  Card,
  CardHeader,
  Dropdown,
  Option,
  MessageBar,
  MessageBarBody,
  MessageBarTitle,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import {
  SearchRegular,
  ArrowDownloadRegular,
  DeleteRegular,
  WandRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { ModInstallWizardDialog } from "./ModWizard";

const TOASTER_ID = "mods-toaster";

/** 「共通(全プロファイル)」を表す内部値。呼び出し時は `profileId: null` に変換する。 */
const SHARED_PROFILE = "__shared__";

interface Profile {
  id: string;
  name: string;
}

interface ResolvedModPayload {
  provider: string;
  project_name: string;
  filename: string;
  is_dependency: boolean;
}

const useStyles = makeStyles({
  section: {
    marginTop: tokens.spacingVerticalXXL,
  },
  form: {
    display: "flex",
    gap: tokens.spacingHorizontalM,
    alignItems: "flex-end",
    flexWrap: "wrap",
    marginTop: tokens.spacingVerticalM,
  },
  urlField: {
    flex: 1,
    minWidth: "320px",
  },
  actions: {
    display: "flex",
    gap: tokens.spacingHorizontalS,
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
  preview: {
    marginTop: tokens.spacingVerticalM,
  },
  profileSelector: {
    marginTop: tokens.spacingVerticalM,
    maxWidth: "480px",
  },
  wizardRow: {
    marginTop: tokens.spacingVerticalM,
  },
});

/**
 * URL入力欄・Minecraftバージョン入力欄・解決プレビュー・導入済み一覧をまとめた
 * 汎用パネル。「Mod」「リソースパック」の両セクションで共有する。
 */
function InstallPanel(props: {
  title: string;
  description: string;
  urlPlaceholder: string;
  resolveCommand?: string;
  installCommand: string;
  listCommand: string;
  removeCommand: string;
  /** インストール結果として返る解決済みファイル一覧をどう解釈するか。 */
  installReturnsList: boolean;
  /** 対象プロファイルID。`null` の場合は全プロファイル共通のディレクトリを対象とする。 */
  profileId: string | null;
  /** この値が変化するたびに導入済み一覧を再取得する(ウィザードでの一括導入後の更新用)。 */
  refreshSignal?: number;
}) {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [url, setUrl] = useState("");
  const [minecraftVersion, setMinecraftVersion] = useState("");
  const [resolving, setResolving] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [preview, setPreview] = useState<ResolvedModPayload[] | null>(null);

  const [installed, setInstalled] = useState<string[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  const [removing, setRemoving] = useState<string | null>(null);

  const refreshInstalled = () => {
    setInstalledLoading(true);
    invoke<string[]>(props.listCommand, { profileId: props.profileId })
      .then(setInstalled)
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>一覧の取得に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setInstalledLoading(false));
  };

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refreshInstalled, [props.profileId, props.refreshSignal]);

  const handleResolve = () => {
    if (!url.trim() || !props.resolveCommand) {
      return;
    }
    setResolving(true);
    setPreview(null);
    invoke<ResolvedModPayload[]>(props.resolveCommand, {
      url: url.trim(),
      minecraftVersion: minecraftVersion.trim() || null,
    })
      .then(setPreview)
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>解決に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setResolving(false));
  };

  const handleInstall = () => {
    if (!url.trim()) {
      return;
    }
    setInstalling(true);
    invoke(props.installCommand, {
      url: url.trim(),
      minecraftVersion: minecraftVersion.trim() || null,
      profileId: props.profileId,
    })
      .then((result) => {
        const count = props.installReturnsList
          ? (result as string[]).length
          : 1;
        dispatchToast(
          <Toast>
            <ToastTitle>導入しました</ToastTitle>
            <ToastBody>{count}件のファイルを導入しました</ToastBody>
          </Toast>,
          { intent: "success" },
        );
        setUrl("");
        setPreview(null);
        refreshInstalled();
      })
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>導入に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setInstalling(false));
  };

  const handleRemove = (filename: string) => {
    setRemoving(filename);
    invoke(props.removeCommand, { filename, profileId: props.profileId })
      .then(() => refreshInstalled())
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>削除に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setRemoving(null));
  };

  return (
    <div className={styles.section}>
      <Title3 as="h3" block>
        {props.title}
      </Title3>
      <Body1 as="p" block>
        {props.description}
      </Body1>

      <div className={styles.form}>
        <Field label="Modrinth / CurseForge のURL" className={styles.urlField}>
          <Input
            value={url}
            onChange={(_event, data) => setUrl(data.value)}
            placeholder={props.urlPlaceholder}
          />
        </Field>
        <Field
          label={
            <InfoLabel info="未指定の場合は最新の互換バージョンを使用します">
              対象Minecraftバージョン(任意)
            </InfoLabel>
          }
        >
          <Input
            value={minecraftVersion}
            onChange={(_event, data) => setMinecraftVersion(data.value)}
            placeholder="例: 1.20.4"
          />
        </Field>
        <div className={styles.actions}>
          {props.resolveCommand && (
            <Button
              icon={resolving ? <Spinner size="tiny" /> : <SearchRegular />}
              disabled={!url.trim() || resolving || installing}
              onClick={handleResolve}
            >
              確認
            </Button>
          )}
          <Button
            appearance="primary"
            icon={
              installing ? <Spinner size="tiny" /> : <ArrowDownloadRegular />
            }
            disabled={!url.trim() || resolving || installing}
            onClick={handleInstall}
          >
            導入
          </Button>
        </div>
      </div>

      {preview && (
        <div className={styles.preview}>
          {preview.length === 0 ? (
            <MessageBar intent="warning">
              <MessageBarBody>
                <MessageBarTitle>解決できませんでした</MessageBarTitle>
              </MessageBarBody>
            </MessageBar>
          ) : (
            <MessageBar intent="info">
              <MessageBarBody>
                <MessageBarTitle>
                  {preview.length}件のファイルが導入対象です
                </MessageBarTitle>
                {preview.map((item) => (
                  <div key={`${item.provider}-${item.filename}`}>
                    {item.project_name} ({item.filename})
                    {item.is_dependency ? " ・ 依存Mod" : ""} ・ {item.provider}
                  </div>
                ))}
              </MessageBarBody>
            </MessageBar>
          )}
        </div>
      )}

      <div className={styles.list}>
        {installedLoading ? (
          <Spinner size="small" label="読み込み中..." />
        ) : installed.length === 0 ? (
          <Body1 as="p" block>
            導入済みのファイルはありません。
          </Body1>
        ) : (
          installed.map((filename) => (
            <Card key={filename}>
              <CardHeader
                header={<Text weight="semibold">{filename}</Text>}
                action={
                  <div className={styles.cardActions}>
                    <Button
                      appearance="subtle"
                      size="small"
                      icon={
                        removing === filename ? (
                          <Spinner size="tiny" />
                        ) : (
                          <DeleteRegular />
                        )
                      }
                      disabled={removing !== null}
                      onClick={() => handleRemove(filename)}
                    >
                      削除
                    </Button>
                  </div>
                }
              />
            </Card>
          ))
        )}
      </div>
    </div>
  );
}

/**
 * Mod / リソースパック画面。
 *
 * Modrinth・CurseForgeのURLを指定してMod/リソースパックを解決・依存関係の自動解決込みで
 * 導入する。導入先は対象プロファイル選択欄で選んだプロファイル専用のゲームディレクトリ
 * (未設定の場合は公式Minecraft Launcherと共有する `.minecraft`
 * フォルダ)。「共通(全プロファイル)」を選ぶと、専用ゲームディレクトリを持たない
 * プロファイル全てに適用される既定の場所を対象にする。「複数のModをまとめて導入」
 * ボタンから、複数URLを一括で解決・導入できる「Mod導入ウィザード」(`ModWizard.tsx`)も
 * 開ける。CurseForgeのURLを解決するには
 * 環境変数 `TRAIN_LAUNCHER_CURSEFORGE_API_KEY` の設定が必要(README参照)。
 */
export function ModsPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<string>(SHARED_PROFILE);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [modsRefreshSignal, setModsRefreshSignal] = useState(0);

  useEffect(() => {
    invoke<Profile[]>("list_profiles")
      .then(setProfiles)
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>プロファイルの取得に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const profileId = selectedProfile === SHARED_PROFILE ? null : selectedProfile;
  const selectedProfileName =
    profiles.find((profile) => profile.id === selectedProfile)?.name ??
    "共通(全プロファイル)";

  return (
    <div>
      <Title2 as="h2" block>
        Mod / リソースパック
      </Title2>
      <Body1 as="p" block>
        Modrinth・CurseForgeのURLを指定してMod・リソースパックを導入します。
        依存Modも自動的に解決してまとめて導入します。
      </Body1>

      <div className={styles.wizardRow}>
        <Button
          appearance="secondary"
          icon={<WandRegular />}
          onClick={() => setWizardOpen(true)}
        >
          複数のModをまとめて導入(ウィザード)
        </Button>
      </div>

      <Field
        label={
          <InfoLabel info="専用ゲームディレクトリを設定していないプロファイルは「共通(全プロファイル)」と同じ場所を参照します">
            対象プロファイル
          </InfoLabel>
        }
        className={styles.profileSelector}
      >
        <Dropdown
          value={selectedProfileName}
          selectedOptions={[selectedProfile]}
          onOptionSelect={(_event, data) =>
            setSelectedProfile(data.optionValue ?? SHARED_PROFILE)
          }
        >
          <Option key={SHARED_PROFILE} value={SHARED_PROFILE}>
            共通(全プロファイル)
          </Option>
          {profiles.map((profile) => (
            <Option key={profile.id} value={profile.id} text={profile.name}>
              {profile.name}
            </Option>
          ))}
        </Dropdown>
      </Field>

      <InstallPanel
        title="Mod"
        description="Modrinthまたは CurseForge のMod詳細ページのURLを貼り付けてください。"
        urlPlaceholder="例: https://modrinth.com/mod/sodium"
        resolveCommand="resolve_mod_url"
        installCommand="install_mod"
        listCommand="list_installed_mods"
        removeCommand="remove_installed_mod"
        installReturnsList={true}
        profileId={profileId}
        refreshSignal={modsRefreshSignal}
      />

      <InstallPanel
        title="リソースパック"
        description="Modrinthまたは CurseForge のリソースパック詳細ページのURLを貼り付けてください。"
        urlPlaceholder="例: https://modrinth.com/resourcepack/faithful-64x"
        installCommand="install_resource_pack"
        listCommand="list_installed_resource_packs"
        removeCommand="remove_installed_resource_pack"
        installReturnsList={false}
        profileId={profileId}
      />

      <div className={styles.section} />
      <ModInstallWizardDialog
        open={wizardOpen}
        onOpenChange={setWizardOpen}
        profileId={profileId}
        onInstalled={() => setModsRefreshSignal((value) => value + 1)}
      />
      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}
