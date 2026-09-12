import { useEffect, useRef, useState } from "react";
import type { UIEvent } from "react";
import {
  Body1,
  Text,
  Button,
  Spinner,
  MessageBar,
  MessageBarBody,
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
  makeStyles,
  tokens,
} from "@fluentui/react-components";

const useStyles = makeStyles({
  scrollArea: {
    maxHeight: "50vh",
    overflowY: "auto",
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    borderRadius: tokens.borderRadiusMedium,
    padding: tokens.spacingHorizontalM,
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalM,
  },
  section: {
    display: "flex",
    flexDirection: "column",
    gap: tokens.spacingVerticalXS,
  },
  hint: {
    marginTop: tokens.spacingVerticalS,
  },
});

/**
 * 初回参加時にDiscord↔Minecraftアカウントの紐づけへ同意を得るための注意事項モーダル。
 *
 * 本文はTRAiNリポジトリの実装(`src/services/minecraft.ts`・`src/commands/link.ts`・
 * `minecraft-plugin`/`minecraft-mod`の`RestrictionManager`/`TrainLinkPlugin`/
 * `TrainLinkMod`、`docs/MINECRAFT-ARCHITECTURE.md`)を確認した上で、実際の挙動のみを
 * 記載している。すべて読んだことを保証するため、末尾までスクロールするまで
 * 「同意して連携する」ボタンは有効化しない。
 */
export function AccountLinkNoticeModal(props: {
  open: boolean;
  serverName: string;
  linking: boolean;
  error: string | null;
  onAgree: () => void;
  onCancel: () => void;
}) {
  const styles = useStyles();
  const [scrolledToEnd, setScrolledToEnd] = useState(false);
  const scrollAreaRef = useRef<HTMLDivElement>(null);

  // モーダルを開くたびにスクロール済み状態をリセットする。また、注意事項の文章が
  // 画面に収まりスクロールが発生しない場合(非常に縦長のウィンドウ等)に備え、開いた
  // 直後に「そもそもスクロール可能か」を確認し、不要なら最初からボタンを有効にする。
  useEffect(() => {
    if (!props.open) {
      setScrolledToEnd(false);
      return;
    }
    const element = scrollAreaRef.current;
    if (element && element.scrollHeight <= element.clientHeight + 4) {
      setScrolledToEnd(true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    const target = event.currentTarget;
    // 数px程度の誤差を許容する(ブラウザ/スクロール実装によって端数がずれることがあるため)。
    const atBottom =
      target.scrollTop + target.clientHeight >= target.scrollHeight - 4;
    if (atBottom) {
      setScrolledToEnd(true);
    }
  };

  return (
    <Dialog
      open={props.open}
      modalType="alert"
      onOpenChange={(_event, data) => {
        if (!data.open && !props.linking) {
          props.onCancel();
        }
      }}
    >
      <DialogSurface>
        <DialogBody>
          <DialogTitle>Discordアカウントとの紐づけについて</DialogTitle>
          <DialogContent>
            <Body1 as="p" block>
              {props.serverName}
              への初回参加です。続行する前に、以下の内容を最後までご確認ください。
            </Body1>

            <div
              className={styles.scrollArea}
              ref={scrollAreaRef}
              onScroll={handleScroll}
              data-testid="account-link-notice-scroll-area"
            >
              <div className={styles.section}>
                <Text weight="semibold">1. このサーバーについて</Text>
                <Body1 as="p" block>
                  このサーバーは、Discordアカウントと参加するMinecraftアカウントを
                  紐づけて運用しています。紐づけていないアカウントで参加した場合、
                  紐づけが完了するまでサーバー内での行動(移動など)が制限されます。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">2. これから行われる処理</Text>
                <Body1 as="p" block>
                  「同意して連携する」を押すと、ランチャーが既に確認済みの次の情報を
                  使って、TRAiNへ紐づけを直接申請します。
                </Body1>
                <Body1 as="p" block>
                  ・Discordアカウント(このランチャーでサインイン済みのアカウント)
                  <br />
                  ・Minecraftアカウント(Microsoft/Xboxで認証済みのプレイヤー名とUUID)
                </Body1>
                <Body1 as="p" block>
                  本来はMinecraft参加時にゲーム内へ表示される認証コードを、Discordで
                  「/link」コマンドへ入力する手順です。ランチャーは両方のアカウントを
                  既にサインインで確認済みのため、この画面での同意をもってその手順を
                  省略し、直接紐づけを行います。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">3. 紐づけの範囲</Text>
                <Body1 as="p" block>
                  紐づけは、このMinecraftサーバー単体ではなく、このサーバーが所属する
                  Discordサーバー(ギルド)全体で共通です。同じDiscordサーバーが複数の
                  Minecraftサーバーを運用している場合、1度の紐づけがそのすべてに
                  適用されます。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">4. 1対1の制約</Text>
                <Body1 as="p" block>
                  1つのDiscordアカウントにつき、このDiscordサーバー内で紐づけられる
                  Minecraftアカウントは1つだけです(逆も同様)。既に別の組み合わせで
                  紐づけ済みの場合、この処理は失敗します。心当たりがない状態で
                  失敗した場合は、サーバー管理者に確認してください。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">5. 紐づけ後に起きること</Text>
                <Body1 as="p" block>
                  紐づけが完了すると、Minecraft参加時の行動制限が解除され、通常の
                  参加位置から始められるようになります。サーバーの設定によっては、
                  Discord側で「連携済み」を示すロールが自動的に付与されます。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">6. 紐づけが解除される場合</Text>
                <Body1 as="p" block>
                  紐づけたDiscordアカウントがこのDiscordサーバーから退出すると、
                  Minecraft側の紐づけは自動的に解除され、次にMinecraftへ接続した際に
                  切断されます。再度参加するには、改めて紐づけをやり直す必要があります。
                </Body1>
              </div>

              <div className={styles.section}>
                <Text weight="semibold">7. 同意しない場合</Text>
                <Body1 as="p" block>
                  ここで同意しない場合、ランチャーは紐づけを行いません。
                  「キャンセル」を押すと今回の起動を中止します。後から改めてこの画面で
                  紐づけることも、Minecraft参加後にゲーム内へ表示される認証コードと
                  Discordの「/link」コマンドで手動で紐づけることもできます。
                </Body1>
              </div>
            </div>

            {!scrolledToEnd && (
              <Body1 as="p" block className={styles.hint}>
                最後まで読むと「同意して連携する」ボタンが有効になります。
              </Body1>
            )}

            {props.error && (
              <MessageBar intent="error" className={styles.hint}>
                <MessageBarBody>{props.error}</MessageBarBody>
              </MessageBar>
            )}
          </DialogContent>
          <DialogActions>
            <Button
              appearance="secondary"
              onClick={props.onCancel}
              disabled={props.linking}
            >
              キャンセル
            </Button>
            <Button
              appearance="primary"
              icon={props.linking ? <Spinner size="tiny" /> : undefined}
              disabled={!scrolledToEnd || props.linking}
              onClick={props.onAgree}
            >
              同意して連携する
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
