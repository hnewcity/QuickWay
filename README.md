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
| 🧠 **Thinking Effort** | Cloud effort hints, plus native reasoning controls on legacy model routes |
| 📋 **Cloud Tiers** | Choose Fast, Balanced, Smart or Advanced; see the model actually used in each response |
| 🛠️ **Tool Calling** | Function calling for Messages, Chat Completions and Responses |
| 👁️ **Vision Support** | Base64 images on supported legacy model routes; cloud modes currently accept text and tools |
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
3. **Open** the Coding tab, choose Claude Code or Codex, the recommended `quick-smart` mode and a project folder, then press Launch
4. **Or** use `http://127.0.0.1:8741` as your Anthropic/OpenAI API endpoint, with the API key from Connection settings

Use an ID returned by `GET /v1/models`. New desktop installations default to `quick-smart`; existing saved model selections are preserved.

### Quick cloud modes

| API model ID | Quick agent mode |
|---|---|
| `quick-fast` | `fast` |
| `quick-balanced` | `balanced` |
| `quick-smart` | `smart` (recommended default) |
| `quick-advanced` | `advanced` |

These IDs select Quick's cloud agent modes. Quick chooses the concrete model. The cloud route currently supports social/AIDP sign-ins; enterprise/gateway sign-ins are not supported by this route.

The legacy fixed-model route can return `AccessDeniedException: This client is not supported for this request.` A successful sign-in or model catalog fetch does not establish inference access. Fixed IDs remain in the API catalog, while desktop pickers show only cloud tiers. Saved fixed-model selections require an explicit tier choice.

Chat replies preserve the actual model reported by the server. Dashboard and Coding also show the current account's latest completed cloud response through the local proxy, including requests from Claude Code, Codex and API clients. Missing model metadata stays unknown; selecting a tier does not guarantee a particular model version.

Cloud conversations use a stateful connection, cached in the current QuickWay process for up to 10 idle minutes. After a restart, expiry or cache eviction, QuickWay can rebuild a cloud session from supported, completed history: text messages and paired tool calls/results, including multiple text result blocks and tool error status. Ordinary tool round-trips continue directly while their original session is retained.

After a connection is lost, a request carrying all completed tool results can rebuild the session and continue from those results. QuickWay does not execute tools during rebuilding. History rebuilding still rejects unpaired tool calls/results, nonempty assistant text after a tool call, user text before tool results, images/documents and signed thinking blocks. Missing tool results cannot be manufactured or resumed on a new session.

Quick controls generation and reasoning limits. `max_tokens` is retained for diagnostics, not enforced as an upstream cap; thinking budgets become effort hints. The Off setting does not guarantee that cloud reasoning is disabled. Cloud modes currently support text and tool calls, and reject unsupported sampling controls (`temperature`, `top_p`, `top_k`), images, documents, forced tool choice and structured outputs.

### Example: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "quick-smart",
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
    model="quick-smart",
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
  model: 'quick-smart',
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
| Cloud tiers | Refreshed every 5 minutes, or on demand from the chat toolbar |
| Start with app | Start the proxy automatically when QuickWay opens |

### Diagnostics

`quickway doctor --online` checks sign-in, account access and the model catalog without generating a reply. Its report marks inference as `not_checked`; a working catalog does not prove model requests work.

Run `quickway doctor --probe-model quick-smart` to also make one model request (choose an ID from `/v1/models`). The probe requests a brief reply with `max_tokens: 32` and uses a 30-second deadline. Cloud modes do not guarantee that token cap. The probe consumes model quota and reports completion or the upstream error without including generated text.

### Live cloud checks

These explicit, opt-in checks use synthetic prompts and fixtures with `quick-smart`, but make real requests and consume Quick quota. Run them from the repository root with a supported social/AIDP sign-in and `python` available on PATH. Each command starts a temporary proxy on a random loopback port with a temporary API key, then shuts it down and unregisters its temporary cloud devices after the suite, including when a check fails.

```sh
cargo run --example smoke_cloud -- messages
cargo run --example smoke_cloud -- openai
cargo run --example smoke_cloud -- history
cargo run --example smoke_cloud -- claude
cargo run --example smoke_cloud -- claude-normal
cargo run --example smoke_cloud -- claude-project
cargo run --example smoke_cloud -- codex-project
cargo run --example smoke_cloud -- codex-recovery
```

| Suite | Checks and additional requirements |
|---|---|
| `messages` | Messages text, streaming and automatic tool round-trips |
| `openai` | Chat Completions and Responses text, streaming and tools; requires the Python OpenAI SDK (`python -m pip install openai`) |
| `history` | Completed text/tool history rebuilding, multiple tool calls, follow-ups and cancellation |
| `claude` | Requires an installed `claude` CLI; enables only its `Read` tool to read a synthetic file in a temporary directory |
| `claude-normal` | Loads normal Claude Code settings and the full default tool catalog; allows `Read` to read the synthetic fixture |
| `claude-project` | Full normal tool catalog; creates a runnable Python project, sends a user update during a shell call, then adds a failing acceptance test for CC to fix. Independently runs the final tests and retains the synthetic project/report in a temporary directory. |
| `codex-project` | Uses the installed Codex CLI, normal user settings and the desktop launcher's Responses provider/catalog configuration. Creates and tests a Python project, resumes the same Codex thread for a failing new requirement, and independently verifies the completed CLI. Covers native freeform tools, including code-mode `exec` calls that apply patches and run commands. |
| `codex-recovery` | Runs the same real Codex project acceptance, deliberately restarting only the test proxy's cloud sessions after the project files have been created and before the tool result is forwarded. Verifies rebuilding from completed results and continuing the project without modifying existing tests. Recovered transport errors are recorded explicitly; HTTP 400 and model failures fail the run. The fault-injection endpoint exists only in this test harness. |

User messages arriving alongside tool results continue in the same cloud session. The bridge carries the new text once in a labeled client-update block on the final tool result, preserving original tool output and error status. Quick's native decision widgets are presented as plain questions and Markdown options in API replies; fenced XML examples remain literal.

History matching treats omitted tool parameters and their declared schema defaults as equivalent, including Claude Code's automatic `Edit.replace_all=false`. Tool IDs, paths, replacement text and non-default values must still match.

Cloud tool descriptions that exceed Quick's 8192-character registration limit retain their full documentation in the system prompt. In cloud replies, standard input/cache usage describes the last internal model call, so repeated cloud prompts do not trigger premature client compaction. `quickway.cloud_usage` on Messages replies preserves the aggregate upstream usage for the reply. Claude Code's transient remaining-token counters are treated as client bookkeeping. The desktop launcher declares a 200K client compaction budget for custom cloud tiers; this does not pin their serving model or guarantee an upstream context limit.

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
