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

- Fabric などの前提Modローダーの導入をサポート
- プロファイルごとに Mod の取得元 URL (Modrinth / CurseForge など) を入力するだけで、自動的に取得・設置
- 前提Mod(依存Mod)を自動的に解決し、依存関係も含めて自動インストールすることで、ユーザーが個別に依存関係を調べて導入する手間をなくす
- リソースパックについても Mod と同様の仕組みで URL 指定による自動導入に対応

### 認証

- Microsoft アカウントでのサインインには Microsoft Entra ID (Azure AD) アプリ登録 + Minecraft API 利用申請 (`XboxLive.signin` スコープ) が必要
- Discord サインインには Discord OAuth2 を利用
- 認証フロー自体は `crates/auth` に実装済み(MSAはデバイスコードフロー、DiscordはPKCE付き認可コードフロー+ローカルループバックサーバでのリダイレクト受信)。取得したトークンはOSの資格情報ストア(Windows Credential Manager / macOS Keychain / Linux Secret Service)に保存される

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

### ワークスペース構成

```
Cargo.toml         # Cargo workspace ルート
crates/
  core/            # train-launcher-core: プロファイル管理、MCバージョンマニフェスト/ライブラリ/アセットのダウンロード、起動コマンド構築
  auth/            # train-launcher-auth: MSA/XboxLive/Minecraft 認証チェーン、Discord OAuth2
  mods/            # train-launcher-mods: Modrinth/CurseForge からの Mod 解決・依存関係自動解決・インストール
  train-api/       # train-launcher-server-api: TRAiN 独自バックエンドAPIクライアント(現時点ではトレイト+モックのスタブ実装)
apps/
  desktop/         # Tauri アプリ本体(上記 crate を呼び出す Tauri commands を定義)
frontend/          # React + Vite + TypeScript + Fluent UI React v9 (`@fluentui/react-components`) 製フロントエンド
```

### 補足

- MSA/Discord 認証フロー(`crates/auth`)は実装済みです。Minecraft 本体の起動処理、Mod 依存解決・ダウンロード、TRAiN 独自APIの実装は今後のタスクで対応予定です
- Windows で `cargo build` 時に MSVC ヘッダ(`vcruntime.h` 等)が見つからないエラーが出る場合は、Visual Studio Installer で「C++ によるデスクトップ開発」ワークロードが正しくインストールされているか確認してください

## ステータス

現在、初期スキャフォールディング段階です。仕様は今後変更される可能性があります。

## ライセンス

未定
