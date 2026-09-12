import { useEffect, useRef, useState } from "react";
import {
  Badge,
  Body1,
  Button,
  Caption1,
  Card,
  Checkbox,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Field,
  MessageBar,
  MessageBarBody,
  Option,
  Spinner,
  Text,
  Title2,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { FolderOpenRegular } from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import {
  GameActivityNotice,
  runGameDataOperation,
  useGameActivity,
} from "../GameActivity";
import type {
  BackupInfo,
  BackupList,
  BackupScope,
  RestoreResult,
} from "../gameDataTypes";

interface Profile {
  id: string;
  name: string;
  minecraft_version: string;
  mod_loader: string | null;
  game_dir: string | null;
}

interface RestoreSelection {
  backup: BackupInfo;
  destination: Profile;
}

const scopeLabels: Record<BackupScope, string> = {
  settings: "標準設定のみ",
  full: "ゲームデータ全体",
};

const useStyles = makeStyles({
  page: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalM,
    minWidth: 0,
    overflowWrap: "anywhere",
  },
  row: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    flexWrap: "wrap",
    gap: tokens.spacingHorizontalM,
    minWidth: 0,
  },
  actions: {
    display: "flex",
    flexWrap: "wrap",
    gap: tokens.spacingHorizontalS,
  },
  column: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalS,
    minWidth: 0,
    overflowWrap: "anywhere",
  },
  dropdown: {
    width: "100%",
    minWidth: 0,
  },
  fields: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fit, minmax(min(100%, 260px), 1fr))",
    gap: tokens.spacingHorizontalM,
  },
  warnings: {
    marginTop: tokens.spacingVerticalXS,
    marginBottom: 0,
    paddingLeft: tokens.spacingHorizontalL,
  },
});

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat("ja-JP", {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(date);
}

function formatSize(bytes: number) {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "サイズ不明";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = bytes > 0
    ? Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
    : 0;
  return `${(bytes / 1024 ** index).toLocaleString("ja-JP", {
    maximumFractionDigits: index === 0 ? 0 : 1,
  })} ${units[index]}`;
}

function profileLabel(profile: Profile) {
  return `${profile.name} — ${profile.minecraft_version} / ${profile.mod_loader ?? "バニラ"}`;
}

export function BackupsPage() {
  const styles = useStyles();
  const activityState = useGameActivity();
  const { activity, error: activityError } = activityState;
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(true);
  const [profilesError, setProfilesError] = useState<string | null>(null);
  const [backupList, setBackupList] = useState<BackupList>({ backups: [], warnings: [] });
  const [backupsLoading, setBackupsLoading] = useState(true);
  const [backupsError, setBackupsError] = useState<string | null>(null);
  const [profileId, setProfileId] = useState("");
  const [scope, setScope] = useState<BackupScope>("settings");
  const [minecraftClosed, setMinecraftClosed] = useState(false);
  const [busy, setBusy] = useState<"create" | "restore" | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ message: string; warnings: string[] } | null>(null);
  const [restoreSelection, setRestoreSelection] = useState<RestoreSelection | null>(null);
  const [restoreClosed, setRestoreClosed] = useState(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [folderOpening, setFolderOpening] = useState(false);
  const [folderError, setFolderError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const mounted = useRef(false);
  const pending = useRef(false);
  const folderPending = useRef(false);
  const selectedProfile = profiles.find((profile) => profile.id === profileId);
  const blocked =
    !activity ||
    activityError !== null ||
    activity.running_games > 0 ||
    activity.file_operation;
  const profileUnavailable = !selectedProfile || profilesLoading || profilesError !== null;
  const restoreUnavailable =
    blocked || busy !== null || profileUnavailable || backupsLoading || backupsError !== null;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    setProfilesLoading(true);
    setBackupsLoading(true);
    setProfilesError(null);
    setBackupsError(null);

    invoke<Profile[]>("list_profiles", {})
      .then((result) => {
        if (!disposed) {
          setProfiles(result);
          setProfileId((current) =>
            result.some((profile) => profile.id === current) ? current : "",
          );
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setProfilesError(String(error));
        }
      })
      .finally(() => {
        if (!disposed) {
          setProfilesLoading(false);
        }
      });
    invoke<BackupList>("list_profile_backups", {})
      .then((result) => {
        if (!disposed) {
          setBackupList(result);
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setBackupsError(String(error));
        }
      })
      .finally(() => {
        if (!disposed) {
          setBackupsLoading(false);
        }
      });

    return () => {
      disposed = true;
    };
  }, [reload]);

  function refresh() {
    setProfilesLoading(true);
    setBackupsLoading(true);
    setReload((value) => value + 1);
  }

  async function createBackup() {
    if (pending.current || blocked || profileUnavailable || !selectedProfile || !minecraftClosed) {
      return;
    }
    pending.current = true;
    setBusy("create");
    setOperationError(null);
    setNotice(null);
    try {
      const backup = await runGameDataOperation(() =>
        invoke<BackupInfo>("create_profile_backup", {
          profileId: selectedProfile.id,
          scope,
        }),
      );
      if (mounted.current) {
        setNotice({
          message: `${backup.profile.name}の${scopeLabels[backup.scope]}を保存しました（${formatSize(backup.size_bytes)}）。`,
          warnings: [],
        });
        setMinecraftClosed(false);
        refresh();
      }
    } catch (error) {
      if (mounted.current) {
        setOperationError(String(error));
      }
    } finally {
      pending.current = false;
      if (mounted.current) {
        setBusy(null);
      }
    }
  }

  async function restoreBackup() {
    if (pending.current || restoreUnavailable || !restoreSelection || !restoreClosed) {
      return;
    }
    pending.current = true;
    setBusy("restore");
    setRestoreError(null);
    setNotice(null);
    try {
      const result = await runGameDataOperation(() =>
        invoke<RestoreResult>("restore_profile_backup", {
          backupId: restoreSelection.backup.id,
          profileId: restoreSelection.destination.id,
        }),
      );
      if (mounted.current) {
        setNotice({
          message: `${restoreSelection.destination.name}に${scopeLabels[result.backup.scope]}を復元しました。${
            result.safety_backup
              ? "復元前のデータを自動バックアップしました。"
              : "復元前の対象データが空のため、自動バックアップは作成されませんでした。"
          }`,
          warnings: result.warnings,
        });
        setRestoreSelection(null);
        setRestoreClosed(false);
        setMinecraftClosed(false);
        refresh();
      }
    } catch (error) {
      if (mounted.current) {
        setRestoreError(String(error));
      }
    } finally {
      pending.current = false;
      if (mounted.current) {
        setBusy(null);
      }
    }
  }

  async function openBackupDirectory() {
    if (folderPending.current || pending.current || blocked) {
      return;
    }
    folderPending.current = true;
    setFolderOpening(true);
    setFolderError(null);
    try {
      await invoke<void>("open_backup_directory", {});
    } catch (error) {
      if (mounted.current) {
        setFolderError(String(error));
      }
    } finally {
      folderPending.current = false;
      if (mounted.current) {
        setFolderOpening(false);
      }
    }
  }

  function closeRestore() {
    if (!pending.current) {
      setRestoreSelection(null);
      setRestoreClosed(false);
      setRestoreError(null);
    }
  }

  return (
    <div className={styles.page}>
      <div className={styles.row}>
        <Title2>バックアップ</Title2>
        <div className={styles.actions}>
          <Button
            icon={<FolderOpenRegular />}
            disabled={blocked || busy !== null || folderOpening}
            onClick={() => void openBackupDirectory()}
          >
            {folderOpening ? "フォルダーを開いています..." : "保存フォルダーを開く"}
          </Button>
          <Button
            disabled={busy !== null || activity?.file_operation || profilesLoading || backupsLoading}
            onClick={refresh}
          >
            再読み込み
          </Button>
        </div>
      </div>
      <Body1>
        バックアップはこのPCに保存されます。ゲームデータには個人情報が含まれる場合があります。
      </Body1>
      <GameActivityNotice {...activityState} />
      {folderError && (
        <MessageBar intent="error">
          <MessageBarBody>保存フォルダーを開けませんでした。{folderError}</MessageBarBody>
        </MessageBar>
      )}
      {profilesError && (
        <MessageBar intent="error">
          <MessageBarBody>プロファイルを取得できませんでした。{profilesError}</MessageBarBody>
        </MessageBar>
      )}
      <Card>
        <Text weight="semibold">作成元・復元先を選ぶ</Text>
        <Body1>
          選択したプロファイルのデータを保存します。下の一覧から復元する場合も、
          このプロファイルが復元先になります。
        </Body1>
        {profilesLoading && <Spinner size="tiny" label="プロファイルを読み込み中..." />}
        {!profilesLoading && !profilesError && profiles.length === 0 && (
          <MessageBar intent="info">
            <MessageBarBody>
              プロファイルがありません。プロファイル画面で作成すると、保存・復元できます。
            </MessageBarBody>
          </MessageBar>
        )}
        <div className={styles.fields}>
          <Field label="作成元 / 復元先のプロファイル" required>
            <Dropdown
              className={styles.dropdown}
              placeholder="プロファイルを選択"
              value={selectedProfile ? profileLabel(selectedProfile) : ""}
              selectedOptions={selectedProfile ? [selectedProfile.id] : []}
              disabled={busy !== null || profilesLoading || profilesError !== null}
              onOptionSelect={(_event, data) => {
                setProfileId(data.optionValue ?? "");
                setMinecraftClosed(false);
                setOperationError(null);
              }}
            >
              {profiles.map((profile) => (
                <Option key={profile.id} value={profile.id} text={profileLabel(profile)}>
                  {profileLabel(profile)}
                </Option>
              ))}
            </Dropdown>
          </Field>
          <Field label="作成するバックアップの範囲" required>
            <Dropdown
              className={styles.dropdown}
              value={scopeLabels[scope]}
              selectedOptions={[scope]}
              disabled={busy !== null}
              onOptionSelect={(_event, data) => {
                if (data.optionValue === "settings" || data.optionValue === "full") {
                  setScope(data.optionValue);
                  setMinecraftClosed(false);
                  setOperationError(null);
                }
              }}
            >
              <Option value="settings">標準設定のみ</Option>
              <Option value="full">ゲームデータ全体</Option>
            </Dropdown>
          </Field>
        </div>
        {selectedProfile && (
          <Caption1>
            ゲームフォルダー: {selectedProfile.game_dir ?? "既定のMinecraftフォルダー"}
          </Caption1>
        )}
        <Body1>
          標準設定のみ: options.txtのキー割り当て・音量・画面設定などを保存します。
          復元先のパック選択や前回の接続先は変更しません。
        </Body1>
        <Body1>
          ゲームデータ全体: saves、mods、resourcepacks、configなど、その他も含めた
          ゲームデータを保存します。公式ランチャーの認証情報、共有のassets / libraries /
          versions / runtime、ログは含めません。
        </Body1>
        <Checkbox
          checked={minecraftClosed}
          disabled={busy !== null || profileUnavailable}
          onChange={(_event, data) => setMinecraftClosed(data.checked === true)}
          label="TRAiN以外から起動したものも含め、すべてのMinecraftを終了しました"
        />
        <Caption1>TRAiN以外から起動したMinecraftは検出できません。</Caption1>
        {operationError && (
          <MessageBar intent="error">
            <MessageBarBody>バックアップを作成できませんでした。{operationError}</MessageBarBody>
          </MessageBar>
        )}
        <div className={styles.actions}>
          <Button
            appearance="primary"
            disabled={blocked || busy !== null || profileUnavailable || !minecraftClosed}
            onClick={() => void createBackup()}
          >
            {busy === "create" ? "保存中..." : "バックアップを作成"}
          </Button>
        </div>
      </Card>
      {notice && (
        <>
          <MessageBar intent="success">
            <MessageBarBody>{notice.message}</MessageBarBody>
          </MessageBar>
          {notice.warnings.length > 0 && (
            <MessageBar intent="warning">
              <MessageBarBody>
                復元結果のお知らせ
                <ul className={styles.warnings}>
                  {notice.warnings.map((warning, index) => (
                    <li key={`${index}:${warning}`}>{warning}</li>
                  ))}
                </ul>
              </MessageBarBody>
            </MessageBar>
          )}
        </>
      )}
      <Text as="h2" size={500} weight="semibold">保存済みバックアップ</Text>
      <Body1>
        すべてのプロファイルのバックアップを表示しています。別のプロファイルにも復元できます。
        Minecraftの継承元を含む実際のバージョンが同じ必要があり、全体復元ではModローダーも
        同じ必要があります。互換性は復元時に確認します。
      </Body1>
      {backupsLoading && <Spinner size="small" label="バックアップを読み込み中..." />}
      {backupsError && (
        <MessageBar intent="error">
          <MessageBarBody>バックアップ一覧を取得できませんでした。{backupsError}</MessageBarBody>
        </MessageBar>
      )}
      {backupList.warnings.length > 0 && (
        <MessageBar intent="warning">
          <MessageBarBody>
            一覧の読み込みに関するお知らせ
            <ul className={styles.warnings}>
              {backupList.warnings.map((warning, index) => (
                <li key={`${index}:${warning}`}>{warning}</li>
              ))}
            </ul>
          </MessageBarBody>
        </MessageBar>
      )}
      {!backupsLoading && !backupsError && backupList.backups.length === 0 && (
        <Body1>バックアップはまだありません。上でプロファイルを選んで作成してください。</Body1>
      )}
      {backupList.backups.map((backup) => (
        <Card key={backup.id}>
          <div className={styles.row}>
            <div className={styles.column}>
              <Text weight="semibold">{backup.profile.name}</Text>
              <Caption1>
                <time dateTime={backup.created_at}>{formatDate(backup.created_at)}</time>
                {" / "}{formatSize(backup.size_bytes)}
              </Caption1>
              <Caption1>
                Minecraft {backup.profile.minecraft_version} / {backup.profile.mod_loader ?? "バニラ"}
              </Caption1>
              <div className={styles.actions}>
                <Badge appearance="outline">{scopeLabels[backup.scope]}</Badge>
                <Badge appearance="tint" color={backup.automatic ? "informative" : "subtle"}>
                  {backup.automatic ? "自動バックアップ" : "手動バックアップ"}
                </Badge>
              </div>
            </div>
            <Button
              disabled={restoreUnavailable}
              aria-label={`${backup.profile.name}の${formatDate(backup.created_at)}のバックアップを復元`}
              onClick={() => {
                if (selectedProfile && !restoreUnavailable && !pending.current) {
                  setRestoreSelection({ backup, destination: selectedProfile });
                  setRestoreClosed(false);
                  setRestoreError(null);
                }
              }}
            >
              復元...
            </Button>
          </div>
        </Card>
      ))}
      {restoreSelection && (
        <Dialog
          open
          modalType={busy === "restore" ? "alert" : "modal"}
          onOpenChange={(_event, data) => {
            if (!data.open) {
              closeRestore();
            }
          }}
        >
          <DialogSurface>
            <DialogBody>
              <DialogTitle>バックアップを復元しますか？</DialogTitle>
              <DialogContent>
                <div className={styles.column}>
                  <Body1>
                    バックアップ: <Text weight="semibold">{restoreSelection.backup.profile.name}</Text>
                    {" / "}{formatDate(restoreSelection.backup.created_at)}
                    {" / "}{scopeLabels[restoreSelection.backup.scope]}
                  </Body1>
                  <Caption1>
                    Minecraft {restoreSelection.backup.profile.minecraft_version}
                    {" / "}{restoreSelection.backup.profile.mod_loader ?? "バニラ"}
                  </Caption1>
                  <Body1>
                    復元先: <Text weight="semibold">{profileLabel(restoreSelection.destination)}</Text>
                  </Body1>
                  <Caption1>
                    {restoreSelection.destination.game_dir ?? "既定のMinecraftフォルダー"}
                  </Caption1>
                  {restoreSelection.backup.scope === "full" ? (
                    <MessageBar intent="warning">
                      <MessageBarBody>
                        復元対象の現在のゲームデータを、バックアップ時点の内容に置き換えます。
                        バックアップ後に追加したワールドやファイルも削除されます。
                        サーバー管理のMod・リソースパックは、次回の起動時にサーバーの構成へ
                        再同期されます。同じMinecraftバージョンとModローダーが必要です。
                      </MessageBarBody>
                    </MessageBar>
                  ) : (
                    <Body1>
                      標準設定を復元します。復元先のパック選択
                      （resourcePacks / incompatibleResourcePacks）と前回の接続先
                      （lastServer）は保持します。Mod、ワールド、リソースパックの実ファイルや
                      configは変更しません。同じMinecraftバージョンが必要です。
                    </Body1>
                  )}
                  <Body1>
                    変更前に、復元先の対象データを自動で安全用バックアップに保存します。
                    対象データが空の場合は作成しません。
                  </Body1>
                  <GameActivityNotice {...activityState} />
                  <Checkbox
                    checked={restoreClosed}
                    disabled={busy !== null}
                    onChange={(_event, data) => setRestoreClosed(data.checked === true)}
                    label="TRAiN以外から起動したものも含め、すべてのMinecraftを終了しました"
                  />
                  <Caption1>TRAiN以外から起動したMinecraftは検出できません。</Caption1>
                  {restoreError && (
                    <MessageBar intent="error">
                      <MessageBarBody>復元できませんでした。{restoreError}</MessageBarBody>
                    </MessageBar>
                  )}
                </div>
              </DialogContent>
              <DialogActions className={styles.actions}>
                <Button disabled={busy !== null} onClick={closeRestore}>キャンセル</Button>
                <Button
                  appearance="primary"
                  disabled={restoreUnavailable || !restoreClosed}
                  onClick={() => void restoreBackup()}
                >
                  {busy === "restore" ? "復元中..." : "このプロファイルに復元"}
                </Button>
              </DialogActions>
            </DialogBody>
          </DialogSurface>
        </Dialog>
      )}
    </div>
  );
}
