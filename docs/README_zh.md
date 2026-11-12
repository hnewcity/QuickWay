<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">通往 Amazon Quick 的网关</h3>
<p align="center">通过本机 API，在 Claude Code、Codex 以及任何兼容 OpenAI / Anthropic 的应用中使用 Quick 的模型</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • 🇨🇳 中文 • <a href="README_ja.md">🇯🇵 日本語</a> • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • <a href="README_pt.md">🇧🇷 Português</a> • <a href="README_id.md">🇮🇩 Indonesia</a> • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
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

## ❤️ 赞助商

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td>感谢 <a href="https://ai18n.chat">ai18n</a> 赞助本项目！ai18n 是一个可靠高效的 API 中转服务提供商，为 OpenClaw、Claude Code、Codex、Gemini 等提供 Claude 模型中转服务。</td>
</tr>
</table>

---

QuickWay 是一个桌面网关，通过本机的 Anthropic 兼容和 OpenAI 兼容 API，开放你已登录的 Amazon Quick 账号中的模型。在 Claude Code、Codex 以及你喜欢的 SDK 和工具中使用 Quick，无需修改代码。

## ✨ 功能特性

| 功能 | 描述 |
|---|---|
| 🔌 **Anthropic 兼容 API** | `/v1/messages` 端点，支持 Anthropic SDK 和 Claude Code |
| 🔌 **OpenAI 兼容 API** | `/v1/chat/completions` 和 `/v1/responses` 端点，支持 OpenAI SDK 和 Codex |
| 🚀 **编程启动器** | 一键在独立终端中启动 Claude Code 或 Codex，各自记住模型和思考强度 |
| 🧠 **思考强度** | 云端模式使用强度提示，旧模型通道使用原生推理参数 |
| 📋 **云端档位** | 选择 Fast、Balanced、Smart 或 Advanced，并查看每次响应实际使用的模型 |
| 🛠️ **工具调用** | Messages、Chat Completions 和 Responses 均支持函数调用 |
| 👁️ **视觉支持** | 支持图片的旧模型通道可接收 base64 图片；云端模式目前支持文本和工具 |
| 📡 **流式传输** | 完整 SSE 流式支持 |
| 💬 **内置聊天** | 用内置聊天界面测试你的配置 |
| 🔐 **本机运行，凭据安全** | 代理仅监听本机，使用独立的 API 密钥；令牌只存在内存中，来自 Windows 凭据管理器或 macOS 钥匙串 |
| 🔄 **凭证迁移** | 在你自己的 Windows 和 macOS 设备之间导出、导入凭证 |
| 🔗 **CC Switch 集成** | 一键导入 [CC Switch](https://github.com/farion1231/cc-switch)，用于 Claude Code |
| 🌍 **多语言界面** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **诊断报告** | 连接或额度检查失败时，可复制已脱敏的诊断报告 |

## 📦 安装

### 下载

从以下页面下载最新版本： [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| 平台 | 架构 | 下载 |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 安装包 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 免安装版 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> 需要 Windows 10/11 x64 或 macOS 12+。Windows 安装包未做代码签名，macOS 版本使用 ad-hoc 签名且未公证，首次打开时 SmartScreen 或 Gatekeeper 可能要求你确认。请先在同一台电脑上登录 Amazon Quick。

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# 安装依赖
npm ci

# 开发模式运行
npm run tauri dev

# 或构建生产版本
npm run desktop:build
```

需要 Rust（版本见 `rust-toolchain.toml`）和 Node.js 20.19+ 或 22.12+。macOS 还需安装 Xcode Command Line Tools。

## 🚀 快速开始

1. 在这台电脑上**登录** Amazon Quick
2. **启动** QuickWay，它会自动读取已登录的账号
3. **打开**「编程」页，选择 Claude Code 或 Codex、推荐的 `quick-smart` 模式和项目文件夹，然后点击启动
4. **或者**把 `http://127.0.0.1:8741` 作为 Anthropic / OpenAI API 地址，API 密钥在「连接设置」中查看

请使用 `GET /v1/models` 返回的 ID。桌面端首次使用默认选择 `quick-smart`，已有的模型选择会保留。

### Quick 云端模式

| API 模型 ID | Quick 智能体模式 |
|---|---|
| `quick-fast` | `fast` |
| `quick-balanced` | `balanced` |
| `quick-smart` | `smart`（推荐默认值） |
| `quick-advanced` | `advanced` |

这些 ID 选择的是 Quick 云端智能体模式，具体模型由 Quick 调度。目前云端通道支持 social/AIDP 登录，不支持 enterprise/gateway 登录。

旧的固定模型通道可能返回 `AccessDeniedException: This client is not supported for this request.`。登录成功或成功获取模型目录，不代表推理请求可用。API 目录仍保留固定型号，桌面选择器仅展示云端档位；已有固定型号选择需要手动重新选择档位。

聊天回复会保存服务端返回的实际型号。概览和编程页还会显示当前账号通过本地代理完成的最近一次云端响应型号，包括 Claude Code、Codex 和 API 客户端的请求。缺少型号信息时显示未知，选择档位不代表锁定某个模型版本。

云端对话使用有状态连接，在当前 QuickWay 进程内最多缓存 10 分钟的闲置连接。重启、缓存过期或连接被回收后，QuickWay 可以根据已完成且受支持的历史重建云端会话：包括文本消息，以及已配对完成的工具调用和结果，支持多个文本结果块及工具错误状态。原会话仍保留时，正常的多轮工具调用可以直接续接。

仍在等待工具结果的会话需要原进程和原连接。历史重建会拒绝未配对的工具调用/结果、工具调用之后仍有非空文本的 assistant 消息、先出现文本再出现工具结果的 user 消息、图片/文档，以及带签名的思考块。如果这类会话的原连接已不可用，请新建对话。

生成长度和推理限额由 Quick 云端控制。`max_tokens` 仅保留用于诊断，不是上游强制上限；思考 token 预算会转换成强度提示。「关闭」也不能保证禁用云端推理。目前云端模式支持文本和工具调用，明确拒绝不支持的采样参数（`temperature`、`top_p`、`top_k`）、图片、文档、强制工具选择和结构化输出。

### 示例：cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "quick-smart",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 示例：Python (OpenAI SDK)

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

### 示例：JavaScript (OpenAI SDK)

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

## 🔌 兼容工具

| 类别 | 工具 |
|---|---|
| **编程助手** | Claude Code、Codex CLI |
| **Python** | OpenAI SDK、Anthropic SDK、LangChain、LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK、Anthropic SDK、Vercel AI SDK |
| **工具** | CC Switch、Cursor、Continue、Cline |

## ⚙️ 配置

### 认证方式

| 方式 | 描述 |
|---|---|
| **本机 Quick 登录** | 读取已登录的 Amazon Quick 账号（Windows 凭据管理器或 macOS 钥匙串），推荐 |
| **导入凭证** | 从你自己的另一台设备导出的 `.qwc` 文件 |

### 服务器设置

| 选项 | 默认值 | 描述 |
|---|---|---|
| 主机 | `127.0.0.1` | 仅限本机回环地址，代理不会监听其他网络接口 |
| 端口 | `8741` | 服务端口，可在「连接设置」中修改 |
| 代理 API 密钥 | 自动生成 | 每个请求都必须携带：`x-api-key` 或 `Authorization: Bearer` |

### 高级设置

| 选项 | 描述 |
|---|---|
| 思考强度 | 每个对话或每次启动可选 关闭 / 低 / 中 / 高，也可用 `x-quickway-thinking-effort` 请求头指定 |
| 云端档位 | 每 5 分钟刷新一次，也可在聊天工具栏手动刷新 |
| 随应用启动 | QuickWay 打开时自动启动代理 |

### 诊断

`quickway doctor --online` 检查登录、账号访问和模型目录，不会生成回复。报告中的推理状态为 `not_checked`；目录可用不代表模型请求可用。

运行 `quickway doctor --probe-model quick-smart` 会额外发送一次模型请求，也可选择 `/v1/models` 中的其他 ID。探针请求简短回复并设置 `max_tokens: 32`，超时时间为 30 秒；云端模式不保证该 token 上限。此操作会消耗模型额度，报告仅包含完成状态或上游错误，不包含生成的内容。

## 🛠️ 技术栈

| 层级 | 技术 |
|---|---|
| **前端** | TypeScript + Vite |
| **桌面** | Tauri 2 (Rust) |
| **后端** | Rust + axum |

## 🤝 贡献

欢迎贡献！你可以：

- 🐛 通过以下页面报告问题： [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 提出功能建议
- 🔧 提交 Pull Request
- 🌍 帮助翻译

## 📚 文档

- [QuickWay 使用手册](USER_GUIDE_zh.md): 详细手册，涵盖凭证、模型、OpenAI 兼容 API 和 Codex
- [发布流程](RELEASING.md): 版本号如何更新、发布如何自动完成

## 📄 许可证

[MIT](../LICENSE) © QuickWay Contributors
