<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Gerbang Anda ke Amazon Quick</h3>
<p align="center">Gunakan model Quick di Claude Code, Codex, dan aplikasi apa pun yang kompatibel dengan OpenAI atau Anthropic melalui API lokal</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • <a href="README_ja.md">🇯🇵 日本語</a> • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • <a href="README_pt.md">🇧🇷 Português</a> • 🇮🇩 Indonesia • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
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

## ❤️ Sponsor

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td>Terima kasih kepada <a href="https://ai18n.chat">ai18n</a> atas sponsornya! ai18n adalah penyedia layanan relay API yang andal dan efisien, menawarkan relay model Claude untuk OpenClaw, Claude Code, Codex, Gemini, dan lainnya.</td>
</tr>
</table>

---

QuickWay adalah gateway desktop yang membuka model dari akun Amazon Quick Anda yang sudah masuk melalui API lokal yang kompatibel dengan Anthropic dan OpenAI. Gunakan Quick dengan Claude Code, Codex, serta SDK dan alat favorit Anda tanpa mengubah kode.

## ✨ Fitur

| Fitur | Deskripsi |
|---|---|
| 🔌 **API kompatibel Anthropic** | Endpoint `/v1/messages` untuk Anthropic SDK dan Claude Code |
| 🔌 **API kompatibel OpenAI** | Endpoint `/v1/chat/completions` dan `/v1/responses` untuk OpenAI SDK dan Codex |
| 🚀 **Peluncur coding** | Jalankan Claude Code atau Codex di terminal sendiri dengan satu klik, masing-masing dengan model dan tingkatnya sendiri |
| 🧠 **Tingkat berpikir** | Mati / Rendah / Sedang / Tinggi, dipetakan ke kontrol penalaran bawaan tiap model |
| 📋 **Katalog model dinamis** | Model dibaca dari akun Quick Anda dan diperbarui otomatis |
| 🛠️ **Pemanggilan alat** | Pemanggilan fungsi untuk Messages, Chat Completions, dan Responses |
| 👁️ **Dukungan visi** | Kirim gambar base64 ke model |
| 📡 **Streaming** | Dukungan streaming SSE lengkap |
| 💬 **Obrolan bawaan** | Uji konfigurasi Anda dengan antarmuka obrolan terintegrasi |
| 🔐 **Lokal dan kredensial aman** | Proxy hanya di loopback dengan kunci API sendiri; token hanya ada di memori dan berasal dari Windows Credential Manager atau Keychain macOS |
| 🔄 **Transfer kredensial** | Ekspor dan impor kredensial antar komputer Windows dan macOS milik Anda sendiri |
| 🔗 **Integrasi CC Switch** | Impor satu klik ke [CC Switch](https://github.com/farion1231/cc-switch) untuk Claude Code |
| 🌍 **Antarmuka multibahasa** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **Diagnostik** | Salin laporan diagnostik yang datanya sudah disamarkan saat pemeriksaan koneksi atau penggunaan gagal |

## 📦 Instalasi

### Unduh

Unduh rilis terbaru dari [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| Platform | Arsitektur | Unduh |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Installer x64 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Portabel x64 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> Membutuhkan Windows 10/11 x64 atau macOS 12+. Installer Windows tidak ditandatangani dan build macOS memakai tanda tangan ad-hoc tanpa notarisasi, sehingga SmartScreen atau Gatekeeper mungkin meminta konfirmasi saat pertama kali dibuka. Masuk dulu ke Amazon Quick di komputer yang sama.

### Build dari sumber

```bash
# Clone repositori
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# Instal dependensi
npm ci

# Jalankan dalam mode pengembangan
npm run tauri dev

# Atau build untuk produksi
npm run desktop:build
```

Membutuhkan Rust (lihat `rust-toolchain.toml`) dan Node.js 20.19+ atau 22.12+. Di macOS, instal juga Xcode Command Line Tools.

## 🚀 Mulai cepat

1. **Masuk** ke Amazon Quick di komputer ini
2. **Buka** QuickWay - akun yang sudah masuk terdeteksi otomatis
3. **Buka** tab Coding, pilih Claude Code atau Codex, model, dan folder proyek, lalu tekan Luncurkan
4. **Atau** gunakan `http://127.0.0.1:8741` sebagai endpoint API Anthropic/OpenAI, dengan kunci API dari pengaturan koneksi

> ID model bergantung pada akun Anda. Gunakan ID yang dikembalikan oleh `GET /v1/models`.

### Contoh: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Contoh: Python (OpenAI SDK)

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

### Contoh: JavaScript (OpenAI SDK)

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

## 🔌 Kompatibel dengan

| Kategori | Alat |
|---|---|
| **Agen coding** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **Alat** | CC Switch, Cursor, Continue, Cline |

## ⚙️ Konfigurasi

### Metode autentikasi

| Metode | Deskripsi |
|---|---|
| **Login Quick lokal** | Membaca akun Amazon Quick yang sedang masuk (Windows Credential Manager atau Keychain macOS) - disarankan |
| **Kredensial impor** | File `.qwc` yang diekspor dari komputer lain milik Anda |

### Pengaturan server

| Opsi | Default | Deskripsi |
|---|---|---|
| Host | `127.0.0.1` | Hanya loopback; proxy tidak pernah mendengarkan di antarmuka lain |
| Port | `8741` | Port server, dapat diubah di pengaturan koneksi |
| Kunci API proxy | dibuat otomatis | Wajib di setiap permintaan: `x-api-key` atau `Authorization: Bearer` |

### Pengaturan lanjutan

| Opsi | Deskripsi |
|---|---|
| Tingkat berpikir | Mati / Rendah / Sedang / Tinggi per obrolan atau peluncuran, atau lewat header permintaan `x-quickway-thinking-effort` |
| Katalog model | Diperbarui setiap 5 menit, atau manual dari bilah alat obrolan |
| Mulai bersama aplikasi | Jalankan proxy otomatis saat QuickWay dibuka |

## 🛠️ Tech Stack

| Lapisan | Teknologi |
|---|---|
| **Frontend** | TypeScript + Vite |
| **Desktop** | Tauri 2 (Rust) |
| **Backend** | Rust + axum |

## 🤝 Kontribusi

Kontribusi sangat disambut! Anda dapat:

- 🐛 Melaporkan bug lewat [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 Mengusulkan fitur
- 🔧 Mengirim pull request
- 🌍 Membantu terjemahan

## 📚 Dokumentasi

- [User guide (中文)](USER_GUIDE_zh.md): panduan lengkap tentang kredensial, model, API kompatibel OpenAI, dan Codex
- [Proses rilis](RELEASING.md): cara versi dinaikkan dan rilis dipublikasikan

## 📄 Lisensi

[MIT](../LICENSE) © QuickWay Contributors
