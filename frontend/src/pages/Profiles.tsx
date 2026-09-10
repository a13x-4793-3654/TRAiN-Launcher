import { useEffect, useState } from "react";
import {
  Body1,
  Title2,
  Field,
  Input,
  Button,
  Spinner,
  ProgressBar,
  Caption1,
  Text,
  Card,
  CardHeader,
  Combobox,
  Option,
  Dialog,
  DialogSurface,
  DialogTitle,
  DialogBody,
  DialogContent,
  DialogActions,
  Toaster,
  useToastController,
  Toast,
  ToastTitle,
  ToastBody,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import {
  AddRegular,
  EditRegular,
  DeleteRegular,
  PlayRegular,
} from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const TOASTER_ID = "profiles-toaster";
const GAME_EXITED_EVENT = "game://exited";
const LAUNCH_PROGRESS_EVENT = "launch://progress";

interface Profile {
  id: string;
  name: string;
  minecraft_version: string;
  mod_loader: string | null;
  server_id: string | null;
  java_path: string | null;
  max_memory_mb: number | null;
  source: "train" | "official";
}

interface VersionEntry {
  id: string;
  type: string;
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

/** プロファイル作成/編集ダイアログの入力状態(数値項目は文字列で保持しバリデーションは送信時に行う)。 */
interface ProfileFormState {
  id: string | null; // nullの場合は新規作成
  name: string;
  minecraftVersion: string;
  javaPath: string;
  maxMemoryMb: string;
}

const EMPTY_FORM: ProfileFormState = {
  id: null,
  name: "",
  minecraftVersion: "",
  javaPath: "",
  maxMemoryMb: "",
};

const useStyles = makeStyles({
  toolbar: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginTop: tokens.spacingVerticalL,
    marginBottom: tokens.spacingVerticalM,
  },
  list: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalS,
  },
  cardActions: {
    display: "flex",
    gap: tokens.spacingHorizontalXS,
  },
  progressArea: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
    marginTop: tokens.spacingVerticalS,
  },
  form: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalM,
    minWidth: "360px",
  },
  dialogActions: {
    display: "flex",
    justifyContent: "flex-end",
    gap: tokens.spacingHorizontalS,
    marginTop: tokens.spacingVerticalM,
  },
});

/**
 * プロファイル画面。
 *
 * Minecraftバージョン・Javaパス・最大メモリなどをまとめた起動プロファイルの一覧/作成/編集/
 * 削除、および各プロファイルからの起動を行う。プロファイルは `train_launcher_core::profile`
 * が `profiles.json` として永続化する。起動にはMicrosoftアカウントでのサインインが必須。
 * 初回起動時はライブラリ/アセットのダウンロードが発生するため数分〜十数分程度かかる場合が
 * あり、進行状況は `launch://progress` イベントを購読して起動中のプロファイルの下に
 * 進捗バーと状況テキストで表示する。
 */
export function ProfilesPage() {
  const styles = useStyles();
  const { dispatchToast } = useToastController(TOASTER_ID);

  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(true);

  const [versions, setVersions] = useState<VersionEntry[]>([]);
  const [versionsLoading, setVersionsLoading] = useState(false);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [form, setForm] = useState<ProfileFormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);

  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [progress, setProgress] = useState<LaunchProgressPayload | null>(
    null,
  );
  const [deleteTarget, setDeleteTarget] = useState<Profile | null>(null);
  const [deleting, setDeleting] = useState(false);

  const refreshProfiles = () => {
    setProfilesLoading(true);
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
      )
      .finally(() => setProfilesLoading(false));
  };

  useEffect(refreshProfiles, [dispatchToast]);

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

  // バージョン一覧はMojangへの通信を伴うため、ダイアログを開いたときに初回のみ取得する。
  const ensureVersionsLoaded = () => {
    if (versions.length > 0 || versionsLoading) {
      return;
    }
    setVersionsLoading(true);
    invoke<VersionEntry[]>("list_minecraft_versions")
      .then(setVersions)
      .catch((err) => console.error("failed to load minecraft versions", err))
      .finally(() => setVersionsLoading(false));
  };

  const openCreateDialog = () => {
    setForm(EMPTY_FORM);
    setDialogOpen(true);
    ensureVersionsLoaded();
  };

  const openEditDialog = (profile: Profile) => {
    setForm({
      id: profile.id,
      name: profile.name,
      minecraftVersion: profile.minecraft_version,
      javaPath: profile.java_path ?? "",
      maxMemoryMb:
        profile.max_memory_mb != null ? String(profile.max_memory_mb) : "",
    });
    setDialogOpen(true);
    ensureVersionsLoaded();
  };

  const handleSubmit = () => {
    const name = form.name.trim();
    const minecraftVersion = form.minecraftVersion.trim();
    if (!name || !minecraftVersion) {
      return;
    }
    const maxMemoryMb = form.maxMemoryMb.trim()
      ? Number(form.maxMemoryMb.trim())
      : null;
    if (maxMemoryMb !== null && (!Number.isFinite(maxMemoryMb) || maxMemoryMb <= 0)) {
      dispatchToast(
        <Toast>
          <ToastTitle>入力内容を確認してください</ToastTitle>
          <ToastBody>最大メモリは正の数値で入力してください</ToastBody>
        </Toast>,
        { intent: "warning" },
      );
      return;
    }

    const profile: Profile = {
      id: form.id ?? crypto.randomUUID(),
      name,
      minecraft_version: minecraftVersion,
      mod_loader: null,
      server_id: null,
      java_path: form.javaPath.trim() || null,
      max_memory_mb: maxMemoryMb,
      // TRAiN側で作成/編集した時点でTRAiN管理のプロファイルとなる
      // (公式ランチャー由来のプロファイルを編集した場合も、この操作でTRAiN側に取り込まれる)。
      source: "train",
    };

    setSaving(true);
    const command = form.id ? "update_profile" : "create_profile";
    invoke(command, { profile })
      .then(() => {
        setDialogOpen(false);
        refreshProfiles();
        dispatchToast(
          <Toast>
            <ToastTitle>
              {form.id ? "プロファイルを更新しました" : "プロファイルを作成しました"}
            </ToastTitle>
            <ToastBody>{name}</ToastBody>
          </Toast>,
          { intent: "success" },
        );
      })
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>保存に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setSaving(false));
  };

  const handleDelete = (profile: Profile) => {
    setDeleteTarget(profile);
  };

  const confirmDelete = () => {
    if (!deleteTarget) {
      return;
    }
    setDeleting(true);
    invoke("delete_profile", { id: deleteTarget.id })
      .then(() => {
        setDeleteTarget(null);
        refreshProfiles();
      })
      .catch((err) =>
        dispatchToast(
          <Toast>
            <ToastTitle>削除に失敗しました</ToastTitle>
            <ToastBody>{String(err)}</ToastBody>
          </Toast>,
          { intent: "error" },
        ),
      )
      .finally(() => setDeleting(false));
  };

  const handleLaunch = (profile: Profile) => {
    setLaunchingId(profile.id);
    setProgress(null);
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
      });
  };

  return (
    <div>
      <Title2 as="h2" block>
        プロファイル
      </Title2>
      <Body1 as="p" block>
        Minecraftのバージョン・起動設定ごとのプロファイルを作成・管理します。
      </Body1>
      <div className={styles.toolbar}>
        <Body1>{profiles.length}件のプロファイル</Body1>
        <Button
          appearance="primary"
          icon={<AddRegular />}
          onClick={openCreateDialog}
        >
          新規作成
        </Button>
      </div>

      {profilesLoading ? (
        <Spinner size="small" label="読み込み中..." />
      ) : profiles.length === 0 ? (
        <Body1 as="p" block>
          プロファイルがありません。「新規作成」から追加してください。
        </Body1>
      ) : (
        <div className={styles.list}>
          {profiles.map((profile) => (
            <Card key={profile.id}>
              <CardHeader
                header={<Text weight="semibold">{profile.name}</Text>}
                description={
                  <Caption1>
                    {profile.minecraft_version}
                    {profile.java_path ? ` ・ Java: ${profile.java_path}` : ""}
                    {profile.max_memory_mb
                      ? ` ・ 最大メモリ: ${profile.max_memory_mb}MB`
                      : ""}
                    {profile.source === "official"
                      ? " ・ 公式ランチャーから取り込み"
                      : ""}
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
                    <Button
                      appearance="subtle"
                      size="small"
                      icon={<EditRegular />}
                      disabled={launchingId !== null}
                      onClick={() => openEditDialog(profile)}
                    >
                      編集
                    </Button>
                    <Button
                      appearance="subtle"
                      size="small"
                      icon={<DeleteRegular />}
                      disabled={launchingId !== null}
                      onClick={() => handleDelete(profile)}
                    >
                      削除
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

      <Dialog
        open={dialogOpen}
        onOpenChange={(_event, data) => setDialogOpen(data.open)}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>
              {form.id ? "プロファイルを編集" : "新規プロファイル作成"}
            </DialogTitle>
            <DialogContent className={styles.form}>
              <Field label="プロファイル名" required>
                <Input
                  value={form.name}
                  onChange={(_event, data) =>
                    setForm((f) => ({ ...f, name: data.value }))
                  }
                  placeholder="例: TRAiNサーバー用"
                />
              </Field>
              <Field label="Minecraftバージョン" required>
                <Combobox
                  freeform
                  value={form.minecraftVersion}
                  placeholder="例: 1.20.4"
                  onOptionSelect={(_event, data) =>
                    setForm((f) => ({
                      ...f,
                      minecraftVersion: data.optionValue ?? f.minecraftVersion,
                    }))
                  }
                  onChange={(event) =>
                    setForm((f) => ({
                      ...f,
                      minecraftVersion: (event.target as HTMLInputElement)
                        .value,
                    }))
                  }
                >
                  {versionsLoading && (
                    <Option key="__loading" value="" disabled>
                      読み込み中...
                    </Option>
                  )}
                  {versions.map((version) => (
                    <Option
                      key={version.id}
                      value={version.id}
                      text={version.id}
                    >
                      {version.id} ({version.type})
                    </Option>
                  ))}
                </Combobox>
              </Field>
              <Field
                label="Javaパス(任意)"
                hint="未指定の場合はPATH上のjavaを使用します"
              >
                <Input
                  value={form.javaPath}
                  onChange={(_event, data) =>
                    setForm((f) => ({ ...f, javaPath: data.value }))
                  }
                  placeholder="例: C:\Program Files\Java\jdk-17\bin\java.exe"
                />
              </Field>
              <Field
                label="最大メモリ(MB、任意)"
                hint="未指定の場合はJVM既定値を使用します"
              >
                <Input
                  type="number"
                  value={form.maxMemoryMb}
                  onChange={(_event, data) =>
                    setForm((f) => ({ ...f, maxMemoryMb: data.value }))
                  }
                  placeholder="例: 4096"
                />
              </Field>
              <div className={styles.dialogActions}>
                <Button
                  appearance="secondary"
                  onClick={() => setDialogOpen(false)}
                  disabled={saving}
                >
                  キャンセル
                </Button>
                <Button
                  appearance="primary"
                  onClick={handleSubmit}
                  disabled={
                    saving || !form.name.trim() || !form.minecraftVersion.trim()
                  }
                  icon={saving ? <Spinner size="tiny" /> : undefined}
                >
                  {form.id ? "保存" : "作成"}
                </Button>
              </div>
            </DialogContent>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(_event, data) => {
          if (!data.open) {
            setDeleteTarget(null);
          }
        }}
      >
        <DialogSurface>
          <DialogBody>
            <DialogTitle>プロファイルの削除</DialogTitle>
            <DialogContent>
              <Body1>
                プロファイル「{deleteTarget?.name}」を削除しますか?
                この操作は元に戻せません。
              </Body1>
            </DialogContent>
            <DialogActions>
              <Button
                appearance="secondary"
                onClick={() => setDeleteTarget(null)}
                disabled={deleting}
              >
                キャンセル
              </Button>
              <Button
                appearance="primary"
                onClick={confirmDelete}
                disabled={deleting}
                icon={deleting ? <Spinner size="tiny" /> : undefined}
              >
                削除
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Toaster toasterId={TOASTER_ID} />
    </div>
  );
}

