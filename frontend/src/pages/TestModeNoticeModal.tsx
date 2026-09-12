import {
  Body1,
  Text,
  Button,
  MessageBar,
  MessageBarBody,
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
} from "@fluentui/react-components";

/**
 * 試験モードで動作しているサーバーを選択した際に表示する通知モーダル。
 *
 * `get_server_config` の `test_mode` が `true`(=試験モードが有効かつ、この
 * Discordアカウントが当該サーバーのギルドでAdministrator権限を持っており、実際に
 * 試験用構成が適用される)の場合にのみ表示する。`AccountLinkNoticeModal` と異なり
 * 同意を要する法的な注意事項ではなく、単なる状態通知のため、スクロール読了は
 * 要求しない(即座に「起動する」を押せる)。
 */
export function TestModeNoticeModal(props: {
  open: boolean;
  serverName: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      open={props.open}
      modalType="alert"
      onOpenChange={(_event, data) => {
        if (!data.open) {
          props.onCancel();
        }
      }}
    >
      <DialogSurface>
        <DialogBody>
          <DialogTitle>試験モードで起動します</DialogTitle>
          <DialogContent>
            <Body1 as="p" block>
              {props.serverName}
              は現在「試験モード」が有効になっています。あなたがこのDiscordサーバーで
              Administrator権限を持っているため、本番の配布設定ではなく、
              <Text weight="semibold">
                Mod調査・検証用に構成されたMod/リソースパック
              </Text>
              を導入して起動します。
            </Body1>
            <Body1 as="p" block>
              一般のプレイヤー(Administrator権限を持たないユーザー)には試験モードは
              一切影響せず、通常どおり本番の配布設定で起動されます。
            </Body1>
            <MessageBar intent="warning">
              <MessageBarBody>
                試験用のMod構成は動作未検証のものを含む場合があります。ワールドやセーブ
                データに影響する可能性があるため、検証目的以外での使用は避けてください。
              </MessageBarBody>
            </MessageBar>
          </DialogContent>
          <DialogActions>
            <Button appearance="secondary" onClick={props.onCancel}>
              キャンセル
            </Button>
            <Button appearance="primary" onClick={props.onConfirm}>
              起動する
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
