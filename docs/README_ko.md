<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Amazon Quick으로 가는 게이트웨이</h3>
<p align="center">로컬 API를 통해 Claude Code, Codex 및 OpenAI / Anthropic 호환 앱에서 Quick의 모델을 사용하세요</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • <a href="README_ja.md">🇯🇵 日本語</a> • 🇰🇷 한국어 • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • <a href="README_pt.md">🇧🇷 Português</a> • <a href="README_id.md">🇮🇩 Indonesia</a> • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
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

## ❤️ 스폰서

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td><a href="https://ai18n.chat">ai18n</a>의 후원에 감사드립니다! ai18n은 신뢰할 수 있고 효율적인 API 릴레이 서비스 제공업체로, OpenClaw, Claude Code, Codex, Gemini 등을 위한 Claude 모델 릴레이를 제공합니다.</td>
</tr>
</table>

---

QuickWay는 로그인된 Amazon Quick 계정의 모델을 로컬 Anthropic 호환 및 OpenAI 호환 API로 제공하는 데스크톱 게이트웨이입니다. Claude Code, Codex, 선호하는 SDK와 도구에서 코드 변경 없이 Quick을 사용할 수 있습니다.

## ✨ 기능

| 기능 | 설명 |
|---|---|
| 🔌 **Anthropic 호환 API** | Anthropic SDK와 Claude Code를 위한 `/v1/messages` 엔드포인트 |
| 🔌 **OpenAI 호환 API** | OpenAI SDK와 Codex를 위한 `/v1/chat/completions` 및 `/v1/responses` 엔드포인트 |
| 🚀 **코딩 런처** | Claude Code 또는 Codex를 별도 터미널에서 한 번에 실행하며, 모델과 강도를 각각 기억 |
| 🧠 **생각 강도** | 끔 / 낮음 / 보통 / 높음, 각 모델의 기본 추론 설정에 대응 |
| 📋 **동적 모델 카탈로그** | 모델은 Quick 계정에서 읽어 오며 자동으로 갱신 |
| 🛠️ **도구 호출** | Messages, Chat Completions, Responses에서 함수 호출 지원 |
| 👁️ **비전 지원** | base64 이미지를 모델에 전송 |
| 📡 **스트리밍** | 완전한 SSE 스트리밍 지원 |
| 💬 **내장 채팅** | 통합 채팅 화면으로 설정을 테스트 |
| 🔐 **로컬 실행과 안전한 자격 증명** | 루프백 전용 프록시와 별도 API 키. 토큰은 메모리에만 있으며 Windows 자격 증명 관리자 또는 macOS 키체인에서 가져옴 |
| 🔄 **자격 증명 이전** | 본인의 Windows 및 macOS 기기 간에 자격 증명을 내보내고 가져오기 |
| 🔗 **CC Switch 연동** | Claude Code용으로 [CC Switch](https://github.com/farion1231/cc-switch)에 원클릭으로 가져오기 |
| 🌍 **다국어 UI** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **진단 보고서** | 연결 또는 사용량 확인이 실패하면 민감 정보가 가려진 진단 보고서를 복사 |

## 📦 설치

### 다운로드

최신 릴리스는 다음에서 받을 수 있습니다: [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| 플랫폼 | 아키텍처 | 다운로드 |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 설치 프로그램 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | x64 포터블 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> Windows 10/11 x64 또는 macOS 12 이상이 필요합니다. Windows 설치 프로그램은 코드 서명이 없고 macOS 빌드는 공증 없이 ad-hoc 서명되어 있어, 처음 실행할 때 SmartScreen 또는 Gatekeeper가 확인을 요청할 수 있습니다. 먼저 같은 컴퓨터에서 Amazon Quick에 로그인하세요.

### 소스에서 빌드

```bash
# 저장소 클론
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# 의존성 설치
npm ci

# 개발 모드로 실행
npm run tauri dev

# 또는 프로덕션 빌드
npm run desktop:build
```

Rust(버전은 `rust-toolchain.toml` 참고)와 Node.js 20.19+ 또는 22.12+가 필요합니다. macOS에서는 Xcode Command Line Tools도 설치하세요.

## 🚀 빠른 시작

1. 이 컴퓨터에서 Amazon Quick에 **로그인**
2. QuickWay를 **실행** - 로그인된 계정을 자동으로 인식합니다
3. 「코딩」 탭을 **열고** Claude Code 또는 Codex, 모델, 프로젝트 폴더를 선택한 뒤 실행
4. **또는** `http://127.0.0.1:8741`을 Anthropic / OpenAI API 엔드포인트로 사용(API 키는 「연결 설정」에서 확인)

> 모델 ID는 계정에 따라 다릅니다. `GET /v1/models`가 반환하는 ID를 사용하세요.

### 예시: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 예시: Python (OpenAI SDK)

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

### 예시: JavaScript (OpenAI SDK)

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

## 🔌 호환 도구

| 분류 | 도구 |
|---|---|
| **코딩 에이전트** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **도구** | CC Switch, Cursor, Continue, Cline |

## ⚙️ 설정

### 인증 방식

| 방식 | 설명 |
|---|---|
| **로컬 Quick 로그인** | 로그인된 Amazon Quick 계정(Windows 자격 증명 관리자 또는 macOS 키체인)을 읽습니다(권장) |
| **가져온 자격 증명** | 본인의 다른 기기에서 내보낸 `.qwc` 파일 |

### 서버 설정

| 옵션 | 기본값 | 설명 |
|---|---|---|
| 호스트 | `127.0.0.1` | 루프백 전용이며 프록시는 다른 인터페이스에서 수신하지 않습니다 |
| 포트 | `8741` | 서버 포트로, 「연결 설정」에서 변경할 수 있습니다 |
| 프록시 API 키 | 자동 생성 | 모든 요청에 필요: `x-api-key` 또는 `Authorization: Bearer` |

### 고급 설정

| 옵션 | 설명 |
|---|---|
| 생각 강도 | 대화 또는 실행마다 끔 / 낮음 / 보통 / 높음을 선택하거나 `x-quickway-thinking-effort` 요청 헤더로 지정 |
| 모델 카탈로그 | 5분마다 갱신되며 채팅 도구 모음에서 수동으로도 갱신 가능 |
| 앱과 함께 시작 | QuickWay를 열 때 프록시를 자동으로 시작 |

## 🛠️ 기술 스택

| 계층 | 기술 |
|---|---|
| **프런트엔드** | TypeScript + Vite |
| **데스크톱** | Tauri 2 (Rust) |
| **백엔드** | Rust + axum |

## 🤝 기여

기여를 환영합니다! 다음과 같은 방법으로 참여할 수 있습니다:

- 🐛 버그 신고: [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 기능 제안
- 🔧 풀 리퀘스트 제출
- 🌍 번역 돕기

## 📚 문서

- [User guide (中文)](USER_GUIDE_zh.md): 자격 증명, 모델, OpenAI 호환 API, Codex를 다룬 상세 매뉴얼
- [릴리스 절차](RELEASING.md): 버전 갱신과 릴리스 자동 게시 방식

## 📄 라이선스

[MIT](../LICENSE) © QuickWay Contributors
