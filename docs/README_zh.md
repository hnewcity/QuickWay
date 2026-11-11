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
| 🧠 **思考强度** | 关闭 / 低 / 中 / 高，对应各模型原生的推理参数 |
| 📋 **动态模型目录** | 模型直接来自你的 Quick 账号，并自动刷新 |
| 🛠️ **工具调用** | Messages、Chat Completions 和 Responses 均支持函数调用 |
| 👁️ **视觉支持** | 向模型发送 base64 图片 |
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
| macOS / Windows | 命令行代理 | [quickway-cli_x.y.z_*](https://github.com/hnewcity/QuickWay/releases/latest) |

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
3. **打开**「编程」页，选择 Claude Code 或 Codex、模型和项目文件夹，然后点击启动
4. **或者**把 `http://127.0.0.1:8741` 作为 Anthropic / OpenAI API 地址，API 密钥在「连接设置」中查看

> 模型 ID 来自你的账号，请使用 `GET /v1/models` 返回的 ID。

### 示例：cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
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
    model="claude-sonnet-4-6",
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
  model: 'claude-sonnet-4-6',
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
| 模型目录 | 每 5 分钟刷新一次，也可在聊天工具栏手动刷新 |
| 随应用启动 | QuickWay 打开时自动启动代理 |

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
