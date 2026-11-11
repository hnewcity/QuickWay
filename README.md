<p align="center">
  <img src="design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Your Gateway to Amazon Quick</h3>
<p align="center">Use Quick's models in Claude Code, Codex and any OpenAI- or Anthropic-compatible app through a local API</p>

<p align="center">
  🇺🇸 English • <a href="docs/README_zh.md">🇨🇳 中文</a> • <a href="docs/README_ja.md">🇯🇵 日本語</a> • <a href="docs/README_ko.md">🇰🇷 한국어</a> • <a href="docs/README_ru.md">🇷🇺 Русский</a> • <a href="docs/README_es.md">🇪🇸 Español</a> • <a href="docs/README_pt.md">🇧🇷 Português</a> • <a href="docs/README_id.md">🇮🇩 Indonesia</a> • <a href="docs/README_vi.md">🇻🇳 Tiếng Việt</a>
</p>

<p align="center">
  <a href="https://github.com/hnewcity/QuickWay/actions/workflows/ci.yml"><img src="https://github.com/hnewcity/QuickWay/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-brightgreen" alt="Platform">
  <a href="http://makeapullrequest.com"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome"></a>
</p>

<p align="center">
  <img src="docs/app-screenshot.png" alt="QuickWay Screenshot" width="800">
</p>

---

## ❤️ Sponsor

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="docs/sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td>Thanks to <a href="https://ai18n.chat">ai18n</a> for sponsoring this project! ai18n is a reliable and efficient API relay service provider, offering relay Claude models for OpenClaw, Claude Code, Codex, Gemini, and more.</td>
</tr>
</table>

---

QuickWay is a desktop gateway that exposes the models of your signed-in Amazon Quick account through a local Anthropic-compatible and OpenAI-compatible API. Use Claude Code, Codex and your favorite SDKs and tools with Quick - no code changes required.

## ✨ Features

| Feature | Description |
|---|---|
| 🔌 **Anthropic-compatible API** | `/v1/messages` endpoint for the Anthropic SDK and Claude Code |
| 🔌 **OpenAI-compatible API** | `/v1/chat/completions` and `/v1/responses` endpoints for the OpenAI SDK and Codex |
| 🚀 **Coding Launcher** | Start Claude Code or Codex in its own terminal with one click, each with its own model and effort |
| 🧠 **Thinking Effort** | Off / Low / Medium / High, mapped to each model's native reasoning controls |
| 📋 **Dynamic Model Catalog** | Models are read from your Quick account and refreshed automatically |
| 🛠️ **Tool Calling** | Function calling for Messages, Chat Completions and Responses |
| 👁️ **Vision Support** | Send base64 images to the model |
| 📡 **Streaming** | Full SSE streaming support |
| 💬 **Built-in Chat** | Test your setup with the integrated chat interface |
| 🔐 **Local & Safe Credentials** | Loopback-only proxy with its own API key; tokens stay in memory and come from Windows Credential Manager or macOS Keychain |
| 🔄 **Credential Transfer** | Export and import credentials between your own Windows and macOS machines |
| 🔗 **CC Switch Integration** | One-click import to [CC Switch](https://github.com/farion1231/cc-switch) for Claude Code |
| 🌍 **Multi-language UI** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **Diagnostics** | Copy a redacted diagnostics report when a connection or usage check fails |

## 📦 Installation

### Download

Download the latest release from [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| Platform | Architecture | Download |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 installer | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 portable | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS / Windows | Command-line proxy | [quickway-cli_x.y.z_*](https://github.com/hnewcity/QuickWay/releases/latest) |

> Requires Windows 10/11 x64 or macOS 12+. The Windows installer is not code-signed and macOS builds are ad-hoc signed without notarization, so SmartScreen or Gatekeeper may ask you to confirm on first launch. Sign in to Amazon Quick on the same computer first.

### Build from Source

```bash
# Clone the repo
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# Install dependencies
npm ci

# Run in dev mode
npm run tauri dev

# Or build for production
npm run desktop:build
```

Requires Rust (see `rust-toolchain.toml`) and Node.js 20.19+ or 22.12+. On macOS also install the Xcode Command Line Tools.

## 🚀 Quick Start

1. **Sign in** to Amazon Quick on this computer
2. **Launch** QuickWay - it picks up the signed-in account automatically
3. **Open** the Coding tab, choose Claude Code or Codex, a model and a project folder, then press Launch
4. **Or** use `http://127.0.0.1:8741` as your Anthropic/OpenAI API endpoint, with the API key from Connection settings

> Model IDs come from your account. Use an ID returned by `GET /v1/models`.

### Example: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Example: Python (OpenAI SDK)

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

### Example: JavaScript (OpenAI SDK)

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

## 🔌 Works With

| Category | Tools |
|---|---|
| **Coding agents** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **Tools** | CC Switch, Cursor, Continue, Cline |

## ⚙️ Configuration

### Authentication Methods

| Method | Description |
|---|---|
| **Local Quick sign-in** | Reads the signed-in Amazon Quick account (Windows Credential Manager or macOS Keychain) - recommended |
| **Imported credentials** | A `.qwc` file exported from another of your own machines |

### Server Settings

| Option | Default | Description |
|---|---|---|
| Host | `127.0.0.1` | Loopback only; the proxy never listens on other interfaces |
| Port | `8741` | Server port, changeable in Connection settings |
| Proxy API Key | auto-generated | Required for every request: `x-api-key` or `Authorization: Bearer` |

### Advanced Settings

| Option | Description |
|---|---|
| Thinking effort | Off / Low / Medium / High per chat or launch, or the `x-quickway-thinking-effort` request header |
| Model catalog | Refreshed every 5 minutes, or on demand from the chat toolbar |
| Start with app | Start the proxy automatically when QuickWay opens |

## 🛠️ Tech Stack

| Layer | Technology |
|---|---|
| **Frontend** | TypeScript + Vite |
| **Desktop** | Tauri 2 (Rust) |
| **Backend** | Rust + axum |

## 🤝 Contributing

Contributions are welcome! Feel free to:

- 🐛 Report bugs via [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 Suggest features
- 🔧 Submit pull requests
- 🌍 Help with translations

## 📚 Documentation

- [User guide (中文)](docs/USER_GUIDE_zh.md): detailed manual covering credentials, models, the OpenAI-compatible API and Codex
- [Releasing](docs/RELEASING.md): how versions are bumped and releases are published

## 📄 License

[MIT](LICENSE) © QuickWay Contributors
