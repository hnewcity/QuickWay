<p align="center">
  <img src="../design/icon/quickway-icon.png" alt="QuickWay Logo" width="128" height="128">
</p>
<h1 align="center">QuickWay</h1>
<h3 align="center">Tu puerta de enlace a Amazon Quick</h3>
<p align="center">Usa los modelos de Quick en Claude Code, Codex y cualquier aplicación compatible con OpenAI o Anthropic mediante una API local</p>

<p align="center">
  <a href="../README.md">🇺🇸 English</a> • <a href="README_zh.md">🇨🇳 中文</a> • <a href="README_ja.md">🇯🇵 日本語</a> • <a href="README_ko.md">🇰🇷 한국어</a> • <a href="README_ru.md">🇷🇺 Русский</a> • 🇪🇸 Español • <a href="README_pt.md">🇧🇷 Português</a> • <a href="README_id.md">🇮🇩 Indonesia</a> • <a href="README_vi.md">🇻🇳 Tiếng Việt</a>
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
<td>¡Gracias a <a href="https://ai18n.chat">ai18n</a> por patrocinar este proyecto! ai18n es un proveedor de servicios de relay de API confiable y eficiente, que ofrece relay de modelos Claude para OpenClaw, Claude Code, Codex, Gemini y más.</td>
</tr>
</table>

---

QuickWay es una puerta de enlace de escritorio que expone los modelos de tu cuenta de Amazon Quick con sesión iniciada mediante una API local compatible con Anthropic y OpenAI. Usa Quick con Claude Code, Codex y tus SDK y herramientas favoritos, sin cambiar código.

## ✨ Características

| Característica | Descripción |
|---|---|
| 🔌 **API compatible con Anthropic** | Endpoint `/v1/messages` para el SDK de Anthropic y Claude Code |
| 🔌 **API compatible con OpenAI** | Endpoints `/v1/chat/completions` y `/v1/responses` para el SDK de OpenAI y Codex |
| 🚀 **Lanzador de programación** | Inicia Claude Code o Codex en su propia terminal con un clic, cada uno con su modelo y nivel |
| 🧠 **Nivel de razonamiento** | Desactivado / Bajo / Medio / Alto, según los controles de razonamiento nativos de cada modelo |
| 📋 **Catálogo dinámico de modelos** | Los modelos se leen de tu cuenta de Quick y se actualizan automáticamente |
| 🛠️ **Llamadas a herramientas** | Llamadas a funciones en Messages, Chat Completions y Responses |
| 👁️ **Soporte de visión** | Envía imágenes en base64 al modelo |
| 📡 **Streaming** | Soporte completo de streaming SSE |
| 💬 **Chat integrado** | Prueba tu configuración con la interfaz de chat integrada |
| 🔐 **Local y credenciales seguras** | Proxy solo en loopback con su propia clave de API; los tokens permanecen en memoria y provienen del Administrador de credenciales de Windows o del llavero de macOS |
| 🔄 **Transferencia de credenciales** | Exporta e importa credenciales entre tus propios equipos Windows y macOS |
| 🔗 **Integración con CC Switch** | Importación con un clic a [CC Switch](https://github.com/farion1231/cc-switch) para Claude Code |
| 🌍 **Interfaz multilingüe** | English, 中文, 日本語, 한국어, Русский, Español, Português, Indonesia, Tiếng Việt |
| 🩺 **Diagnóstico** | Copia un informe de diagnóstico con datos ocultos cuando falle la conexión o la consulta de uso |

## 📦 Instalación

### Descargar

Descarga la última versión desde [GitHub Releases](https://github.com/hnewcity/QuickWay/releases):

| Plataforma | Arquitectura | Descargar |
|---|---|---|
| macOS | Apple Silicon (M1/M2/M3) | [QuickWay_x.y.z_aarch64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS | Intel | [QuickWay_x.y.z_x64.dmg](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Instalador x64 | [QuickWay_x.y.z_x64-setup.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| Windows | Portable x64 | [QuickWay_x.y.z_x64-portable.exe](https://github.com/hnewcity/QuickWay/releases/latest) |
| macOS / Windows | Proxy de línea de comandos | [quickway-cli_x.y.z_*](https://github.com/hnewcity/QuickWay/releases/latest) |

> Requiere Windows 10/11 x64 o macOS 12+. El instalador de Windows no está firmado y las compilaciones de macOS usan firma ad-hoc sin notarización, así que SmartScreen o Gatekeeper pueden pedirte confirmación al abrirlo por primera vez. Inicia sesión primero en Amazon Quick en el mismo equipo.

### Compilar desde el código fuente

```bash
# Clonar el repositorio
git clone https://github.com/hnewcity/QuickWay.git
cd QuickWay/desktop

# Instalar dependencias
npm ci

# Ejecutar en modo desarrollo
npm run tauri dev

# O compilar para producción
npm run desktop:build
```

Requiere Rust (consulta `rust-toolchain.toml`) y Node.js 20.19+ o 22.12+. En macOS instala también las Xcode Command Line Tools.

## 🚀 Inicio rápido

1. **Inicia sesión** en Amazon Quick en este equipo
2. **Abre** QuickWay: detecta automáticamente la cuenta con sesión iniciada
3. **Entra** en la pestaña Programación, elige Claude Code o Codex, un modelo y una carpeta de proyecto, y pulsa Iniciar
4. **O** usa `http://127.0.0.1:8741` como endpoint de la API de Anthropic/OpenAI, con la clave de API de los ajustes de conexión

> Los ID de modelo dependen de tu cuenta. Usa un ID devuelto por `GET /v1/models`.

### Ejemplo: cURL

```bash
curl http://127.0.0.1:8741/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_PROXY_API_KEY" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Ejemplo: Python (OpenAI SDK)

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

### Ejemplo: JavaScript (OpenAI SDK)

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

## 🔌 Compatible con

| Categoría | Herramientas |
|---|---|
| **Agentes de programación** | Claude Code, Codex CLI |
| **Python** | OpenAI SDK, Anthropic SDK, LangChain, LlamaIndex |
| **JavaScript** | OpenAI Node.js SDK, Anthropic SDK, Vercel AI SDK |
| **Herramientas** | CC Switch, Cursor, Continue, Cline |

## ⚙️ Configuración

### Métodos de autenticación

| Método | Descripción |
|---|---|
| **Sesión local de Quick** | Lee la cuenta de Amazon Quick con sesión iniciada (Administrador de credenciales de Windows o llavero de macOS); recomendado |
| **Credenciales importadas** | Un archivo `.qwc` exportado desde otro de tus equipos |

### Configuración del servidor

| Opción | Predeterminado | Descripción |
|---|---|---|
| Host | `127.0.0.1` | Solo loopback; el proxy nunca escucha en otras interfaces |
| Puerto | `8741` | Puerto del servidor, editable en los ajustes de conexión |
| Clave de API del proxy | generada automáticamente | Obligatoria en cada solicitud: `x-api-key` o `Authorization: Bearer` |

### Configuración avanzada

| Opción | Descripción |
|---|---|
| Nivel de razonamiento | Desactivado / Bajo / Medio / Alto por chat o por inicio, o mediante el encabezado `x-quickway-thinking-effort` |
| Catálogo de modelos | Se actualiza cada 5 minutos o bajo demanda desde la barra del chat |
| Iniciar con la aplicación | Inicia el proxy automáticamente al abrir QuickWay |

## 🛠️ Stack tecnológico

| Capa | Tecnología |
|---|---|
| **Frontend** | TypeScript + Vite |
| **Escritorio** | Tauri 2 (Rust) |
| **Backend** | Rust + axum |

## 🤝 Contribuir

¡Las contribuciones son bienvenidas! Puedes:

- 🐛 Informar de errores en [GitHub Issues](https://github.com/hnewcity/QuickWay/issues)
- 💡 Sugerir funciones
- 🔧 Enviar pull requests
- 🌍 Ayudar con las traducciones

## 📚 Documentación

- [User guide (中文)](USER_GUIDE_zh.md): manual detallado sobre credenciales, modelos, la API compatible con OpenAI y Codex
- [Publicación de versiones](RELEASING.md): cómo se actualizan las versiones y se publican las versiones finales

## 📄 Licencia

[MIT](../LICENSE) © QuickWay Contributors
