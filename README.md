# TRAiN Launcher

TRAiN が管理する Minecraft サーバーへ簡単にアクセスするためのランチャーです。

## 概要

TRAiN Launcher は、TRAiN 管理下のサーバーに参加しているユーザーが、複雑な設定(Mod / 前提Mod / リソースパック / 接続情報など)を意識せずにワンクリックでプレイを開始できることを目的としたランチャーです。TRAiN に所属していないユーザーでも、一般的な Minecraft Launcher と同様の使い方ができます。

## 主な機能

### サインインと自動設定取得

- **Discord** と **Microsoft アカウント (MSA)** にそれぞれサインイン
- Discord でサインイン後、TRAiN が導入されているサーバー(所属サーバー)を選択すると、そのサーバー向けの設定情報(接続先・Mod構成・リソースパックなど)を自動取得して反映
- 所属サーバーがない(TRAiNに参加していない)ユーザーでも、通常の Minecraft Launcher と同様に単体で利用可能

### プロファイル / Mod 管理

- **Fabric / Quilt / Forge / NeoForge** の前提Modローダーをプロファイルの設定だけで自動導入(未導入の場合のみ実行され、既に導入済みなら再導入はしない)
  - Fabric / Quilt は公式Meta APIから完全なバージョン情報を取得して導入
  - Forge / NeoForge は公式インストーラjarをダウンロードし、ヘッドレスモードで実行して導入(GUIは表示されない)
  - ローダーバージョンを指定しない場合は安定版の最新(Forge/NeoForgeは推奨版)を自動選択
- プロファイルごとに Mod の取得元 URL (Modrinth / CurseForge など) を入力するだけで、自動的に取得・設置
- 前提Mod(依存Mod)を自動的に解決し、依存関係も含めて自動インストールすることで、ユーザーが個別に依存関係を調べて導入する手間をなくす
- リソースパックについても Mod と同様の仕組みで URL 指定による自動導入に対応
- プロファイルごとに専用のゲームディレクトリ(`gameDir`)を設定可能。設定した場合、Mod / リソースパック / セーブデータ / `options.txt` はそのプロファイル専用の場所に保存され、他のプロファイルと混ざらない(バージョン本体・ライブラリ・アセットは引き続き共有し、二重ダウンロードを避ける)。未設定の場合は従来どおり全プロファイル共通の `.minecraft` フォルダを使用する。この `gameDir` は公式 Minecraft Launcher の `launcher_profiles.json` の `gameDir` と双方向に同期される
- TRAiN サーバーへの参加時に自動作成されるプロファイルは、サーバーごとに専用のゲームディレクトリを自動的に割り当てるため、複数のTRAiNサーバー間でMod構成が競合しない

### 認証

- Microsoft アカウントでのサインインには Microsoft Entra ID (Azure AD) アプリ登録 + Minecraft API 利用申請 (`XboxLive.signin` スコープ) が必要
- Discord サインインには Discord OAuth2 を利用
- 認証フロー自体は `crates/auth` に実装済み(MSAはデバイスコードフロー、DiscordはPKCE付き認可コードフロー+ローカルループバックサーバでのリダイレクト受信)。取得したトークンはOSの資格情報ストア(Windows Credential Manager / macOS Keychain / Linux Secret Service)に保存される

### 自動アップデート

- `tauri-plugin-updater` を使用し、ランチャー起動直後に一度サイレントでアップデートの有無を確認する(見つからない場合・確認に失敗した場合もユーザーには通知しない)
- アップデートが見つかった場合、画面上部(全タブ共通)にバナーを表示し、「今すぐアップデート」でダウンロード・インストール・再起動まで行う。「後で」を押すと、次回起動時まで再表示しない
- 設定画面にも「ランチャー本体」セクションがあり、現在のバージョン表示・手動でのアップデート確認・適用が行える
- 更新情報の配布元は GitHub Releases (`https://github.com/a13x-4793-3654/TRAiN-Launcher/releases/latest/download/latest.json`)。配布物の署名検証用の公開鍵は `apps/desktop/tauri.conf.json` の `plugins.updater.pubkey` に埋め込み済み。リリースの作成・署名は `.github/workflows/release.yml` が自動で行う(詳細は本README末尾の「リリース手順」を参照)

## 開発環境セットアップ

### 前提条件

- **Rust** (stable) — [rustup](https://rustup.rs/) 経由でインストールしてください
  - Windows の場合は Visual Studio Build Tools の「C++ によるデスクトップ開発」ワークロード(MSVC ツールセット・Windows SDK)が必要です
- **Node.js** 20.x 以上 と npm
- **Tauri CLI の前提条件**(WebView2 ランタイムなど): [Tauri Prerequisites](https://tauri.app/start/prerequisites/) を参照してください
  - Windows 10/11 には標準で WebView2 Runtime が含まれていることが多いですが、ない場合は別途インストールが必要です
  - Tauri CLI 自体は `npm install` で `apps/desktop` の devDependency (`@tauri-apps/cli`) として導入されるため、別途グローバルインストールは不要です

### セットアップ手順

1. 依存関係のインストール(ルートで npm workspaces を使用し、`frontend/` と `apps/desktop/` の依存も併せて解決されます)
   ```powershell
   npm install
   ```
2. 開発モードで起動(Tauri アプリがフロントエンドの dev server を自動起動し、ウィンドウが開きます)
   ```powershell
   npm run dev
   ```
   内部的には `apps/desktop` で Tauri CLI (`tauri dev`) が実行され、`frontend` の Vite dev server (`http://localhost:1420`) と連携します。
3. ビルド
   ```powershell
   npm run build         # フロントエンドのみビルド (tsc && vite build)
   npm run tauri build   # Tauri アプリ全体をビルド(実行ファイル・インストーラ一式を生成)
   ```
4. Rust ワークスペース単体のビルド・確認(フロントエンドを介さない場合)
   ```powershell
   cargo build --workspace
   ```

### サインイン機能を試すための環境変数(OAuth2アプリ登録)

MSA/Discordサインインを実際に動作させるには、それぞれのアプリ登録情報を環境変数として設定する必要があります。未設定の場合、サインインボタンを押すと日本語のエラーメッセージ(どの環境変数が不足しているか)が表示されます。

| 環境変数 | 必須 | 説明 |
| --- | --- | --- |
| `TRAIN_LAUNCHER_MS_CLIENT_ID` | Microsoftサインインに必須 | [Microsoft Entra ID](https://portal.azure.com/) で登録したアプリの クライアントID。個人用Microsoftアカウント (`consumers` テナント) 向けに、パブリッククライアントとしてデバイスコードフローを許可する必要があります |
| `TRAIN_LAUNCHER_DISCORD_CLIENT_ID` | Discordサインインに必須 | [Discord Developer Portal](https://discord.com/developers/applications) で登録したアプリのクライアントID |
| `TRAIN_LAUNCHER_DISCORD_CLIENT_SECRET` | 任意 | Discordアプリのクライアントシークレット(Developer Portal側の設定によっては不要) |
| `TRAIN_LAUNCHER_DISCORD_CALLBACK_PORT` | 任意(デフォルト `38271`) | ローカルループバックリダイレクトサーバのポート番号。Discord Developer Portal の "Redirects" に `http://127.0.0.1:{ポート番号}/callback` を同じ値で事前登録しておく必要があります(Discordはワイルドカードポートを許可しないため) |

例(PowerShellで `cargo tauri dev` の前に設定):
```powershell
$env:TRAIN_LAUNCHER_MS_CLIENT_ID = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
$env:TRAIN_LAUNCHER_DISCORD_CLIENT_ID = "123456789012345678"
npm run dev
```

> **注意**: この開発環境には実際のMicrosoft Entra ID / Discord Developer Portal のアプリ登録がないため、エンドツーエンドのサインイン動作確認は行えていません。ビルド成功・ユニットテスト(設定読み込み・URL構築ロジック)の確認までを実施済みです。実際の登録情報を用意できる環境で動作確認を行ってください。

### Mod / リソースパック導入機能を試すための環境変数

Modrinth のURLはAPIキー不要で解決できますが、CurseForgeのURLを解決・導入するにはAPIキーが必要です。

| 環境変数 | 必須 | 説明 |
| --- | --- | --- |
| `TRAIN_LAUNCHER_CURSEFORGE_API_KEY` | CurseForgeのURLを使う場合のみ必須(Modrinthのみなら不要) | [CurseForge Console](https://console.curseforge.com/#/api-keys) で発行するAPIキー |

例(PowerShellで `cargo tauri dev` の前に設定):
```powershell
$env:TRAIN_LAUNCHER_CURSEFORGE_API_KEY = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
npm run dev
```

### TRAiN 独自バックエンドAPIを試すための環境変数

TRAiN側のAPI仕様は確定済みです(TRAiNリポジトリの `docs/LAUNCHER-API.md` 参照)。配布版(リリースビルド)は環境変数を何も設定しなくても本番のTRAiN API(`https://train.alex-infosys.info`、[`DEFAULT_API_BASE_URL`](crates/train-api/src/lib.rs))へ既定で接続します。以下の環境変数は、ローカル開発時に接続先を切り替えたい場合のみ設定してください。

| 環境変数 | 必須 | 説明 |
| --- | --- | --- |
| `TRAIN_LAUNCHER_API_BASE_URL` | 任意(未設定時は本番TRAiN APIを使用) | TRAiN APIのベースURL(例: `https://train.example.com`)を上書きする場合に指定する。値として `mock`(大文字小文字は区別しない)を指定すると、常にダミーデータを返すモック実装(`MockTrainApiClient`)を強制的に使用する。認証はDiscordアクセストークンをBearerとして送るだけでよく(TRAiN側がDiscord APIへ問い合わせて本人確認する)、追加設定は不要 |

お知らせ機能(`GET {base}/api/announcements` / `GET {base}/api/servers/{server_id}/announcements`)も同じ `TRAIN_LAUNCHER_API_BASE_URL` を使うため、新しい環境変数は不要です。

例(PowerShellで、ローカルの開発用TRAiNバックエンドへ向けたい場合):
```powershell
$env:TRAIN_LAUNCHER_API_BASE_URL = "http://localhost:8787"
npm run dev
```

例(モック実装を強制したい場合):
```powershell
$env:TRAIN_LAUNCHER_API_BASE_URL = "mock"
npm run dev
```

### ワークスペース構成

```
Cargo.toml         # Cargo workspace ルート
crates/
  core/            # train-launcher-core: プロファイル管理、MCバージョンマニフェスト/ライブラリ/アセットのダウンロード、起動コマンド構築
  auth/            # train-launcher-auth: MSA/XboxLive/Minecraft 認証チェーン、Discord OAuth2
  mods/            # train-launcher-mods: Modrinth/CurseForge からの Mod 解決・依存関係自動解決・インストール
  train-api/       # train-launcher-server-api: TRAiN 独自バックエンドAPIクライアント(トレイト+モック実装+HTTPクライアント。エンドポイント形式はTRAiN側 docs/LAUNCHER-API.md で確定済み)
apps/
  desktop/         # Tauri アプリ本体(上記 crate を呼び出す Tauri commands を定義)
frontend/          # React + Vite + TypeScript + Fluent UI React v9 (`@fluentui/react-components`) 製フロントエンド
```

### 補足

- MSA/Discord 認証フロー(`crates/auth`)、Minecraft本体の起動処理(`crates/core`、Fabric/Quilt/Forge/NeoForgeの自動導入込み)、Mod 依存解決・ダウンロード(`crates/mods`)は実装済みです。TRAiN 独自APIもエンドポイント形式・認証方式が確定済みで実装済みです(`TRAIN_LAUNCHER_API_BASE_URL` 参照)。ただし、サーバー管理者が `minecraft-mods manifest`(または `apply`)を一度もTRAiN Link経由で送っていないサーバーは設定未登録として扱われ、参加はできません
- Forge/NeoForge の自動導入にはインストーラjarの実行に Java (JRE/JDK) が必要です。設定画面またはプロファイルで指定したJavaパス(未指定時はPATH上の `java`)が使われます
- Windows で `cargo build` 時に MSVC ヘッダ(`vcruntime.h` 等)が見つからないエラーが出る場合は、Visual Studio Installer で「C++ によるデスクトップ開発」ワークロードが正しくインストールされているか確認してください

## リリース手順(メンテナー向け)

ランチャー自体の自動アップデート機能(`tauri-plugin-updater`)は GitHub Releases を配布元としています。新バージョンを配布するには以下の手順を行ってください。

1. **バージョン番号を更新する**: `apps/desktop/tauri.conf.json` の `version` と、`Cargo.toml` の `[workspace.package] version` を新しいバージョン(例: `0.2.0`)へ揃えて更新し、`main`(または開発ブランチ経由で)へマージする
2. **タグを作成してpushする**:
   ```powershell
   git tag v0.2.0
   git push origin v0.2.0
   ```
3. `.github/workflows/release.yml` が自動的にWindows/macOS(Intel・Apple Silicon)/Linux向けにビルド・署名を行い、**下書き(draft)状態のGitHub Release** を作成する(誤って未検証のビルドが配布されないよう、意図的に下書きにしている)
4. GitHubの「Releases」画面で成果物とリリースノートを確認し、問題なければ **「Publish release」を押して公開する**。公開して初めて `https://github.com/a13x-4793-3654/TRAiN-Launcher/releases/latest/download/latest.json` が更新され、既存ユーザーのランチャーがアップデートを検知できるようになる

### 事前準備(初回のみ・設定済み)

- アップデート成果物の署名用キーペアを生成済み(`npm run tauri -- signer generate`)。公開鍵は `apps/desktop/tauri.conf.json` の `plugins.updater.pubkey` に埋め込み済み
- 秘密鍵とパスワードは、このリポジトリの GitHub Actions Secrets (`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) に設定済み。**秘密鍵はリポジトリのどこにもコミットしておらず、ローテーションする場合は鍵を再生成のうえSecretsと`pubkey`を両方更新する必要がある**

## ステータス

現在、初期スキャフォールディング段階です。仕様は今後変更される可能性があります。

## ライセンス

未定
