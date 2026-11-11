<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Seu portal para o Amazon Quick</h3>
<p align="center">Use os modelos do Quick no Claude Code, no Codex e em qualquer app compatível com OpenAI ou Anthropic por meio de uma API local</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • <a href="README_ja.md">🇯🇵 日本語</a> • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • <a href="README_es.md">🇪🇸 Español</a> • 🇧🇷 Português • <a href="README_id.md">🇮🇩 Indonesia</a> • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
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

## ❤️ Patrocinador

<table>
<tr>
<td width="180"><a href="https://ai18n.chat"><img src="sponsors/ai18n.png" alt="ai18n" width="150"></a></td>
<td>Obrigado ao <a href="https://ai18n.chat">ai18n</a> por patrocinar este projeto! ai18n é um provedor de serviços de relay de API confiável e eficiente, oferecendo relay de modelos Claude para OpenClaw, Claude Code, Codex, Gemini e muito mais.</td>
</tr>
</table>

---

O QuickWay é um gateway de desktop que expõe os modelos da sua conta do Amazon Quick conectada por meio de uma API local compatível com Anthropic e OpenAI. Use o Quick com Claude Code, Codex e seus SDKs e ferramentas favoritos, sem alterar código.

## ✨ Funcionalidades

| Funcionalidade | Descrição |
|---|---|
| 🔌 **API compatível com Anthropic** | Endpoint `/v1/messages` para o SDK da Anthropic e o Claude Code |
| 🔌 **API compatível com OpenAI** | Endpoints `/v1/chat/completions` e `/v1/responses` para o SDK da OpenAI e o Codex |
| 🚀 **Lançador de programação** | Inicie o Claude Code ou o Codex em um terminal próprio com um clique, cada um com seu modelo e nível |
| 🧠 **Nível de raciocínio** | Desligado / Baixo / Médio / Alto, mapeado para os controles de raciocínio nativos de cada modelo |
| 📋 **Catálogo dinâmico de modelos** | Os modelos são lidos da sua conta do Quick e atualizados automaticamente |
| 🛠️ **Chamada de ferramentas** | Chamada de funções em Messages, Chat Completions e Responses |
| 👁️ **Suporte a visão** | Envie imagens em base64 ao modelo |
| 📡 **Streaming** | Suporte completo a streaming SSE |
| 💬 **Chat integrado** | Teste sua configuração com a interface de chat integrada |
| 🔐 **Local e credenciais seguras** | Proxy apenas em loopback com chave de API própria; os tokens ficam só na memória e vêm do Gerenciador de Credenciais do Windows ou do Chaveiro do macOS |
| 🔄 **Transferência de credenciais** | Exporte e importe credenciais entre os seus próprios computadores Windows e macOS |
| 🔗 **Integração com o CC Switch** | Importação com um clique para o [CC Switch](https://github.com/farion1231/cc-switch) no Claude Code |
| 🌍 **Interface multilíngue** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **Diagnóstico** | Copie um relatório de diagnóstico com dados ocultos quando a conexão ou a consulta de uso falhar |

## 📦 Instalação

### Download

Baixe a versão mais recente em [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| Plataforma | Arquitetura | Download |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Instalador x64 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Portátil x64 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |

> Requer Windows 10/11 x64 ou macOS 12+. O instalador do Windows não é assinado e as versões do macOS usam assinatura ad-hoc sem notarização, então o SmartScreen ou o Gatekeeper podem pedir confirmação na primeira abertura. Antes, entre no Amazon Quick no mesmo computador.

### Compilar do código fonte

```bash
# Clonar o repositório
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# Instalar dependências
npm ci

# Executar em modo de desenvolvimento
npm run tauri dev

# Ou compilar para produção
npm run desktop:build
```

Requer Rust (veja `rust-toolchain.toml`) e Node.js 20.19+ ou 22.12+. No macOS, instale também as Xcode Command Line Tools.

## 🚀 Início rápido

1. **Entre** no Amazon Quick neste computador
2. **Abra** o QuickWay - ele detecta automaticamente a conta conectada
3. **Acesse** a aba Programação, escolha Claude Code ou Codex, um modelo e uma pasta de projeto, e clique em Iniciar
4. **Ou** use `http://127.0.0.1:8741` como endpoint da API Anthropic/OpenAI, com a chave de API das configurações de conexão

> Os IDs de modelo dependem da sua conta. Use um ID retornado por `GET /v1/models`.

### Exemplo: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Exemplo: Python (OpenAI SDK)

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

### Exemplo: JavaScript (OpenAI SDK)

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

## 🔌 Compatível com

| Categoria | Ferramentas |
|---|---|
| **Agentes de programação** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **Ferramentas** | CC Switch, Cursor, Continue, Cline |

## ⚙️ Configuração

### Métodos de autenticação

| Método | Descrição |
|---|---|
| **Login local do Quick** | Lê a conta do Amazon Quick conectada (Gerenciador de Credenciais do Windows ou Chaveiro do macOS) - recomendado |
| **Credenciais importadas** | Um arquivo `.qwc` exportado de outro computador seu |

### Configurações do servidor

| Opção | Padrão | Descrição |
|---|---|---|
| Host | `127.0.0.1` | Somente loopback; o proxy nunca escuta em outras interfaces |
| Porta | `8741` | Porta do servidor, alterável nas configurações de conexão |
| Chave de API do proxy | gerada automaticamente | Obrigatória em toda requisição: `x-api-key` ou `Authorization: Bearer` |

### Configurações avançadas

| Opção | Descrição |
|---|---|
| Nível de raciocínio | Desligado / Baixo / Médio / Alto por chat ou por execução, ou pelo cabeçalho `x-quickway-thinking-effort` |
| Catálogo de modelos | Atualizado a cada 5 minutos ou sob demanda pela barra do chat |
| Iniciar com o app | Inicia o proxy automaticamente ao abrir o QuickWay |

## 🛠️ Stack tecnológica

| Camada | Tecnologia |
|---|---|
| **Frontend** | TypeScript + Vite |
| **Desktop** | Tauri 2 (Rust) |
| **Backend** | Rust + axum |

## 🤝 Contribuir

Contribuições são bem-vindas! Você pode:

- 🐛 Relatar bugs em [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 Sugerir funcionalidades
- 🔧 Enviar pull requests
- 🌍 Ajudar com as traduções

## 📚 Documentação

- [User guide (中文)](USER_GUIDE_zh.md): manual detalhado sobre credenciais, modelos, a API compatível com OpenAI e o Codex
- [Publicação de releases](RELEASING.md): como as versões são atualizadas e as releases são publicadas

## 📄 Licença

[MIT](../LICENSE) © QuickWay Contributors
