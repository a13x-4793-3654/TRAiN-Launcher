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

## ステータス

現在、企画・設計段階です。仕様は今後変更される可能性があります。

## ライセンス

未定
