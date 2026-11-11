<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Amazon Quick へのゲートウェイ</h3>
<p align="center">ローカル API を通じて、Claude Code、Codex、そして OpenAI / Anthropic 互換のあらゆるアプリで Quick のモデルを利用</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • 🇯🇵 日本語 • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • <a href="README_pt.md">🇧🇷 Português</a> • <a href="README_id.md">🇮🇩 Indonesia</a> • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
</p>

<p align="center">
  <a href="https://github.com/hnewcity/QuickWay/actions/workflows/ci.yml"><img src="https://github.com/hnewcity/QuickWay/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-brightgreen" alt="Platform">
  <a href="http://makeapullrequest.com"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome"></a>
</p>

<p align="center">
  <img src="app-screenshot.png" alt="QuickWay Screenshot" width="800">
</p>

---

## ❤️ スポンサー

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td><a href="https://ai18n.chat">ai18n</a> にスポンサーいただきありがとうございます！ai18n は信頼性が高く効率的な API リレーサービスプロバイダーで、OpenClaw、Claude Code、Codex、Gemini などの Claude モデルリレーを提供しています。</td>
</tr>
</table>

---

QuickWay は、ログイン済みの Amazon Quick アカウントのモデルを、ローカルの Anthropic 互換・OpenAI 互換 API として公開するデスクトップゲートウェイです。Claude Code、Codex、お好みの SDK やツールで、コードを変更せずに Quick を利用できます。

## ✨ 機能

| 機能 | 説明 |
|---|---|
| 🔌 **Anthropic 互換 API** | Anthropic SDK と Claude Code 向けの `/v1/messages` エンドポイント |
| 🔌 **OpenAI 互換 API** | OpenAI SDK と Codex 向けの `/v1/chat/completions` および `/v1/responses` エンドポイント |
| 🚀 **コーディングランチャー** | Claude Code または Codex を専用ターミナルでワンクリック起動。モデルと強度はそれぞれ記憶 |
| 🧠 **思考の強度** | オフ / 低 / 中 / 高。各モデルのネイティブな推論設定に対応 |
| 📋 **動的モデルカタログ** | モデルは Quick アカウントから取得され、自動的に更新 |
| 🛠️ **ツール呼び出し** | Messages、Chat Completions、Responses で関数呼び出しに対応 |
| 👁️ **ビジョン対応** | base64 画像をモデルに送信 |
| 📡 **ストリーミング** | SSE ストリーミングに完全対応 |
| 💬 **内蔵チャット** | 統合チャット画面で設定をテスト |
| 🔐 **ローカル動作と安全な認証情報** | ループバック限定のプロキシと専用 API キー。トークンはメモリ内のみで、Windows 資格情報マネージャーまたは macOS キーチェーンから取得 |
| 🔄 **認証情報の移行** | ご自身の Windows / macOS マシン間で認証情報をエクスポート・インポート |
| 🔗 **CC Switch 連携** | Claude Code 用に [CC Switch](https://github.com/farion1231/cc-switch) へワンクリックでインポート |
| 🌍 **多言語 UI** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **診断レポート** | 接続や使用量の確認に失敗したとき、マスク済みの診断レポートをコピー |

## 📦 インストール

### ダウンロード

最新リリースはこちらからダウンロードできます： [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| プラットフォーム | アーキテクチャ | ダウンロード |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 インストーラー | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 ポータブル版 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> Windows 10/11 x64 または macOS 12 以降が必要です。Windows インストーラーはコード署名されておらず、macOS 版は ad-hoc 署名で公証されていないため、初回起動時に SmartScreen や Gatekeeper の確認が表示される場合があります。事前に同じコンピューターで Amazon Quick にログインしてください。

### ソースからビルド

```bash
# リポジトリをクローン
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# 依存関係をインストール
npm ci

# 開発モードで実行
npm run tauri dev

# または本番用にビルド
npm run desktop:build
```

Rust（バージョンは `rust-toolchain.toml` を参照）と Node.js 20.19+ または 22.12+ が必要です。macOS では Xcode Command Line Tools も必要です。

## 🚀 クイックスタート

1. このコンピューターで Amazon Quick に**ログイン**
2. QuickWay を**起動**。ログイン済みのアカウントを自動的に認識します
3. 「コーディング」タブを**開き**、Claude Code か Codex、モデル、プロジェクトフォルダーを選んで起動
4. **または** `http://127.0.0.1:8741` を Anthropic / OpenAI API エンドポイントとして使用（API キーは「接続設定」で確認）

> モデル ID はアカウントによって異なります。`GET /v1/models` が返す ID を使用してください。

### 例：cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 例：Python (OpenAI SDK)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:8741/v1",
    api_key="YOUR_PROXY_API_KEY"
)

response = client.chat.completions.create(
    model="claude-sonnet-4-6",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)
```

### 例：JavaScript (OpenAI SDK)

```javascript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://127.0.0.1:8741/v1',
  apiKey: 'YOUR_PROXY_API_KEY',
});

const response = await client.chat.completions.create({
  model: 'claude-sonnet-4-6',
  messages: [{ role: 'user', content: 'Hello!' }],
});
console.log(response.choices[0].message.content);
```

## 🔌 対応ツール

| カテゴリ | ツール |
|---|---|
| **コーディングエージェント** | Claude Code、Codex CLI |
| **Python** | OpenAI SDK、Anthropic SDK、LangChain、LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK、Anthropic SDK、Vercel AI SDK |
| **ツール** | CC Switch、Cursor、Continue、Cline |

## ⚙️ 設定

### 認証方式

| 方式 | 説明 |
|---|---|
| **ローカルの Quick ログイン** | ログイン済みの Amazon Quick アカウント（Windows 資格情報マネージャーまたは macOS キーチェーン）を読み取ります（推奨） |
| **インポートした認証情報** | ご自身の別のマシンからエクスポートした `.qwc` ファイル |

### サーバー設定

| オプション | デフォルト | 説明 |
|---|---|---|
| ホスト | `127.0.0.1` | ループバック限定。プロキシは他のインターフェースでは待ち受けません |
| ポート | `8741` | サーバーポート。「接続設定」で変更できます |
| プロキシ API キー | 自動生成 | すべてのリクエストに必須：`x-api-key` または `Authorization: Bearer` |

### 詳細設定

| オプション | 説明 |
|---|---|
| 思考の強度 | チャットまたは起動ごとにオフ / 低 / 中 / 高を選択。`x-quickway-thinking-effort` リクエストヘッダーでも指定可能 |
| モデルカタログ | 5 分ごとに更新。チャットのツールバーから手動更新も可能 |
| アプリと同時に起動 | QuickWay を開いたときにプロキシを自動起動 |

## 🛠️ 技術スタック

| レイヤー | 技術 |
|---|---|
| **フロントエンド** | TypeScript + Vite |
| **デスクトップ** | Tauri 2 (Rust) |
| **バックエンド** | Rust + axum |

## 🤝 コントリビュート

コントリビュートを歓迎します！次のような形で参加できます：

- 🐛 バグを報告： [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 機能を提案
- 🔧 プルリクエストを送信
- 🌍 翻訳を手伝う

## 📚 ドキュメント

- [User guide (中文)](USER_GUIDE_zh.md): 認証情報、モデル、OpenAI 互換 API、Codex を網羅した詳細マニュアル
- [リリース手順](RELEASING.md): バージョンの更新とリリースの自動公開の仕組み

## 📄 ライセンス

[MIT](../LICENSE) © QuickWay Contributors
