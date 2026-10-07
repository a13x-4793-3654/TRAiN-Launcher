import { useCallback, useEffect, useState } from "react";
import {
  Body1, Button, Caption1, Dialog, DialogActions, DialogBody, DialogContent,
  DialogSurface, DialogTitle, MessageBar, MessageBarBody, Spinner, Title3,
} from "@fluentui/react-components";
import { invoke } from "@tauri-apps/api/core";

interface FallbackStatus {
  available: boolean;
  enabled_until: number;
  protocol: string;
}

export function AuthFallbackPanel(props: {
  serverId: string;
  busy: boolean;
  testMode: boolean;
  onLaunch: () => void;
}) {
  const [status, setStatus] = useState<FallbackStatus | null>(null);
  const [now, setNow] = useState(Date.now());
  const [loading, setLoading] = useState(false);
  const [enrolling, setEnrolling] = useState(false);
  const [message, setMessage] = useState<{ error: boolean; text: string } | null>(null);
  const [confirmation, setConfirmation] = useState<"enroll" | "launch" | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setStatus(null);
    setMessage(null);
    try {
      const result = await invoke<FallbackStatus>("get_auth_fallback_status", { serverId: props.serverId });
      if (result.protocol !== "train-auth-fallback-v1") throw new Error("未対応の障害時接続プロトコルです");
      setStatus(result);
    } catch (error) {
      setMessage({ error: true, text: String(error) });
    } finally {
      setLoading(false);
    }
  }, [props.serverId]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const enroll = async () => {
    setEnrolling(true);
    setMessage(null);
    try {
      const expiry = await invoke<number>("enroll_auth_fallback", { serverId: props.serverId, consent: true });
      setMessage({ error: false, text: `事前登録しました。有効期限: ${new Date(expiry).toLocaleString("ja-JP")}` });
      setConfirmation(null);
    } catch (error) {
      setMessage({ error: true, text: String(error) });
      setConfirmation(null);
    } finally {
      setEnrolling(false);
    }
  };

  const available = status?.available === true;
  const enabled = available && status.enabled_until > now;
  const busy = props.busy || loading || enrolling;

  return (
    <section>
      <Title3 as="h3" block>Mojang障害時の接続（任意）</Title3>
      <Body1 as="p" block>
        Minecraft認証が正常な間に、この端末・Discord・Minecraftアカウントを事前登録します。
        登録は7日間有効です。障害時もDiscordとTRAiN APIへの接続が必要です。
        管理者が一時的に許可したサーバーに限り使用でき、通常起動は変更しません。
      </Body1>
      <Caption1 as="p" block>
        {loading ? "状態を確認中..." : !available ? "対応Mod未登録、または状態を取得できません" :
          enabled ? `管理者が有効化中（${new Date(status.enabled_until).toLocaleString("ja-JP")}まで）` :
            "管理者による障害時接続の許可は現在無効です（事前登録は可能）"}
      </Caption1>
      {message && <MessageBar intent={message.error ? "error" : "success"}>
        <MessageBarBody>{message.text}</MessageBarBody>
      </MessageBar>}
      <Button disabled={busy || !available} onClick={() => setConfirmation("enroll")}>
        {enrolling && <Spinner size="tiny" />}事前登録・再登録
      </Button>{" "}
      <Button disabled={busy || !enabled} onClick={() => setConfirmation("launch")}>障害時接続で起動</Button>{" "}
      <Button disabled={busy} onClick={() => void refresh()}>状態を更新</Button>
      <Dialog open={confirmation !== null} onOpenChange={(_, data) => {
        if (!data.open && !enrolling) setConfirmation(null);
      }}>
        <DialogSurface>
          <DialogBody>
            <DialogTitle>{confirmation === "enroll" ? "障害時接続を事前登録しますか" : "障害時接続で起動しますか"}</DialogTitle>
            <DialogContent>
              {confirmation === "enroll" ? (
                <Body1>
                  TRAiNへMinecraftアクセストークンを送信し、紐づけ済みアカウントの本人・所有権をオンライン確認します。
                  秘密鍵はこの端末のOS資格情報ストアだけに保存し、公開鍵をサーバー別に登録します。
                  再登録すると、このDiscordアカウントの同じサーバー向けの古い登録とチケットは無効になります。
                  別端末からの登録も置き換わります。Minecraft認証サービスの復旧後に行ってください。
                </Body1>
              ) : (
                <Body1>
                  事前登録した本人情報で、このサーバーだけに一度接続するチケットを発行します。
                  Minecraft 1.21.1 / Fabricと配布済みtrain-auth-fallback 1.0.0が必要です。
                  通常のMinecraft認証更新は行いません。他のサーバーで認証を省略することはありません。
                  準備完了後に対象サーバーへ直接接続します。チケットは最大60秒・一回限りのため、
                  起動が遅い場合は失敗します。その場合はこのボタンから起動し直してください。
                  {props.testMode && " このサーバーでは管理者向け試験モードの配布構成が使われます。"}
                </Body1>
              )}
            </DialogContent>
            <DialogActions>
              <Button disabled={enrolling} onClick={() => setConfirmation(null)}>キャンセル</Button>
              <Button appearance="primary" disabled={enrolling || props.busy} onClick={() => {
                if (confirmation === "enroll") void enroll();
                else { setConfirmation(null); props.onLaunch(); }
              }}>同意して{confirmation === "enroll" ? "登録" : "起動"}</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </section>
  );
}
