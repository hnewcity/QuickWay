<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Cổng kết nối của bạn tới Amazon Quick</h3>
<p align="center">Dùng các mô hình của Quick trong Claude Code, Codex và mọi ứng dụng tương thích OpenAI hoặc Anthropic thông qua API cục bộ</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • <a href="README_ja.md">🇯🇵 日本語</a> • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • <a href="README_pt.md">🇧🇷 Português</a> • <a href="README_id.md">🇮🇩 Indonesia</a> • 🇻🇳 Tiếng Việt
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

## ❤️ Nhà tài trợ

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td>Cảm ơn <a href="https://ai18n.chat">ai18n</a> đã tài trợ dự án này! ai18n là nhà cung cấp dịch vụ relay API đáng tin cậy và hiệu quả, cung cấp relay các mô hình Claude cho OpenClaw, Claude Code, Codex, Gemini và nhiều hơn nữa.</td>
</tr>
</table>

---

QuickWay là cổng kết nối trên máy tính, mở các mô hình của tài khoản Amazon Quick đang đăng nhập thông qua API cục bộ tương thích Anthropic và OpenAI. Dùng Quick với Claude Code, Codex cùng các SDK và công cụ bạn yêu thích mà không cần sửa mã.

## ✨ Tính năng

| Tính năng | Mô tả |
|---|---|
| 🔌 **API tương thích Anthropic** | Endpoint `/v1/messages` cho Anthropic SDK và Claude Code |
| 🔌 **API tương thích OpenAI** | Endpoint `/v1/chat/completions` và `/v1/responses` cho OpenAI SDK và Codex |
| 🚀 **Trình khởi chạy lập trình** | Mở Claude Code hoặc Codex trong terminal riêng chỉ với một cú nhấp, mỗi công cụ nhớ mô hình và mức riêng |
| 🧠 **Mức suy luận** | Tắt / Thấp / Trung bình / Cao, ánh xạ tới điều khiển suy luận gốc của từng mô hình |
| 📋 **Danh mục mô hình động** | Mô hình được đọc từ tài khoản Quick của bạn và tự động làm mới |
| 🛠️ **Gọi công cụ** | Gọi hàm cho Messages, Chat Completions và Responses |
| 👁️ **Hỗ trợ hình ảnh** | Gửi ảnh base64 tới mô hình |
| 📡 **Streaming** | Hỗ trợ đầy đủ streaming SSE |
| 💬 **Trò chuyện tích hợp** | Kiểm tra cấu hình bằng giao diện trò chuyện tích hợp |
| 🔐 **Chạy cục bộ, thông tin xác thực an toàn** | Proxy chỉ lắng nghe trên loopback với khóa API riêng; token chỉ nằm trong bộ nhớ và lấy từ Windows Credential Manager hoặc Keychain của macOS |
| 🔄 **Chuyển thông tin xác thực** | Xuất và nhập thông tin xác thực giữa các máy Windows và macOS của chính bạn |
| 🔗 **Tích hợp CC Switch** | Nhập một chạm vào [CC Switch](https://github.com/farion1231/cc-switch) cho Claude Code |
| 🌍 **Giao diện đa ngôn ngữ** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **Chẩn đoán** | Sao chép báo cáo chẩn đoán đã che dữ liệu nhạy cảm khi kiểm tra kết nối hoặc mức dùng thất bại |

## 📦 Cài đặt

### Tải xuống

Tải bản phát hành mới nhất tại [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| Nền tảng | Kiến trúc | Tải xuống |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Trình cài đặt x64 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Bản di động x64 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> Yêu cầu Windows 10/11 x64 hoặc macOS 12 trở lên. Trình cài đặt Windows chưa được ký mã, còn bản macOS dùng chữ ký ad-hoc và chưa công chứng, nên SmartScreen hoặc Gatekeeper có thể yêu cầu xác nhận ở lần mở đầu tiên. Hãy đăng nhập Amazon Quick trên cùng máy trước.

### Build từ mã nguồn

```bash
# Clone kho mã
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# Cài đặt phụ thuộc
npm ci

# Chạy ở chế độ phát triển
npm run tauri dev

# Hoặc build bản production
npm run desktop:build
```

Cần Rust (xem `rust-toolchain.toml`) và Node.js 20.19+ hoặc 22.12+. Trên macOS cần cài thêm Xcode Command Line Tools.

## 🚀 Bắt đầu nhanh

1. **Đăng nhập** Amazon Quick trên máy này
2. **Mở** QuickWay - ứng dụng tự nhận tài khoản đang đăng nhập
3. **Vào** tab Lập trình, chọn Claude Code hoặc Codex, một mô hình và thư mục dự án rồi bấm Khởi chạy
4. **Hoặc** dùng `http://127.0.0.1:8741` làm endpoint API Anthropic/OpenAI, với khóa API lấy ở phần cài đặt kết nối

> ID mô hình phụ thuộc vào tài khoản của bạn. Hãy dùng ID do `GET /v1/models` trả về.

### Ví dụ: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Ví dụ: Python (OpenAI SDK)

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

### Ví dụ: JavaScript (OpenAI SDK)

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

## 🔌 Tương thích với

| Nhóm | Công cụ |
|---|---|
| **Tác tử lập trình** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **Công cụ** | CC Switch, Cursor, Continue, Cline |

## ⚙️ Cấu hình

### Phương thức xác thực

| Phương thức | Mô tả |
|---|---|
| **Đăng nhập Quick cục bộ** | Đọc tài khoản Amazon Quick đang đăng nhập (Windows Credential Manager hoặc Keychain macOS) - khuyên dùng |
| **Thông tin xác thực đã nhập** | Tệp `.qwc` xuất từ một máy khác của chính bạn |

### Cài đặt máy chủ

| Tùy chọn | Mặc định | Mô tả |
|---|---|---|
| Host | `127.0.0.1` | Chỉ loopback; proxy không bao giờ lắng nghe trên giao diện khác |
| Cổng | `8741` | Cổng máy chủ, có thể đổi trong cài đặt kết nối |
| Khóa API của proxy | tự động tạo | Bắt buộc với mọi yêu cầu: `x-api-key` hoặc `Authorization: Bearer` |

### Cài đặt nâng cao

| Tùy chọn | Mô tả |
|---|---|
| Mức suy luận | Tắt / Thấp / Trung bình / Cao theo từng cuộc trò chuyện hoặc lần khởi chạy, hoặc qua header `x-quickway-thinking-effort` |
| Danh mục mô hình | Làm mới mỗi 5 phút, hoặc thủ công từ thanh công cụ trò chuyện |
| Khởi động cùng ứng dụng | Tự khởi động proxy khi mở QuickWay |

## 🛠️ Công nghệ

| Tầng | Công nghệ |
|---|---|
| **Frontend** | TypeScript + Vite |
| **Desktop** | Tauri 2 (Rust) |
| **Backend** | Rust + axum |

## 🤝 Đóng góp

Rất hoan nghênh mọi đóng góp! Bạn có thể:

- 🐛 Báo lỗi qua [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 Đề xuất tính năng
- 🔧 Gửi pull request
- 🌍 Giúp dịch thuật

## 📚 Tài liệu

- [User guide (中文)](USER_GUIDE_zh.md): hướng dẫn chi tiết về thông tin xác thực, mô hình, API tương thích OpenAI và Codex
- [Quy trình phát hành](RELEASING.md): cách nâng phiên bản và xuất bản bản phát hành

## 📄 Giấy phép

[MIT](../LICENSE) © QuickWay Contributors
