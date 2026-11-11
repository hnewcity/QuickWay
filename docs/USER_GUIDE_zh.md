# QuickWay 使用手册

> 本文是详细的中文使用手册。项目简介、下载和快速开始见 [README](README_zh.md)。

用 Rust 将电脑上已登录的 Amazon Quick 会话接到 Claude Code。Tauri 2 桌面版内置流式聊天、模型切换、代理管理与 Claude Code 启动器。

## 开始使用

### Windows 桌面版

从 [GitHub Releases](https://github.com/hnewcity/QuickWay/releases/latest) 下载 `QuickWay_<版本>_x64-setup.exe` 安装，或直接运行免安装的 `QuickWay_<版本>_x64-portable.exe`。需要 Windows 10/11 x64 与 WebView2 Runtime；安装包会在缺少 Runtime 时引导安装。

1. 在 Amazon Quick 中登录，打开 QuickWay 自动读取本机当前账号。
2. 在「对话」直接聊天，首次使用先选择模型；顶部可切换从当前 Quick 登录动态读取的全部模型，以及 Off / Low / Medium / High 思考强度。支持 Markdown、多轮、停止生成、复制回复与本机历史；每段对话单独记住强度，旧对话默认 Off。
3. 在「Claude Code」选择项目文件夹、模型和思考强度，点击启动。需要电脑上已安装 Claude Code。强度绑定这次启动；调整后重新启动一个 Claude Code 会话。
4. 在「Codex」同样选择项目文件夹、模型和思考强度后启动。Codex 单独记住自己的模型和强度，未选择时沿用默认模型。需要已安装 Codex CLI（PATH 或 npm 中的 `codex`），Windows 上也能使用 Codex 桌面应用自带的 `codex.exe`。详见下文「连接 Codex CLI」。
5. 在「连接设置」查看真实额度、启动/停止代理、修改端口或复制连接配置。

关闭窗口后继续在系统托盘运行，右键托盘选择退出才能停止应用。Claude Code 通过 `--settings` 加载 QuickWay 生成的本机连接配置，覆盖当前启动中已有的 API 设置，不修改全局配置文件。

思考强度越高，通常耗时和用量越多。Opus / Sonnet 使用自适应 effort；Haiku 4.5 使用 1024 / 4096 / 8192 token 思考预算，Haiku 5.5 使用自适应 effort；GPT 使用 `reasoning.effort`，Off 显式发送 `none`。Sonnet 5.5 的 Off 按服务端要求发送 `between_tools`，关闭回答前的思考，工具之间仍可能返回简短更新。Opus 5.5 不接受关闭思考或 `none`，Off 使用最低档 `adaptive + low`。自适应档位不保证每次返回可见的思考块。

启动器使用独立的 `claude-connection-{effort}.json` 文件和 `x-quickway-thinking-effort` 本机请求头落实所选档位，兼容 Claude Code 对不同模型的参数生成差异。普通 API 客户端未发送该请求头时，继续使用请求自身的 `thinking` 和 `output_config`；直接运行 `claude` 时，Claude Code 内的 `/effort`、`--effort`（low 至 max）按原值生效。应用内仅显示回答与所选强度，不展示思考文本。

桌面版设置及独立代理密钥位于 `%APPDATA%\dev.quickway.desktop`；对话保存在当前用户的 WebView2 本机存储。默认从 Windows 凭据管理器读取账号令牌，仅在内存中续期；手动导入的凭证使用下述文件保存方式。发送对话时，内容会通过 Quick 发给选定模型。

### macOS 桌面版

从 [GitHub Releases](https://github.com/hnewcity/QuickWay/releases/latest) 下载对应芯片的 `QuickWay_<版本>_aarch64.dmg`（Apple Silicon）或 `QuickWay_<版本>_x64.dmg`（Intel），打开后将 QuickWay 拖入「应用程序」。最低系统版本设置为 macOS 12；使用系统自带的 WKWebView，无需 WebView2。当前包使用 ad-hoc 本地签名，尚未完成 Apple 公证；通过网络分发后可能需要在「隐私与安全性」中确认打开。

- 先在 Amazon Quick 中登录。支持生产 App Integrations 社交登录和企业登录，凭据从 macOS 钥匙串读取；首次访问时按系统提示授权。
- 企业账号请保持 Amazon Quick 运行以续期登录。Mac 授权成功后，聊天、状态、额度和模型目录共用进程内的有效会话；有效期内不重复读取钥匙串。令牌过期时才尝试读取 Quick 续期后的凭据，不使用社交登录的刷新接口。
- Claude Code 使用 Terminal 打开，支持官方 `~/.local/bin` 安装、Homebrew 和进程 PATH 中的安装。Finder 启动时也能检测常用安装位置；终端连接命令按 zsh/bash 转义。
- `⌘N` 新建对话，`⌘Q` 退出。关闭窗口后代理仍在菜单栏运行，点击 Dock 图标可重新打开窗口。
- 设置及本地代理密钥位于 `~/Library/Application Support/dev.quickway.desktop`；密钥及 Claude 连接文件只允许当前用户读写。对话保存在本机 WebKit 存储。

在 Mac 上构建需要 Rust、Node.js 和 Xcode Command Line Tools（`xcode-select --install`）：

```sh
cd desktop
npm ci
npm run desktop:build
```

同一个构建命令会根据系统生成 Mac 的 App/DMG 或 Windows 的 NSIS 安装包。Mac 原生架构输出位于 `desktop/src-tauri/target/release/bundle/`。构建通用包：

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri -- build --target universal-apple-darwin
```

通用包输出位于 `desktop/src-tauri/target/universal-apple-darwin/release/bundle/`。Apple Silicon 已做原生运行验证；Intel 核心测试通过 Rosetta 执行，尚未在 Intel 实机及 macOS 12 上测试。

### Windows 命令行版

在 PowerShell 中运行：

```powershell
cd path\to\QuickWay
.\scripts\Start-QuickClaude.ps1
```

脚本启动本地代理，读取 `/v1/models` 后让你选择模型，再打开 Claude Code。传入 `-Model` 时也必须匹配当前动态目录。也可以在任意项目目录调用该脚本，Claude Code 会在当前目录工作。启动脚本仅为本次 Claude Code 进程设置连接环境，退出后恢复环境，不改全局 Claude Code 配置。

```powershell
# 指定当前云端目录里存在的模型（示例）
.\scripts\Start-QuickClaude.ps1 -Model claude-opus-4-8

# 只启动代理 / 停止由脚本启动的代理
.\scripts\Start-QuickWay.ps1
.\scripts\Stop-QuickWay.ps1

# 检查当前 Quick 登录和真实额度
.\target\release\quickway.exe doctor --online
```

第一次构建需要 Rust；已构建后可直接使用 `target\release\quickway.exe`。监听地址默认为 `http://127.0.0.1:8741`。本地代理密钥自动保存在 `.quickway\proxy.key`；账号令牌从 Windows 凭据管理器读取，不需要手动导出。

### macOS 命令行版

```sh
cargo build --release --locked
./target/release/quickway doctor --online
./target/release/quickway serve
```

代理默认监听 `127.0.0.1:8741`，密钥生成在当前目录的 `.quickway/proxy.key`。`Ctrl+C` 停止。Claude Code 可使用桌面版「复制连接配置」得到的终端命令。

Mac 首次读取 Quick 登录时，进入「连接设置」点击「授权钥匙串」，完成系统授权。成功读取的会话会保留在当前进程内，连续发送、流式回复和后台状态检查直接复用，Quick 重建钥匙串条目不会清掉仍有效的会话。每次使用前核对当前账号资料；账号变更、移除资料或服务端返回 HTTP 401 时使缓存失效。令牌过期后才静默读取新凭据，无法读取时显示连接提示，不自动弹窗。本机登录模式的会话令牌不写入普通文件，退出应用后不保留；重启、更新开发签名或实际令牌到期后可能仍需授权，不承诺永久免授权。命令行可显式运行 `quickway doctor --authorize-keychain` 完成授权。

## 导出凭证到另一台机器

桌面版「连接设置 → 跨机器使用」支持 Windows / macOS 之间迁移 QuickWay 凭证，直接导出、直接导入，无需设置密码。两台机器都需要使用包含此功能的新版 QuickWay。

1. 在已登录的机器确认 QuickWay 已连接，点击「导出凭证」，保存为新的 `.qwc` 文件。Mac 尚未连接时先点击「授权钥匙串」。
2. 将文件带到另一台机器，打开 QuickWay 的「连接设置」，点击「导入凭证」并选择文件。该机器无需先登录 Quick，聊天、Claude Code 和本地 API 都会使用导入会话；模型目录会按该会话重新同步。
3. 重启 QuickWay 后会自动恢复导入的凭证。要切回本机 Quick 登录，点击「移除导入凭证」；原始导出文件不会被删除。

企业凭证只能使用原访问令牌的剩余有效期，不能在导入机器上自动续期；到期后需在原机器打开 Quick 续期，再重新导出和导入。社交登录携带刷新凭证，可以在服务端允许时自动续期；多台机器共用刷新令牌时，令牌轮换可能使其他副本失效。复制文件不会延长有效期，也不保证服务端允许跨设备使用；服务端拒绝时会直接报告错误。

`.qwc` 是包含账号令牌的明文 JSON 文件，请妥善保管，只在自己的设备间传输。导入副本保存在 QuickWay 配置目录的 `imported-credentials.qwc`，Mac 文件权限为 `0600`，Windows 继承当前用户应用目录的权限；社交令牌续期后原子更新该副本，原始导出文件不会跟着更新。Windows 凭据管理器、macOS 钥匙串及 Quick 的账号资料不会被改写，QuickWay 的本地代理仍只监听本机。独立 CLI 暂未提供导入入口，可以连接桌面版启动的本地代理。

## 支持的模型

模型列表完全来自当前 Quick 账号的云端 `config/all`，不按模型家族过滤，不补入内置型号。下表仅记录历史调用验证，不是当前支持清单；当前可选模型以界面和 `/v1/models` 为准。

| Claude Code 模型名 | Quick 上游模型 ID | 本机实测 |
| --- | --- | --- |
| `claude-sonnet-4-6` | `global.anthropic.claude-sonnet-4-6` | Claude Code Read 工具两轮成功 |
| `claude-opus-4-8` | `global.anthropic.claude-opus-4-8` | 文本、流式成功 |
| `claude-opus-4-6` | `us.anthropic.claude-opus-4-6-v1` | 文本、流式、工具往返成功 |
| `claude-haiku-4-5-20251001` | `global.anthropic.claude-haiku-4-5-20251001-v1:0` | 文本、流式成功 |
| `claude-sonnet-5-5` | `global.anthropic.claude-sonnet-5-5` | 文本、流式、自动工具往返成功 |
| `claude-sonnet-5` | `us.anthropic.claude-sonnet-5` | 文本、流式、自动工具往返成功 |
| `claude-opus-5-5` | `us.anthropic.claude-opus-5-5` | 文本、流式、自动工具往返成功 |
| `claude-haiku-5-5` | `us.anthropic.claude-haiku-5-5` | 文本、流式、自动工具往返成功 |
| `gpt-6-astra` | `us.openai.gpt-6-astra` | 文本、流式、工具往返成功 |
| `gpt-5.6-sol` | `us.openai.gpt-5.6-sol` | 文本、流式、工具往返成功 |
| `gpt-5.6-terra` | `us.openai.gpt-5.6-terra` | 文本、流式、工具往返成功 |
| `gpt-5.6-luna` | `us.openai.gpt-5.6-luna` | 文本、流式、工具往返成功 |

可用性取决于账号与 Quick 服务端，代理不会静默切换模型。Claude Code 显示的美元金额是其按模型价目表计算的估算；本代理实际调用 Quick 返回额度信息的接口。

模型目录从当前 Quick 登录的云端 `config/all` 动态读取。桌面聊天、Claude Code 启动器、请求解析和 `/v1/models` 共用同一目录。桌面启动时加载，运行期间每 5 分钟更新；命令行代理首次请求时加载，之后在请求时检查是否需要更新。聊天顶部或「连接设置 → 模型目录」可立即刷新。

刷新失败时保留同一账号上次成功同步的目录，明确显示刷新失败，1 分钟后自动重试；从未成功获取目录时列表为空。成功同步会完整替换旧目录，包括云端返回空列表的情况。移除的型号不再出现在可选列表中；历史对话保留原选择并提示不可用，要求重新选择，不自动换模型。

桌面版只把云端目录的模型名称和路由保存到应用配置目录 `models-cache.json`，按 Quick 账号资料隔离；缓存是上次同步结果，不是另一套备用列表。切换账号或移除账号资料时清除旧目录。CLI 使用进程内缓存。升级后会丢弃旧版曾过滤模型的缓存，重新同步完整目录。

Fable 5.1、Nova 以及新模型家族只要出现在 `values.models` 中就会展示，调用错误直接显示，不因尚未完成协议验证而隐藏。此前 Fable 5.1 的调用存在响应解析问题；展示它不代表该问题已解决。重复槽位引用同一模型 ID 时去重，不同区域或版本的路由全部保留；简短 ID 冲突时使用完整上游 ID，避免静默覆盖。

不再提供 `opus`、`sonnet`、`haiku` 到固定旧型号的别名。API、桌面聊天和启动器只接受当前目录列出的 ID 或对应的完整上游 ID。Claude Code 启动配置中的各模型角色均指向本次选定的云端模型，避免后台请求写死的旧型号。

## 连接方式

```text
QuickWay 聊天 / Claude Code → localhost /v1/messages → Quick Bedrock proxy → 所选模型
                  ↑                        ↑
         本地独立 API key             当前 Quick access_token
```

Anthropic 非流请求通常使用 `bedrock-proxy` 的 `INVOKE_MODEL`，返回原生 Anthropic Messages；Opus 5.5 和其他供应商的非流请求使用 `CONVERSE` 并转换为 Anthropic Messages。Opus 5.5 的 InvokeModel 路径在指定思考参数时返回不完整响应，因此统一使用 Converse。所有流式请求转换为 Converse，调用 `bedrock-proxy-stream`，逐帧校验 AWS EventStream 并转为 Anthropic SSE。工具定义、输入和结果由协议转换保留，工具由 Claude Code 执行。应用内聊天提供文本对话，不执行本机工具。

接口包括 `/v1/messages`、`/v1/messages/count_tokens`、`/v1/chat/completions`、`/v1/responses`、`/v1/models`、`/quickway/usage` 和 `/health`。除基础健康检查外，均要求 `x-api-key` 或 `Authorization: Bearer` 本地代理密钥。仅允许监听 loopback。

### OpenAI 兼容接口

`/v1/chat/completions` 和 `/v1/responses` 将 OpenAI 请求转换为 Anthropic Messages，再走与 `/v1/messages` 相同的模型解析、思考强度和上游路由，结果转换回 OpenAI 格式（含 SSE 流式）。OpenAI SDK 将 `base_url` 设为 `http://127.0.0.1:8741/v1`，`api_key` 使用本地代理密钥。`/v1/models` 同时返回 OpenAI 的 `object`/`created`/`owned_by` 字段。

- 支持文本、base64 `data:` 图片、system/developer、函数工具（`tool_choice`、`parallel_tool_calls`）、`stop`、`temperature`/`top_p`、`json_schema` 结构化输出及流式 usage。
- `reasoning_effort` 或 `reasoning.effort`：`none`/`minimal` 对应 Off，`xhigh` 对应 High；桌面启动器的 `x-quickway-thinking-effort` 请求头优先。Chat Completions 以 `reasoning_content` 返回思考文本。
- 未指定输出上限时默认 `max_tokens` 为 32000。
- Responses 为无状态：不存储响应，`previous_response_id`、`conversation`、`item_reference` 会被拒绝，请每次发送完整 `input`。Claude 的签名思考放在 reasoning 项的 `encrypted_content`，原样回传即可在工具循环中续用；GPT 推理没有签名，不回放。
- Responses 还接受 `namespace` 工具组和 `custom`（自由文本）工具。上游只支持普通函数工具，因此命名空间内的函数展开为 `命名空间__函数名`，custom 工具转为带单个 `input` 字符串参数的函数并在说明中附上语法；返回时还原为原来的 `function_call`（带 `namespace`）或 `custom_tool_call`。`web_search`、`image_generation` 由 OpenAI 服务端执行，QuickWay 无法提供，请求中出现时直接忽略，模型看不到这些工具。
- 不支持并会返回 400 的选项：`n>1`、`logprobs`、`logit_bias`、非零 `frequency_penalty`/`presence_penalty`、音频、远程图片 URL、`json_object`、其他托管工具（file_search、code_interpreter 等），以及已有工具调用历史时的 `tool_choice=none`。
- 实测：Sonnet 4.6、Opus 5.5、Haiku 4.5、GPT-5.6 Sol 在 Chat Completions 与 Responses 下的非流式、流式、工具往返均通过（含开启思考）。实时检查：`python scripts/smoke-openai.py --model claude-sonnet-4-6 --effort high`（需 `pip install openai`）。

手动连接 Claude Code（`--model` 请使用当前 `/v1/models` 返回的 ID）：

```powershell
$env:ANTHROPIC_BASE_URL = 'http://127.0.0.1:8741'
$env:ANTHROPIC_AUTH_TOKEN = (Get-Content -Raw .quickway\proxy.key).Trim()
$env:ANTHROPIC_API_KEY = $env:ANTHROPIC_AUTH_TOKEN
$env:CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS = '1'
$env:ENABLE_TOOL_SEARCH = 'false'
$connection = Join-Path $PWD '.quickway\claude-connection.json'
@{ env = @{ ANTHROPIC_BASE_URL = $env:ANTHROPIC_BASE_URL; ANTHROPIC_AUTH_TOKEN = $env:ANTHROPIC_AUTH_TOKEN; ANTHROPIC_API_KEY = $env:ANTHROPIC_API_KEY; CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS = '1'; ENABLE_TOOL_SEARCH = 'false' } } | ConvertTo-Json | Set-Content -LiteralPath $connection -Encoding UTF8
claude --settings $connection --model claude-sonnet-4-6
```

### 连接 Codex CLI

Codex 通过 `/v1/responses` 流式接口工作，Claude 和 GPT 模型都可以使用。

桌面版「Codex」页面在 PowerShell（Mac 为终端）中启动 Codex，只用本次命令行的 `-c` 覆盖项接入 QuickWay：提供方、本机地址、`env_key = "QUICKWAY_API_KEY"`、`x-quickway-thinking-effort` 请求头及对应的 `model_reasoning_effort`。不修改 `~/.codex/config.toml`，你的登录、MCP、插件和历史会话照常加载；密钥只放在这个终端的环境变量中。每次启动前，QuickWay 运行 `codex debug models --bundled` 读取本机 Codex 自带的模型目录，GPT 模型保留原条目，Codex 没有的模型（Claude 等）复制第一个可见条目并改为对应名称和 200K 上下文，写入配置目录的 `codex-models.json`，通过 `model_catalog_json` 传入。这样 Claude 也能用到 `apply_patch` 等完整工具，不会出现 `Model metadata ... not found`。读取失败时不传目录，Codex 使用回退设置继续启动。

在自己的终端使用时，可在页面上复制提供方配置，或在 `~/.codex/config.toml`（或单独的 `CODEX_HOME`）中手动添加，密钥从环境变量读取，不写入配置文件：

```toml
model_provider = "quickway"
model = "claude-sonnet-5-5"        # 使用当前 /v1/models 返回的 ID
model_reasoning_effort = "medium"  # none/minimal=Off，low，medium，high/xhigh/max/ultra=High

[model_providers.quickway]
name = "QuickWay"
base_url = "http://127.0.0.1:8741/v1"
wire_api = "responses"
env_key = "QUICKWAY_API_KEY"
```

```powershell
$env:QUICKWAY_API_KEY = (Get-Content -Raw .quickway\proxy.key).Trim()
codex
```

- Codex 每轮发送完整历史并设置 `store: false`，与 QuickWay 的无状态 Responses 一致；Claude 的签名思考随 reasoning 项回传，多轮工具循环可继续使用。
- Codex 的命令执行、`apply_patch`（custom 工具）、子代理等命名空间工具都在本机由 Codex 执行；`web_search` 会被忽略。默认设置下 Codex 把工具放在 `additional_tools` 输入项中发送，QuickWay 同样读取；顶层 `functions` 命名空间按普通工具处理，不加前缀。
- Codex 默认发送 `parallel_tool_calls: false`。上游 Converse 无法强制单个工具调用，Responses 接口把它视为提示而不报错；Codex 会依次执行返回的每个调用。
- MCP 工具的参数结构常带顶层 `oneOf`/`anyOf`/`allOf`，上游会因此让整个请求失败。QuickWay 转发前移除这些顶层组合，`properties` 中的字段说明保持不变。无参数工具的调用即使没有输入内容也视为有效。
- Codex 没有内置 Claude 的模型元数据时会提示 `Model metadata ... not found` 并使用回退设置，此时只提供 shell 工具、不提供 `apply_patch`。需要 `apply_patch` 时，可在 `model_catalog_json` 指向的目录文件中为该 Claude 模型添加一项，设置 `"apply_patch_tool_type": "freeform"` 和实际的 `context_window`。
- 实测（Codex CLI 0.167，Windows）：`codex exec` 分别用 Claude Sonnet 5.5、Claude Haiku 4.5、GPT-6 Astra、GPT-5.6 Sol、GPT-5.6 Terra 和 GPT-5.6 Luna 从空目录创建带 pytest 测试的 Python 项目，命令执行、custom 工具写文件和多轮工具循环均正常，测试全部通过；Haiku 4.5 开启思考时签名 reasoning 项在后续轮次回传并被上游接受。桌面版的 Windows 启动命令（含中文和空格路径、生成的模型目录、本机 `~/.codex` 中的 MCP 工具）以 `codex exec` 方式实测，Claude Sonnet 5.5 和 GPT-5.6 Sol 均写出并运行了代码：`QUICKWAY_TEST_PORT=8741 QUICKWAY_TEST_KEY=... cargo test live_codex -- --ignored`（在 `desktop/src-tauri` 中运行）。Mac 启动脚本尚未在 Mac 上实测。

## 排查问题

登录、额度或模型请求报错时，在桌面版「连接设置」的「诊断报告」中点击「复制诊断报告」。额度读取失败时，提示旁边也有同一个按钮。报告复制到剪贴板后，可以直接粘贴发给支持人员。命令行版运行 `quickway doctor --online` 会输出同样的报告。

报告内容：
- Quick 登录资料：登录通道、字段名，以及 Quick 自身续期失败了多久。
- 凭据：字段名、各有效期距现在的秒数，以及看起来是毫秒的值。
- 令牌：长度、JWT 的 `iss`/`aud`/`token_use`/`scope` 等声明和到期时间，以及实际发送的是哪个令牌。
- `user-info` 和模型目录请求：状态码、请求 ID、错误类型，以及最多 800 字的错误正文。
- 用 Quick 存储的另一个令牌重试 `user-info` 的结果（仅本机登录）。
- 最近 20 次失败的上游请求。

报告不包含令牌和密钥的值，不包含邮箱、用户名、姓名或对话内容。租户主机名、账号 ID 和 IdC 实例路径只保留开头或末尾几位。生成报告时会向 Quick 发送只读请求，不写入 Quick 的凭据，也不会因为探测失败把当前凭据标记为已拒绝。

## 范围与限制

- 支持 Windows 和 macOS 上的生产 App Integrations 社交及企业登录；Gateway 传输仍未适配。企业登录由 Amazon Quick 自身续期，需保持 Quick 登录可用。企业凭据带有 `id_token` 时发送 `id_token` 并按它的有效期判断，与 Quick 一致；没有时才发送 access token。
- Sonnet 5.5 仅支持自动工具选择，强制 `tool_choice=tool/any` 会被上游拒绝。Nova 语音尚未可用，不列入桌面版。
- GPT 将自适应推理转换为 `reasoning.effort`，关闭思考对应 `none`；不支持 Anthropic 固定 thinking token 预算。GPT 工具结果后不支持的缓存提示会被移除，工具内容保持原样。
- 普通工具、图片、system、缓存标记、带签名 thinking、多轮工具结果已实现。模型/服务端仍可拒绝某些参数。服务端工具、延迟工具搜索和强制禁止并行工具调用尚未适配，启动脚本关闭相关实验 beta 与 tool search。
- `count_tokens` 是包含 system、消息、工具及结果的保守估算，响应带 `x-quickway-token-count: estimated`；生成响应的 usage 来自实际上游。
- 本机社交登录令牌过期时通过 IDC OIDC 刷新；刷新结果只保留内存，不覆盖 Quick 的凭据。若服务端轮换令牌且代理重启，请重新打开 Quick 续期。Quick 注销/重登录会使代理缓存失效。
- HTTP 额度/权限错误保留相应状态，断流、CRC 错误及缺少终止事件会返回错误。`max_tokens` 导致工具 JSON 截断时保留真实停止原因，以便 Claude Code 恢复。

## 构建与测试

构建 Windows 程序需要 Rust、MSVC C++ Build Tools 和 Node.js 20.19+ 或 22.12+。在项目根目录运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\Build-Windows.ps1
```

脚本会重新安装当前平台的前端依赖，运行核心及桌面 Rust 测试、Clippy、Chromium 界面回归，然后生成 Windows x64 程序和 NSIS 安装包。全部测试和构建成功后，备份旧交付文件，更新 `release/` 下的程序及 SHA-256 校验；命令行启动脚本使用的 `target/release/quickway.exe` 也会更新。默认从公开 npm 源安装锁定的依赖，可用 `-NpmRegistry` 指定其他源。

Windows 原始构建输出位于 `desktop/src-tauri/target/x86_64-pc-windows-msvc/release`。仅手动打包可在 `desktop` 下执行 `npm ci` 和 `npm run desktop:build:windows`；前端本地开发使用 `npm run tauri dev`。

```powershell
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
py -3.12 scripts/smoke-test.py --tools
```

`smoke-test.py` 是显式的真实接口测试，会消耗少量 Quick 额度。普通单元测试使用合成数据和本地 mock，不访问真实账号。

协议适配参考了 [kiro.rs](https://github.com/hank9999/kiro.rs) 的设计，当前实现根据 Quick 自身接口独立编写，没有复制其上游端点或客户端代码。

浏览器测试明确使用模拟 Tauri IPC，分别运行 WebKit 和 Chromium；原生钥匙串、实际模型请求与 Terminal 启动需要单独验证。

```sh
cargo test --locked
cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked
cargo clippy --all-targets --locked -- -D warnings
cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cd desktop
npx playwright install webkit chromium
npm run test:ui
```

原生钥匙串测试显式执行 `cargo test --locked macos_keychain_roundtrip_and_chunk_layout -- --ignored`，仅创建并清理唯一命名的合成测试条目，不修改 Quick 登录。
