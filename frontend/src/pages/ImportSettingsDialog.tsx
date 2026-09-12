import { useEffect, useRef, useState } from "react";
import {
  Body1,
  Button,
  Caption1,
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
  Toast,
  ToastBody,
  ToastTitle,
  makeStyles,
  tokens,
  useToastController,
} from "@fluentui/react-components";
import { invoke } from "@tauri-apps/api/core";
import {
  GameActivityNotice,
  runGameDataOperation,
  useGameActivity,
} from "../GameActivity";
import type {
  ImportSettingsResult,
  SettingsSource,
  SettingsSourceList,
} from "../gameDataTypes";

export interface ImportSettingsDialogProps {
  target: { id: string; name: string; minecraft_version: string };
  onClose: () => void;
  onImported: () => void;
}

const useStyles = makeStyles({
  content: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalM,
    minWidth: 0,
    overflowWrap: "anywhere",
  },
  dropdown: {
    minWidth: 0,
    width: "100%",
  },
  actions: {
    flexWrap: "wrap",
  },
  warnings: {
    marginTop: tokens.spacingVerticalXS,
    marginBottom: 0,
    paddingLeft: tokens.spacingHorizontalL,
  },
});

function sourceLabel(source: SettingsSource) {
  return `${source.name} (${source.mod_loader ?? "バニラ"})`;
}

export function ImportSettingsDialog(props: ImportSettingsDialogProps) {
  return (
    <ImportSettingsDialogContent
      key={JSON.stringify([props.target.id, props.target.minecraft_version])}
      {...props}
    />
  );
}

function ImportSettingsDialogContent({
  target,
  onClose,
  onImported,
}: ImportSettingsDialogProps) {
  const styles = useStyles();
  const { dispatchToast } = useToastController("profiles-toaster");
  const activityState = useGameActivity();
  const { activity, error: activityError } = activityState;
  const [sources, setSources] = useState<SettingsSourceList | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sourceId, setSourceId] = useState("");
  const [minecraftClosed, setMinecraftClosed] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const mounted = useRef(false);
  const pending = useRef(false);
  const selectedSource = sources?.sources.find((source) => source.id === sourceId);
  const blocked =
    !activity ||
    activityError !== null ||
    activity.running_games > 0 ||
    activity.file_operation;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setLoadError(null);
    setSources(null);
    setSourceId("");
    setMinecraftClosed(false);

    invoke<SettingsSourceList>("list_settings_sources", {
      targetProfileId: target.id,
    })
      .then((result) => {
        if (!disposed) {
          setSources(result);
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setLoadError(String(error));
        }
      })
      .finally(() => {
        if (!disposed) {
          setLoading(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [target.id, reload]);

  async function importSettings() {
    if (pending.current || blocked || !selectedSource || !minecraftClosed || loading) {
      return;
    }
    pending.current = true;
    setImporting(true);
    setImportError(null);
    let result: ImportSettingsResult;
    try {
      result = await runGameDataOperation(() =>
        invoke<ImportSettingsResult>("import_profile_settings", {
          sourceProfileId: selectedSource.id,
          targetProfileId: target.id,
        }),
      );
    } catch (error) {
      if (mounted.current) {
        setImportError(String(error));
      }
      return;
    } finally {
      pending.current = false;
      if (mounted.current) {
        setImporting(false);
      }
    }
    if (!mounted.current) {
      return;
    }
    dispatchToast(
      <Toast>
        <ToastTitle>標準設定を取り込みました</ToastTitle>
        <ToastBody>
          {result.imported_settings.toLocaleString("ja-JP")}件の設定を取り込みました。
          {result.safety_backup
            ? "取り込み前の設定を自動バックアップしました。バックアップ画面で確認できます。"
            : "既存の標準設定がないため、自動バックアップは作成されませんでした。"}
        </ToastBody>
      </Toast>,
      { intent: "success" },
    );
    onImported();
    onClose();
  }

  return (
    <Dialog
      open
      modalType={importing ? "alert" : "modal"}
      onOpenChange={(_event, data) => {
        if (!data.open && !pending.current) {
          onClose();
        }
      }}
    >
      <DialogSurface>
        <DialogBody>
          <DialogTitle>Minecraftの標準設定を取り込む</DialogTitle>
          <DialogContent>
            <div className={styles.content}>
              <Body1>
                取り込み先: <Text weight="semibold">{target.name}</Text>
              </Body1>
              <Body1>
                同じMinecraftバージョンのプロファイルから、options.txtのキー割り当て・
                音量・画面設定などの標準設定を取り込みます。元のプロファイルは変更しません。
              </Body1>
              <Body1>
                取り込み先のパック選択（resourcePacks / incompatibleResourcePacks）と
                前回の接続先（lastServer）は保持します。Mod、ワールド、リソースパックの
                実ファイルやconfigは変更しません。
              </Body1>
              <Body1>
                取り込み先に既存の標準設定がある場合は、変更前に自動バックアップを
                作成します。変更しないパック選択などの項目だけの場合は作成しません。
              </Body1>
              <GameActivityNotice {...activityState} />
              {loading && <Spinner size="small" label="互換性のある取り込み元を探しています..." />}
              {loadError && (
                <MessageBar intent="error">
                  <MessageBarBody>
                    取り込み元を取得できませんでした。{loadError}
                  </MessageBarBody>
                </MessageBar>
              )}
              {!loading && (
                <Button
                  onClick={() => {
                    setSourceId("");
                    setMinecraftClosed(false);
                    setImportError(null);
                    setLoading(true);
                    setReload((value) => value + 1);
                  }}
                  disabled={importing}
                >
                  取り込み元を再読み込み
                </Button>
              )}
              {sources && (
                <>
                  <Caption1>
                    対象のMinecraft: {sources.minecraft_version}
                    （継承元を含めて互換性を確認済み）
                  </Caption1>
                  {sources.warnings.length > 0 && (
                    <MessageBar intent="warning">
                      <MessageBarBody>
                        取り込み元の確認に関するお知らせ
                        <ul className={styles.warnings}>
                          {sources.warnings.map((warning, index) => (
                            <li key={`${index}:${warning}`}>{warning}</li>
                          ))}
                        </ul>
                      </MessageBarBody>
                    </MessageBar>
                  )}
                  {sources.sources.length === 0 ? (
                    <MessageBar intent="info">
                      <MessageBarBody>
                        取り込める標準設定がある、同じMinecraftバージョンの
                        プロファイルが見つかりませんでした。
                      </MessageBarBody>
                    </MessageBar>
                  ) : (
                    <Field label="取り込み元のプロファイル" required>
                      <Dropdown
                        className={styles.dropdown}
                        placeholder="取り込み元を選択"
                        value={selectedSource ? sourceLabel(selectedSource) : ""}
                        selectedOptions={selectedSource ? [selectedSource.id] : []}
                        disabled={importing}
                        onOptionSelect={(_event, data) => {
                          setSourceId(data.optionValue ?? "");
                          setMinecraftClosed(false);
                          setImportError(null);
                        }}
                      >
                        {sources.sources.map((source) => (
                          <Option key={source.id} value={source.id} text={sourceLabel(source)}>
                            {sourceLabel(source)}
                          </Option>
                        ))}
                      </Dropdown>
                    </Field>
                  )}
                  {selectedSource && (
                    <Caption1>
                      Minecraft {selectedSource.minecraft_version} / {selectedSource.game_dir}
                    </Caption1>
                  )}
                </>
              )}
              <Checkbox
                checked={minecraftClosed}
                disabled={importing || !selectedSource}
                onChange={(_event, data) => setMinecraftClosed(data.checked === true)}
                label="TRAiN以外から起動したものも含め、すべてのMinecraftを終了しました"
              />
              <Caption1>
                TRAiN以外から起動したMinecraftは検出できません。終了していないと、
                設定が正しく保存されない場合があります。
              </Caption1>
              {importError && (
                <MessageBar intent="error">
                  <MessageBarBody>設定を取り込めませんでした。{importError}</MessageBarBody>
                </MessageBar>
              )}
            </div>
          </DialogContent>
          <DialogActions className={styles.actions}>
            <Button disabled={importing} onClick={onClose}>
              キャンセル
            </Button>
            <Button
              appearance="primary"
              disabled={blocked || loading || importing || !selectedSource || !minecraftClosed}
              onClick={() => void importSettings()}
            >
              {importing ? "取り込み中..." : "標準設定を取り込む"}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
