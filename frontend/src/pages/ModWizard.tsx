import { useState } from "react";
import {
  Body1,
  Caption1,
  Field,
  Input,
  Textarea,
  Button,
  Spinner,
  Text,
  MessageBar,
  MessageBarBody,
  MessageBarTitle,
  Dialog,
  DialogSurface,
  DialogTitle,
  DialogBody,
  DialogContent,
  DialogActions,
  makeStyles,
  tokens,
} from "@fluentui/react-components";
import { ArrowDownloadRegular } from "@fluentui/react-icons";
import { invoke } from "@tauri-apps/api/core";

interface ResolvedModPayload {
  provider: string;
  project_name: string;
  filename: string;
  is_dependency: boolean;
}

type WizardStep = "input" | "preview" | "done";

const useStyles = makeStyles({
  field: {
    marginTop: tokens.spacingVerticalM,
  },
  urlsTextarea: {
    minHeight: "140px",
  },
  previewList: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXXS,
    marginTop: tokens.spacingVerticalS,
  },
});

/** URLを1行ずつ分割し、空行・前後の空白を除いた一覧を返す。 */
function parseUrls(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/**
 * 複数のMod URLをまとめて解決・導入する「Mod導入ウィザード」ダイアログ。
 *
 * 1. URL入力(複数行、1行1URL) + 対象Minecraftバージョン(任意)
 * 2. `resolve_mod_urls` で一括解決したプレビュー(依存Modも含め、重複は1件にまとめる)を確認
 * 3. `install_mods` で一括インストールし、結果件数を表示
 *
 * 対象プロファイルは `Mods.tsx` の「対象プロファイル」選択に従う(`profileId` として受け取る)。
 * 単体URLでの導入は引き続き既存の `InstallPanel`(Mod欄)から行える。
 */
export function ModInstallWizardDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profileId: string | null;
  onInstalled: () => void;
}) {
  const styles = useStyles();

  const [step, setStep] = useState<WizardStep>("input");
  const [urlsText, setUrlsText] = useState("");
  const [minecraftVersion, setMinecraftVersion] = useState("");
  const [resolving, setResolving] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [preview, setPreview] = useState<ResolvedModPayload[]>([]);
  const [installedCount, setInstalledCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const resetState = () => {
    setStep("input");
    setUrlsText("");
    setMinecraftVersion("");
    setPreview([]);
    setInstalledCount(0);
    setError(null);
  };

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      resetState();
    }
    props.onOpenChange(open);
  };

  const urls = parseUrls(urlsText);

  const handleResolve = () => {
    if (urls.length === 0) {
      return;
    }
    setResolving(true);
    setError(null);
    invoke<ResolvedModPayload[]>("resolve_mod_urls", {
      urls,
      minecraftVersion: minecraftVersion.trim() || null,
    })
      .then((result) => {
        setPreview(result);
        setStep("preview");
      })
      .catch((err) => setError(String(err)))
      .finally(() => setResolving(false));
  };

  const handleInstall = () => {
    setInstalling(true);
    setError(null);
    invoke<string[]>("install_mods", {
      urls,
      minecraftVersion: minecraftVersion.trim() || null,
      profileId: props.profileId,
    })
      .then((installed) => {
        setInstalledCount(installed.length);
        setStep("done");
        props.onInstalled();
      })
      .catch((err) => setError(String(err)))
      .finally(() => setInstalling(false));
  };

  return (
    <Dialog open={props.open} onOpenChange={(_event, data) => handleOpenChange(data.open)}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>Mod導入ウィザード</DialogTitle>
          <DialogContent>
            {step === "input" && (
              <>
                <Body1 as="p" block>
                  複数のModのURL(Modrinth・CurseForge)を1行に1件ずつ貼り付けてください。
                  依存Modも自動的に解決し、重複するModはまとめて1件として扱います。
                </Body1>
                <Field label="Mod URL(1行に1件)" className={styles.field}>
                  <Textarea
                    className={styles.urlsTextarea}
                    value={urlsText}
                    onChange={(_event, data) => setUrlsText(data.value)}
                    placeholder={
                      "例:\nhttps://modrinth.com/mod/sodium\nhttps://modrinth.com/mod/lithium"
                    }
                  />
                </Field>
                <Field label="対象Minecraftバージョン(任意)" className={styles.field}>
                  <Input
                    value={minecraftVersion}
                    onChange={(_event, data) => setMinecraftVersion(data.value)}
                    placeholder="例: 1.20.4"
                  />
                </Field>
                {error && (
                  <MessageBar intent="error" className={styles.field}>
                    <MessageBarBody>{error}</MessageBarBody>
                  </MessageBar>
                )}
              </>
            )}

            {step === "preview" && (
              <>
                {preview.length === 0 ? (
                  <MessageBar intent="warning">
                    <MessageBarBody>
                      <MessageBarTitle>解決できませんでした</MessageBarTitle>
                    </MessageBarBody>
                  </MessageBar>
                ) : (
                  <>
                    <Body1 as="p" block>
                      {preview.length}件のファイルが導入対象です(依存Modを含む)。
                    </Body1>
                    <div className={styles.previewList}>
                      {preview.map((item) => (
                        <div key={`${item.provider}-${item.filename}`}>
                          <Text weight="semibold">{item.project_name}</Text>{" "}
                          <Caption1>
                            ({item.filename})
                            {item.is_dependency ? " ・ 依存Mod" : ""} ・{" "}
                            {item.provider}
                          </Caption1>
                        </div>
                      ))}
                    </div>
                  </>
                )}
                {error && (
                  <MessageBar intent="error" className={styles.field}>
                    <MessageBarBody>{error}</MessageBarBody>
                  </MessageBar>
                )}
              </>
            )}

            {step === "done" && (
              <MessageBar intent="success">
                <MessageBarBody>
                  <MessageBarTitle>導入が完了しました</MessageBarTitle>
                  {installedCount}件のファイルを導入しました。
                </MessageBarBody>
              </MessageBar>
            )}
          </DialogContent>
          <DialogActions>
            {step === "input" && (
              <>
                <Button appearance="secondary" onClick={() => handleOpenChange(false)}>
                  キャンセル
                </Button>
                <Button
                  appearance="primary"
                  disabled={urls.length === 0 || resolving}
                  icon={resolving ? <Spinner size="tiny" /> : undefined}
                  onClick={handleResolve}
                >
                  解決して確認
                </Button>
              </>
            )}
            {step === "preview" && (
              <>
                <Button appearance="secondary" onClick={() => setStep("input")}>
                  戻る
                </Button>
                <Button
                  appearance="primary"
                  icon={
                    installing ? <Spinner size="tiny" /> : <ArrowDownloadRegular />
                  }
                  disabled={preview.length === 0 || installing}
                  onClick={handleInstall}
                >
                  導入
                </Button>
              </>
            )}
            {step === "done" && (
              <Button appearance="primary" onClick={() => handleOpenChange(false)}>
                閉じる
              </Button>
            )}
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
