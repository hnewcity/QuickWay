import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open, save } from '@tauri-apps/plugin-dialog';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import '@fontsource-variable/fraunces/full.css';
import './style.css';
import { drawCrowd, figure, sprite } from './characters';
import { language, languages, locale, setLanguage, t, type Language, type MessageKey } from './i18n';

type Model = { id: string; name: string; family: string; upstream: string };
type ThinkingEffort = 'off' | 'low' | 'medium' | 'high';
type Preferences = { port: number; model: string; projectDir: string; autoStart: boolean; thinkingEffort: ThinkingEffort; codexModel: string; codexEffort: ThinkingEffort };
type ModelCatalog = { source: 'cloud' | 'cache' | 'unavailable'; lastUpdated: number | null; error: string | null };
type Status = { preferences: Preferences; running: boolean; account: { signedIn: boolean; canRefresh: boolean; error: string | null; expiresAt?: number; enterprise?: boolean }; credentials?: { source: 'local' | 'imported' }; models: Model[]; modelCatalog: ModelCatalog; lastCloudModel?: { mode: string; servedModel: string | null; at: number } | null; claudeInstalled: boolean; codexInstalled: boolean; version: string };
type Usage = { monthlyUsage?: { availableUnits?: number; provisionedUnits?: number; usedPercentage?: number }; sessionUsage?: { usedPercentage?: number }; subscription?: { subscriptionType?: string; freeTrialMetadata?: { expiryDate?: number } } };
type Message = { id: string; role: 'user' | 'assistant'; content: string; model?: string; servedModel?: string; thinkingEffort?: ThinkingEffort; error?: string; interrupted?: boolean; input?: number; output?: number };
type Conversation = { id: string; title: string; model: string; thinkingEffort?: ThinkingEffort; messages: Message[]; updated: number };
type ChatEvent = { type: 'delta'; text: string } | { type: 'model'; served: string } | { type: 'usage'; input: number; output: number } | { type: 'done'; reason: string } | { type: 'cancelled' } | { type: 'error'; message: string };

const isMac = /Mac/.test(navigator.platform);
// Matches the native window configs: macOS overlays its traffic lights, Windows has no system frame.
const isWindows = /Win/.test(navigator.platform);
document.documentElement.dataset.platform = isMac ? 'mac' : isWindows ? 'windows' : 'other';
// Content headers double as the window's drag handle on macOS, where the title bar overlays them.
const headerDrag = isMac ? ' data-tauri-drag-region="deep"' : '';
const terminalName = () => isMac ? t('terminalMac') : 'PowerShell';
const backgroundNote = () => t(isMac ? 'backgroundMac' : 'backgroundWindows');

const icons: Record<string, string> = {
  chat: '<path d="M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z"/>',
  terminal: '<path d="m5 7 5 5-5 5M13 17h6"/>',
  settings: '<path d="M4 7h9m4 0h3M4 17h3m4 0h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  send: '<path d="m12 19 0-14M5 12l7-7 7 7"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  arrow: '<path d="M5 12h14m-6-6 6 6-6 6"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V4H4v12h4"/>',
  refresh: '<path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 8a7 7 0 0 1 11-3l3 7M4 12l3 7a7 7 0 0 0 11-3"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  bolt: '<path d="m13 2-9 12h7l-1 8 10-12h-7l1-8Z"/>',
  code: '<path d="m8 6-6 6 6 6m8-12 6 6-6 6m-3-15-2 18"/>',
  pen: '<path d="m15 5 4 4M4 20l4-1L20 7a2.8 2.8 0 0 0-4-4L4 15v5Z"/>',
  layers: '<path d="m12 3 10 6-10 6L2 9l10-6Zm-9 11 9 5 9-5M3 18l9 5 9-5"/>',
  dashboard: '<rect x="3" y="3" width="7" height="9" rx="1.5"/><rect x="14" y="3" width="7" height="5" rx="1.5"/><rect x="14" y="12" width="7" height="9" rx="1.5"/><rect x="3" y="16" width="7" height="5" rx="1.5"/>',
  eye: '<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M3 3l18 18M10.6 5.1A10.6 10.6 0 0 1 12 5c6.4 0 10 7 10 7a17.8 17.8 0 0 1-3.2 4.2M6.6 6.6C3.9 8.4 2 12 2 12s3.6 7 10 7a9.9 9.9 0 0 0 5.4-1.6M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3Z"/>',
  external: '<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>',
};
const icon = (name: string, cls = '') => `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${icons[name] || icons.chat}</svg>`;
// Little Q, the selected monochrome app mark (design/icon).
const brand = '<svg viewBox="24 24 464 464" aria-hidden="true"><rect x="24" y="24" width="464" height="464" rx="108" fill="#000"/><g transform="translate(7.68 12.53) scale(0.97)" fill="#fff"><path fill-rule="evenodd" d="M252 114C168 114 110 165 110 244C110 323 168 374 252 374C275 374 297 369 317 359Q323 356 329 360L347 371C359 379 373 370 373 357V329Q373 319 377 311C389 289 394 268 394 244C394 165 336 114 252 114ZM252 156C190 156 152 188 152 244C152 300 190 332 252 332C314 332 352 300 352 244C352 188 314 156 252 156Z"/><rect x="205" y="216" width="30" height="48" rx="15"/><rect x="269" y="216" width="30" height="48" rx="15"/></g></svg>';
const shortcut = isMac ? '⌘ N' : 'Ctrl N';
const escape = (s: unknown) => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]!));
const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T | null;
const fmt = (n: number | undefined) => n === undefined ? '—' : new Intl.NumberFormat(locale(), { maximumFractionDigits: 1 }).format(n);
let status: Status | null = null;
let usage: Usage | null = null;
let usageError = '';
let credentialBusy = false;
let credentialRevision = 0;
type View = 'dashboard' | 'code' | 'chat' | 'settings';
const viewTitleKeys: Record<View, MessageKey> = { dashboard: 'navDashboard', code: 'navCode', chat: 'navChat', settings: 'navSettings' };
let view: View = 'dashboard';
type ApiFormat = 'openai' | 'anthropic';
type ApiLanguage = 'shell' | 'python' | 'javascript';
let apiFormat: ApiFormat = 'openai';
let apiLanguage: ApiLanguage = 'shell';
let apiKey = '';
let revealKey = false;
let showModels = false;
let conversations: Conversation[] = [];
try { const saved = JSON.parse(localStorage.getItem('quickway.conversations.v1') || '[]'); if (Array.isArray(saved)) conversations = saved.filter(c => typeof c?.id === 'string' && Array.isArray(c.messages) && typeof c.title === 'string' && typeof c.model === 'string'); } catch { /* Keep an invalid saved value untouched until the user creates a new conversation. */ }
let selected = conversations[0]?.id || '';
let activeRequest: string | null = null;
let refreshing = false;
let refreshingModels = false;
let authorizing = false;
let preferenceRevision = 0;
let draft = '';
let pendingPreferences: Promise<void> = Promise.resolve();
let toastTimer: ReturnType<typeof setTimeout>;
const current = () => conversations.find(c => c.id === selected);
const hasSelectedModel = () => availableModels().some(m => m.id === status?.preferences.model);
// Codex keeps its own model; an empty choice follows the shared default.
const codexModel = () => status!.preferences.codexModel || status!.preferences.model;
const hasCodexModel = () => availableModels().some(m => m.id === codexModel());
const codexReady = () => !!status?.codexInstalled && !!status.preferences.projectDir && hasCodexModel();
const claudeReady = () => !!status?.claudeInstalled && !!status.preferences.projectDir && hasSelectedModel();
// The Coding page launches one tool at a time; the last pick is remembered across restarts.
type CodingTool = 'claude' | 'codex';
let codingTool: CodingTool = localStorage.getItem('quickway.codingTool') === 'codex' ? 'codex' : 'claude';
const tierKeys: Record<string, MessageKey> = { 'quick-fast': 'tierFast', 'quick-balanced': 'tierBalanced', 'quick-smart': 'tierSmart', 'quick-advanced': 'tierAdvanced' };
const availableModels = () => Object.keys(tierKeys).flatMap(id => status?.models.find(m => m.id === id) || []);
const modelName = (id?: string) => id && Object.hasOwn(tierKeys, id) ? t(tierKeys[id]) : t('chooseModel');
const modelMark = (_id: string) => '◇';
const validServedModel = (id: unknown): id is string => typeof id === 'string' && id.length > 0 && id.length <= 512 && /^[a-zA-Z0-9._:/-]+$/.test(id) && !id.includes('://');
function servedModelName(id: string) {
  const short = id.replace(/^(?:(?:us|eu|apac|global)\.)?(?:anthropic|openai)\./, '');
  const claude = short.match(/^claude-(haiku|sonnet|opus|fable)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?(?:-v\d+(?::\d+)?)?$/);
  if (claude) return `Claude ${claude[1][0].toUpperCase()}${claude[1].slice(1)} ${claude[2]}${claude[3] ? `.${claude[3]}` : ''}`;
  const gpt = short.match(/^gpt-(\d+(?:\.\d+)?)-(astra|luna|terra|sol)$/);
  return gpt ? `GPT-${gpt[1]} ${gpt[2][0].toUpperCase()}${gpt[2].slice(1)}` : id;
}
function actualModelHtml(message?: Message) {
  const served = message?.servedModel;
  const pending = !!activeRequest && message === current()?.messages.at(-1);
  const label = validServedModel(served) ? servedModelName(served) : t(pending ? 'actualModelPending' : message ? 'actualModelUnknown' : 'actualModelIdle');
  return `<span class="actual-model" title="${escape(validServedModel(served) ? served : '')}"><span>${t('actualModel')}</span> ${escape(label)}</span>`;
}
function recentModelHtml() {
  const recent = status?.lastCloudModel;
  const served = recent?.servedModel;
  const label = validServedModel(served) ? servedModelName(served) : t(recent ? 'actualModelUnknown' : 'actualModelIdle');
  return `<span>${t('recentModel')}</span><strong title="${escape(validServedModel(served) ? served : '')}">${escape(label)}</strong>${recent ? `<small>${escape(modelName(`quick-${recent.mode}`))} · ${escape(new Date(recent.at * 1000).toLocaleTimeString(locale()))}</small>` : ''}`;
}

function toast(text: string, error = false) {
  const target = el('toast')!;
  target.textContent = text;
  target.className = `toast visible ${error ? 'error' : ''}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => target.classList.remove('visible'), error ? 7000 : 3300);
}
// Redraw the welcome crowd when its stage changes size; old stages are released on re-render.
let crowdFrame = 0;
const crowdObserver = new ResizeObserver(entries => {
  cancelAnimationFrame(crowdFrame);
  crowdFrame = requestAnimationFrame(() => entries.forEach(entry => {
    const host = entry.target as HTMLElement;
    if (!host.isConnected) { crowdObserver.unobserve(host); return; }
    const svg = host.firstElementChild as SVGSVGElement | null;
    const { width, height } = entry.contentRect;
    if (!svg || Math.abs(svg.width.baseVal.value - width) > 2 || Math.abs(svg.height.baseVal.value - height) > 2) drawCrowd(host);
  }));
});
function persist() {
  try { localStorage.setItem('quickway.conversations.v1', JSON.stringify(conversations)); }
  catch { toast(t('storageFull'), true); }
}
async function action(command: string, args?: Record<string, unknown>, success?: string) {
  try { const result = await invoke(command, args); if (success) toast(success); return result; }
  catch (error) { toast(String(error), true); throw error; }
}

function renderShell() {
  document.documentElement.lang = locale();
  document.querySelector('#app')!.innerHTML = `${sprite}
  <aside class="sidebar">
    <div class="sidebar-drag" data-tauri-drag-region aria-hidden="true"></div>
    <div class="brand" data-tauri-drag-region="deep"><span class="brand-icon">${brand}</span><div><strong>QuickWay</strong><small>${t('tagline')}</small></div></div>
    <nav class="side-nav" aria-label="${t('mainNav')}">
      <button data-view="dashboard" class="nav-item active">${icon('dashboard')}${t('navDashboard')}</button>
      <button data-view="code" class="nav-item">${icon('terminal')}${t('navCode')}</button>
      <button data-view="chat" class="nav-item">${icon('chat')}${t('navChat')}</button>
      <button data-view="settings" class="nav-item">${icon('settings')}${t('navSettings')}</button>
    </nav>
    <section class="recents" id="recents-panel" aria-label="${t('recents')}">
      <div class="recent-heading"><span>${t('recents')}</span><span class="count" id="conversation-count"></span><button class="recent-new" id="new-chat" title="${t('newChatTitle', { shortcut })}">${icon('plus')}${t('newChat')}</button></div>
      <div class="recent-list" id="recents"></div>
    </section>
    <footer class="sidebar-ending">
      <button class="connection-summary" id="connection-summary"><i class="status-dot" id="sidebar-dot"></i><span><strong id="sidebar-status">${t('connecting')}</strong><small id="sidebar-endpoint">${t('readingLogin')}</small></span>${icon('arrow')}</button>
      <svg class="wordmark" viewBox="0 0 200 46" role="img" aria-label="QuickWay"><text x="1" y="37" textLength="198" lengthAdjust="spacingAndGlyphs">QuickWay</text></svg>
      <label class="language-picker" title="${t('language')}">${icon('globe')}<select id="language" aria-label="${t('language')}">${languages.map(l => `<option value="${l.code}" ${l.code === language() ? 'selected' : ''}>${l.label}</option>`).join('')}</select></label>
      <div class="sidebar-note" title="${backgroundNote()}"><span id="version">v${escape(status?.version || '0.3.0')}</span> · ${backgroundNote()}</div>
    </footer>
  </aside>
  <div class="shell" data-tauri-drag-region>
    ${isWindows ? `<header class="titlebar" data-tauri-drag-region><div class="window-controls">
      <button class="window-control" data-window="minimize" aria-label="${t('minimize')}" title="${t('minimize')}"><svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10"/></svg></button>
      <button class="window-control" data-window="maximize" id="window-maximize" aria-label="${t('maximize')}" title="${t('maximize')}"><svg viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" rx="1"/></svg></button>
      <button class="window-control close" data-window="close" aria-label="${t('close')}" title="${t('closeTitle')}"><svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9"/></svg></button>
    </div></header>` : ''}
    <main class="sheet" id="workspace"></main>
  </div>
  <div class="toast" id="toast" role="status"></div>`;
  el('new-chat')!.onclick = newConversation;
  el('connection-summary')!.onclick = () => { view = 'dashboard'; renderView(); };
  el<HTMLSelectElement>('language')!.onchange = e => changeLanguage((e.target as HTMLSelectElement).value as Language);
  if (isWindows) void syncMaximized();
}

// Windows draws its own caption buttons; the window API is only reached on click, so browser previews still load.
const maximizeIcon = '<rect x="0.5" y="0.5" width="9" height="9" rx="1"/>';
const restoreIcon = '<rect x="0.5" y="2.5" width="7" height="7" rx="1"/><path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1"/>';
let maximizeFrame = 0;
async function syncMaximized() {
  const button = el('window-maximize');
  if (!button) return;
  try {
    const maximized = await getCurrentWindow().isMaximized();
    const label = t(maximized ? 'restore' : 'maximize');
    if (button.getAttribute('aria-label') === label) return;
    button.setAttribute('aria-label', label);
    button.title = label;
    button.querySelector('svg')!.innerHTML = maximized ? restoreIcon : maximizeIcon;
  } catch { /* No native window, as in a browser preview. */ }
}
if (isWindows) {
  try { void getCurrentWindow().onResized(() => { cancelAnimationFrame(maximizeFrame); maximizeFrame = requestAnimationFrame(() => void syncMaximized()); }).catch(() => {}); } catch { /* Browser preview. */ }
}

function renderRecents() {
  el('conversation-count')!.textContent = conversations.length ? String(conversations.length).padStart(2, '0') : '';
  el('recents')!.innerHTML = conversations.length ? conversations.map(c => `<button class="recent-item ${c.id === selected && view === 'chat' ? 'selected' : ''}" data-conversation="${escape(c.id)}" title="${escape(c.title)}">${icon('chat')}<span>${escape(c.title)}</span></button>`).join('') : `<div class="no-recents">${figure('bubble', 'no-recents-figure')}<p>${t('noRecents')}</p></div>`;
}

function chrome() {
  if (!status) return;
  el('sidebar-status')!.textContent = t(status.running ? 'proxyRunning' : 'proxyStopped');
  el('sidebar-endpoint')!.textContent = `127.0.0.1:${status.preferences.port}`;
  el('sidebar-dot')!.className = `status-dot ${status.running ? 'online' : ''}`;
  el('version')!.textContent = `v${status.version}`;
  document.querySelectorAll<HTMLElement>('[data-account-heading]').forEach(e => { const content = accountContents(); if (e.innerHTML !== content) e.innerHTML = content; });
  const credentialState = el('credential-state');
  if (credentialState) credentialState.textContent = credentialSummary();
  document.querySelectorAll<HTMLButtonElement>('[data-credentials]').forEach(e => {
    const mode = e.dataset.credentials;
    e.disabled = credentialBusy || (mode === 'export' && !status!.account.signedIn);
    e.hidden = (mode === 'forget' && !importedCredentials());
  });
  const notice = el('login-notice');
  if (notice) {
    notice.hidden = status.account.signedIn;
    notice.style.display = status.account.signedIn ? 'none' : '';
    const content = status.account.signedIn ? '' : loginNotice();
    if (notice.innerHTML !== content) notice.innerHTML = content;
  }
  document.querySelectorAll<HTMLElement>('[data-model-summary]').forEach(e => { e.textContent = catalogSummary(); e.title = status!.modelCatalog?.error || t('catalogAutoUpdate'); });
  document.querySelectorAll<HTMLButtonElement>('[data-authorize-quick]').forEach(e => { e.disabled = authorizing; });
  document.querySelectorAll<HTMLButtonElement>('[data-refresh-models]').forEach(e => { e.disabled = refreshingModels; });
  document.querySelectorAll<HTMLElement>('[data-recent-model]').forEach(e => { const content = recentModelHtml(); if (e.innerHTML !== content) e.innerHTML = content; });
  const launch = el<HTMLButtonElement>('launch-claude');
  if (launch) launch.disabled = !claudeReady();
  const launchCodex = el<HTMLButtonElement>('launch-codex');
  if (launchCodex) launchCodex.disabled = !codexReady();
  document.querySelectorAll<HTMLButtonElement>('[data-import-ccswitch], [data-copy-example], [data-copy-connection]').forEach(e => { e.disabled = !hasSelectedModel(); });
  document.querySelectorAll<HTMLElement>('[data-proxy-label]').forEach(e => e.textContent = t(status!.running ? 'running' : 'stopped'));
  document.querySelectorAll<HTMLElement>('[data-proxy-state]').forEach(e => e.classList.toggle('is-running', status!.running));
  document.querySelectorAll<HTMLElement>('[data-model-count]').forEach(e => e.textContent = String(availableModels().length));
  document.querySelectorAll<HTMLButtonElement>('[data-toggle-proxy]').forEach(e => {
    const label = t(status!.running ? 'stopProxy' : 'startProxy');
    // Preserve the pressed DOM node during a port save or periodic refresh.
    if (e.textContent !== label) e.innerHTML = `${icon(status!.running ? 'stop' : 'bolt')}${label}`;
  });
}

function loginNotice() {
  return `${figure('key')}<span>${escape(status!.account.error || t('noQuickLogin'))}</span>${isMac && !importedCredentials() ? `<button class="pill pill-brown" data-authorize-quick>${t('authorizeKeychain')}</button>` : ''}<button class="pill pill-brown" data-go="settings">${t('importCredentials')}</button>${!importedCredentials() ? `<button class="pill pill-brown" data-open-quick>${t('openAmazonQuick')}</button>` : ''}`;
}

function catalogSummary() {
  const source = status?.modelCatalog?.source;
  return t('catalogSummary', { count: availableModels().length, state: t(refreshingModels ? 'catalogRefreshing' : source === 'cloud' ? 'catalogSynced' : source === 'cache' ? 'catalogCache' : 'catalogNone') });
}
// Model and thinking effort share one picker: a pill that opens a single popover with the effort levels
// on top and the model list below, so both choices are one click away.
type PickerScope = 'chat' | 'claude' | 'codex';
type PickerValue = { model: string; effort: ThinkingEffort };
const efforts: ThinkingEffort[] = ['off', 'low', 'medium', 'high'];
// The message badge keeps the English level name ("Thinking Low") in every language.
const effortLabels: Record<ThinkingEffort, string> = { off: 'Off', low: 'Low', medium: 'Medium', high: 'High' };
const effortNameKeys: Record<ThinkingEffort, MessageKey> = { off: 'effortOff', low: 'effortLow', medium: 'effortMedium', high: 'effortHigh' };
const effortHintKeys: Record<ThinkingEffort, MessageKey> = { off: 'effortHintOff', low: 'effortHintLow', medium: 'effortHintMedium', high: 'effortHintHigh' };
const effortName = (effort: ThinkingEffort) => t(effortNameKeys[effort]);
const effortValue = (value: unknown): ThinkingEffort => typeof value === 'string' && Object.hasOwn(effortLabels, value) ? value as ThinkingEffort : 'off';
const chevron = (path: string, cls = '') => `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
// What each picker shows. Read from the conversation or preferences on render, then updated on each pick,
// so a choice shows at once even while its preference save is still pending.
const pickerValues: Partial<Record<PickerScope, PickerValue>> = {};
let openPicker: PickerScope | null = null;

function pickerSource(scope: PickerScope): PickerValue {
  const prefs = status!.preferences;
  if (scope === 'codex') return { model: codexModel(), effort: effortValue(prefs.codexEffort) };
  const conversation = scope === 'chat' ? current() : undefined;
  return { model: conversation?.model || prefs.model, effort: effortValue(conversation ? conversation.thinkingEffort : prefs.thinkingEffort) };
}
const isKnownModel = (id: string) => availableModels().some(m => m.id === id);
const pickedModelName = (id: string) => isKnownModel(id) ? modelName(id) : Object.hasOwn(tierKeys, id) ? t('modelUnavailableName', { name: modelName(id) }) : t('chooseModel');
const missingModelText = (id: string) => Object.hasOwn(tierKeys, id) ? t('modelNotInCatalog', { name: modelName(id) }) : t(availableModels().length ? 'pickCloudModel' : 'noCloudModels');

function triggerHtml(scope: PickerScope) {
  const { model, effort } = pickerValues[scope]!;
  return `<span class="model-mark">${modelMark(model)}</span><span class="mode-name ${isKnownModel(model) ? '' : 'is-missing'}">${escape(pickedModelName(model))}</span><span class="mode-effort level-${effort}">${effort === 'off' ? t('effortOff') : t('thinkingLevel', { level: effortName(effort) })}</span>${chevron('m6 9 6 6 6-6', 'mode-chevron')}`;
}
function pickerHtml(scope: PickerScope) {
  pickerValues[scope] = pickerSource(scope);
  const { model, effort } = pickerValues[scope]!;
  return `<div class="mode-picker mode-${scope}" data-picker="${scope}"><button type="button" class="mode-trigger" id="${scope}-picker" aria-haspopup="dialog" aria-expanded="false" aria-controls="${scope}-picker-pop" title="${t('pickerTitle')}" data-model="${escape(model)}" data-effort="${effort}" ${activeRequest ? 'disabled' : ''}>${triggerHtml(scope)}</button><div class="mode-pop" id="${scope}-picker-pop" role="dialog" aria-label="${t('modelAndEffort')}" hidden></div></div>`;
}
function popHtml(scope: PickerScope) {
  const { model, effort } = pickerValues[scope]!;
  const missing = isKnownModel(model) ? '' : `<div class="mode-option mode-missing" role="option" aria-selected="true" aria-disabled="true"><span class="mode-option-name">${escape(missingModelText(model))}</span></div>`;
  const groups = `<div class="mode-family" role="group" aria-label="Amazon Quick">${availableModels().map(m => `<button type="button" class="mode-option" role="option" aria-selected="${m.id === model}" data-model-option="${escape(m.id)}"><span class="model-mark">${modelMark(m.id)}</span><span class="mode-option-name">${escape(modelName(m.id))}</span>${m.id === model ? icon('check') : ''}</button>`).join('')}</div>`;
  return `<section class="mode-section"><div class="mode-section-head"><span class="eyebrow">${t('thinkingEffort')}</span></div>
    <div class="effort-choice" id="${scope}-effort" role="radiogroup" aria-label="${t('thinkingEffort')}" data-value="${effort}">${efforts.map(key => `<button type="button" role="radio" id="${scope}-effort-${key}" class="level-${key}" aria-checked="${key === effort}" tabindex="${key === effort ? 0 : -1}" data-effort-stop="${key}">${key === 'off' ? t('effortOffShort') : effortName(key)}</button>`).join('')}</div>
    <p class="mode-hint">${t(effortHintKeys[effort])}</p></section>
    <section class="mode-section mode-model-section"><div class="mode-section-head"><span class="eyebrow">${t('model')}</span><span class="mode-count">${t('modelCountShort', { count: availableModels().length })}</span></div><div class="mode-models" role="listbox" aria-label="${t('model')}">${missing}${groups}</div></section>`;
}
function syncPicker(scope: PickerScope) {
  const trigger = el<HTMLButtonElement>(`${scope}-picker`);
  const pop = el(`${scope}-picker-pop`);
  if (!trigger || !pop || !pickerValues[scope]) return;
  const { model, effort } = pickerValues[scope]!;
  trigger.innerHTML = triggerHtml(scope);
  trigger.dataset.model = model;
  trigger.dataset.effort = effort;
  const open = openPicker === scope;
  trigger.setAttribute('aria-expanded', String(open));
  pop.hidden = !open;
  if (!open) { pop.innerHTML = ''; return; }
  // Rebuilding the popover must not drop keyboard focus or the list's scroll position.
  const focused = document.activeElement instanceof HTMLElement && pop.contains(document.activeElement) ? document.activeElement : null;
  const focusKey = focused && (focused.dataset.modelOption || focused.id);
  const scroll = pop.querySelector('.mode-models')?.scrollTop || 0;
  pop.innerHTML = popHtml(scope);
  pop.querySelector('.mode-models')!.scrollTop = scroll;
  if (focusKey) [...pop.querySelectorAll<HTMLElement>('[id], [data-model-option]')].find(e => e.dataset.modelOption === focusKey || e.id === focusKey)?.focus();
}
const checkedEffort = (scope: PickerScope) => el(`${scope}-effort-${pickerValues[scope]!.effort}`);
function setPicker(next: PickerScope | null, focus = true) {
  const previous = openPicker;
  openPicker = next;
  if (previous && previous !== next) syncPicker(previous);
  if (!next) return;
  syncPicker(next);
  const pop = el(`${next}-picker-pop`)!;
  pop.querySelector<HTMLElement>('.mode-option[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  if (focus) checkedEffort(next)?.focus();
}
function closePicker(returnFocus = false) {
  const scope = openPicker;
  if (!scope) return;
  setPicker(null);
  if (returnFocus) el(`${scope}-picker`)?.focus();
}
function updateModelChoices() {
  for (const scope of ['chat', 'claude', 'codex'] as PickerScope[]) syncPicker(scope);
}
// Picking a model finishes the choice, like any dropdown; changing effort keeps the popover open.
async function pickModel(scope: PickerScope, value: string) {
  const conversation = scope === 'chat' ? current() : undefined;
  pickerValues[scope] = { ...pickerValues[scope]!, model: value };
  closePicker(true);
  try {
    await savePrefs(scope === 'codex' ? { codexModel: value } : { model: value });
    if (conversation) { conversation.model = value; persist(); }
  } catch { renderView(); }
}
async function pickEffort(scope: PickerScope, value: ThinkingEffort) {
  if (pickerValues[scope]!.effort === value) { syncPicker(scope); return; }
  const conversation = scope === 'chat' ? current() : undefined;
  pickerValues[scope] = { ...pickerValues[scope]!, effort: value };
  syncPicker(scope);
  if (el(`${scope}-picker-pop`)?.contains(document.activeElement)) checkedEffort(scope)?.focus();
  try {
    await savePrefs(scope === 'codex' ? { codexEffort: value } : { thinkingEffort: value });
    if (conversation) { conversation.thinkingEffort = value; persist(); }
  } catch { renderView(); }
}

// Keys follow the radio group and listbox patterns; ↓ from the levels moves into the model list.
document.addEventListener('keydown', e => {
  if (!openPicker) return;
  const scope = openPicker;
  if (e.key === 'Escape') { e.preventDefault(); closePicker(true); return; }
  const target = e.target as HTMLElement;
  const pop = el(`${scope}-picker-pop`)!;
  const options = [...pop.querySelectorAll<HTMLElement>('[data-model-option]')];
  if (target.matches('[data-effort-stop]')) {
    if (e.key === 'ArrowDown') { e.preventDefault(); (options.find(o => o.getAttribute('aria-selected') === 'true') || options[0])?.focus(); return; }
    const index = efforts.indexOf(pickerValues[scope]!.effort);
    const moves: Record<string, number> = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: efforts.length - 1 };
    if (!(e.key in moves)) return;
    e.preventDefault();
    void pickEffort(scope, efforts[Math.min(efforts.length - 1, Math.max(0, moves[e.key]))]);
  } else if (target.matches('[data-model-option]') && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault();
    const next = options.indexOf(target) + (e.key === 'ArrowDown' ? 1 : -1);
    if (next < 0) checkedEffort(scope)?.focus();
    else options[Math.min(options.length - 1, next)]?.focus();
  }
});

function messageHtml(message: Message) {
  const assistant = message.role === 'assistant';
  const content = assistant ? DOMPurify.sanitize(marked.parse(message.content, { async: false }), { FORBID_TAGS: ['style', 'iframe', 'form', 'input', 'button'], FORBID_ATTR: ['style'] }) : escape(message.content);
  return `<article class="message ${assistant ? 'assistant' : 'user'}" id="message-${message.id}"><div class="message-avatar">${assistant ? brand : escape(t('you'))}</div><div class="message-main"><div class="message-author">${assistant ? escape(modelName(message.model)) : escape(t('you'))}${assistant ? '<span class="author-tag">Quick</span>' : ''}${assistant && message.thinkingEffort ? `<span class="author-tag effort-badge">Thinking ${effortLabels[effortValue(message.thinkingEffort)]}</span>` : ''}</div>${assistant ? actualModelHtml(message) : ''}<div class="message-content ${assistant ? 'markdown' : ''}">${content || (assistant && activeRequest ? `<span class="thinking-label">${message.thinkingEffort && message.thinkingEffort !== 'off' ? t('thinking') : ''}<span class="typing"><i></i><i></i><i></i></span></span>` : '')}</div><div class="message-error">${escape(message.error || '')}</div><div class="message-footer">${assistant && message.content ? `<button class="subtle-icon" data-copy-message="${message.id}" title="${t('copyReply')}" aria-label="${t('copyReply')}">${icon('copy')}</button>` : ''}<span>${message.interrupted ? t('generationStopped') : message.output !== undefined ? t('tokenUsage', { input: fmt(message.input), output: fmt(message.output) }) : ''}</span></div></div></article>`;
}

function renderChat() {
  const conversation = current();
  const hasMessages = !!conversation?.messages.length;
  const selectedModel = conversation?.model || status!.preferences.model;
  const suggestion = (tone: string, character: string, name: 'Explain' | 'Review' | 'Plan' | 'Polish') => `<button class="suggestion tile-${tone}" data-prompt="${escape(t(`suggest${name}Prompt`))}">${figure(character)}<span><strong>${t(`suggest${name}Title`)}</strong><small>${t(`suggest${name}Text`)}</small></span></button>`;
  el('workspace')!.innerHTML = `<div class="chat-view ${hasMessages ? '' : 'is-empty'}">
    <header class="chat-header"${headerDrag}><h1 class="chat-title">${escape(conversation?.title || t('newChat'))}</h1><div class="chat-tools"><span class="chat-toolbar-note" data-model-summary>${escape(catalogSummary())}</span><button class="icon-button" data-refresh-models title="${t('refreshModels')}" aria-label="${t('refreshModels')}">${icon('refresh')}</button><button class="icon-button" id="copy-chat" title="${t('copyChat')}" aria-label="${t('copyChat')}" ${!hasMessages ? 'disabled' : ''}>${icon('copy')}</button></div></header>
    <div class="chat-scroll" id="chat-scroll">${hasMessages ? `<div class="messages" id="messages">${conversation.messages.map(messageHtml).join('')}</div>` : `<div class="welcome"><div class="crowd" id="crowd" aria-hidden="true"></div><section class="hero-island"><div class="hero-emblem">${brand}</div><h1>${t('heroTitle')}</h1><p>${t('heroBody')}</p><div class="suggestions">${suggestion('blue', 'spark', 'Explain')}${suggestion('yellow', 'bot', 'Review')}${suggestion('pink', 'folder', 'Plan')}${suggestion('peach', 'bubble', 'Polish')}</div></section></div>`}</div>
    <div class="composer-area"><div class="chat-model-status" id="chat-model-status" role="status">${actualModelHtml(conversation?.messages.filter(m => m.role === 'assistant').at(-1))}</div><div class="login-notice" id="login-notice" ${status!.account.signedIn ? 'hidden style="display:none"' : ''}>${status!.account.signedIn ? '' : loginNotice()}</div><form class="composer" id="chat-form"><textarea id="prompt" rows="1" maxlength="60000" placeholder="${t('composerPlaceholder')}" aria-label="${t('messageInput')}">${escape(draft)}</textarea><div class="composer-bottom"><div class="composer-tools">${pickerHtml('chat')}</div><button class="send-button ${activeRequest ? 'stopping' : ''}" id="send" type="${activeRequest ? 'button' : 'submit'}" aria-label="${t(activeRequest ? 'stopGenerating' : 'sendMessage')}" title="${t(activeRequest ? 'stopGenerating' : 'sendMessage')}">${icon(activeRequest ? 'stop' : 'send')}</button></div></form></div>
    </div>`;
  const crowd = el('crowd');
  if (crowd) { drawCrowd(crowd); crowdObserver.observe(crowd); }
  const prompt = el<HTMLTextAreaElement>('prompt')!;
  prompt.addEventListener('input', () => { draft = prompt.value; resizePrompt(); });
  prompt.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); if (!activeRequest) void sendMessage(); } });
  el('chat-form')!.addEventListener('submit', e => { e.preventDefault(); if (!activeRequest) void sendMessage(); });
  el('send')!.addEventListener('click', () => { if (activeRequest) void action('cancel_chat', { requestId: activeRequest }).catch(() => {}); });
  resizePrompt();
  if (hasMessages) el('chat-scroll')!.scrollTop = el('chat-scroll')!.scrollHeight;
}

function resizePrompt() { const p = el<HTMLTextAreaElement>('prompt'); if (p) { p.style.height = 'auto'; p.style.height = `${Math.min(160, p.scrollHeight)}px`; } }

const codingTools = {
  claude: { name: 'Claude Code', icon: 'terminal', installed: () => status!.claudeInstalled, ready: claudeReady, detected: 'claudeDetected', missing: 'claudeMissing', effortHelp: 'claudeEffortHelp', launch: 'launchClaude', launched: 'claudeLaunched', command: 'launch_claude', install: 'claudeInstallInfo', installUrl: 'https://code.claude.com/docs/en/setup' },
  codex: { name: 'Codex', icon: 'code', installed: () => status!.codexInstalled, ready: codexReady, detected: 'codexDetected', missing: 'codexMissing', effortHelp: 'codexEffortHelp', launch: 'launchCodex', launched: 'codexLaunched', command: 'launch_codex', install: 'codexInstallInfo', installUrl: 'https://developers.openai.com/codex/cli' },
} satisfies Record<CodingTool, { name: string; icon: string; installed: () => boolean; ready: () => boolean; detected: MessageKey; missing: MessageKey; effortHelp: MessageKey; launch: MessageKey; launched: MessageKey; command: string; install: MessageKey; installUrl: string }>;

function renderCode() {
  const prefs = status!.preferences;
  const tool = codingTools[codingTool];
  const installed = tool.installed();
  const toolTab = (id: CodingTool) => `<button type="button" class="${id === codingTool ? 'active' : ''}" data-coding-tool="${id}" aria-pressed="${id === codingTool}">${icon(codingTools[id].icon)}${codingTools[id].name}</button>`;
  const terminalTile = codingTool === 'codex'
    ? `<p>${t('codexConfigBody')}</p><button class="pill pill-outline pill-small" data-copy-codex-config>${icon('copy')}${t('copyConfig')}</button>`
    : `<p>${t('ownTerminalBody', { terminal: terminalName() })}</p><button class="pill pill-outline pill-small" data-copy-connection>${icon('copy')}${t('copyConfig')}</button>`;
  el('workspace')!.innerHTML = `<div class="page-scroll"><div class="page">
    <header class="page-header"${headerDrag}><div><span class="eyebrow">Claude Code · Codex</span><h1>${t('codeHeading')}</h1><p>${t('codeIntro')}</p></div>${figure('bot', 'header-figure')}</header>
    <div class="split">
      <section class="scene launch-panel"><div class="panel-title"><h2>${t('startCoding')}</h2><span class="badge ${installed ? 'on' : 'warn'}">${t(installed ? tool.detected : tool.missing)}</span></div>
        <div class="recent-model coding-recent-model" data-recent-model role="status">${recentModelHtml()}</div><span class="field-label">${t('codingTool')}</span><div class="segmented tool-switch" role="group" aria-label="${t('codingTool')}">${toolTab('claude')}${toolTab('codex')}</div>
        <span class="field-label">${t('modelAndEffort')}</span>${pickerHtml(codingTool)}<p class="field-help claude-effort-help">${t(tool.effortHelp)}</p>
        <label class="field-label" for="project-path">${t('projectFolder')}</label><div class="path-field"><span>${icon('folder')}</span><input id="project-path" readonly value="${escape(prefs.projectDir)}" placeholder="${t('projectPlaceholder')}"><button class="pill pill-outline pill-small" id="pick-folder">${t('chooseFolder')}</button></div>
        <div class="launch-row"><button class="pill pill-ink launch-button" id="launch-${codingTool}" ${tool.ready() ? '' : 'disabled'}>${icon(tool.icon)}${t(tool.launch)}${icon('arrow')}</button><p class="field-help">${t('launchHelp')}</p></div>
        ${installed ? '' : `<button class="text-button" id="${codingTool}-install-info">${t(tool.install)}</button>`}</section>
      <aside class="side-tiles">
        <div class="tile tile-yellow tip-card">${figure('plug')}<h3>${t('stayConnected')}</h3><p>${t(isMac ? 'stayConnectedMac' : 'stayConnectedWindows')}</p></div>
        <section class="tile tile-blue compact-panel">${figure('mug')}<h3>${t('ownTerminal')}</h3>${terminalTile}</section>
      </aside>
    </div></div></div>`;
  el('pick-folder')!.onclick = async () => {
    try { const folder = await open({ directory: true, multiple: false, title: t('chooseFolderTitle'), ...(prefs.projectDir ? { defaultPath: prefs.projectDir } : {}) }); if (typeof folder === 'string') { await savePrefs({ projectDir: folder }); renderCode(); } } catch (e) { toast(String(e), true); }
  };
  const launch = el<HTMLButtonElement>(`launch-${codingTool}`)!;
  launch.onclick = async () => { launch.disabled = true; try { await pendingPreferences; await action(tool.command, undefined, t(tool.launched)); await refreshStatus(); } catch {} finally { if (launch.isConnected) chrome(); } };
  const install = el(`${codingTool}-install-info`);
  if (install) install.onclick = () => void action('open_external', { url: tool.installUrl }).catch(() => {});
}

function usageHtml() {
  const month = usage?.monthlyUsage;
  const percentage = (value: number | undefined) => typeof value === 'number' && Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : undefined;
  const meter = (tone: string, label: string, value: number | undefined, note: string) => `<div class="meter tile-${tone}"><div class="meter-top"><span>${label}</span><strong>${fmt(value)}<small>${value === undefined ? '' : '%'}</small></strong></div><div class="progress"><i style="width:${value ?? 0}%"></i></div><p>${note}</p></div>`;
  const allowance = month?.availableUnits !== undefined ? t('usageRemaining', { available: fmt(month.availableUnits), total: fmt(month.provisionedUnits) }) : t('usagePlan', { total: fmt(month?.provisionedUnits) });
  const detail = refreshing ? t('usageLoading') : usageError ? `${escape(t('usageFailed', { error: usageError }))}${usage ? t('usageStale') : ''}` : !usage ? t('usageAfterSignIn') : '';
  return `<div class="usage-meters">${meter('blue', t('usageMonth'), percentage(month?.usedPercentage), allowance)}${meter('pink', t('usageSession'), percentage(usage?.sessionUsage?.usedPercentage), t('usageLive'))}</div>${detail ? `<p class="field-help" id="usage-status" role="status">${detail}${usageError && !refreshing ? ` <button class="text-button" data-retry-usage>${t('retry')}</button> <button class="text-button" data-diagnostics>${t('copyDiagnostics')}</button>` : ''}</p>` : ''}`;
}

const faqs = (): [string, string][] => [
  [t('faqBackgroundQ'), t(isMac ? 'faqBackgroundMac' : 'faqBackgroundWindows')],
  [t('faqKeyQ'), t('faqKeyA')],
  [t('faqTokensQ'), t('faqTokensA')],
];
let openFaq = 0;
const faqHtml = () => faqs().map(([q, a], i) => `<div class="faq-row ${i === openFaq ? 'open' : ''}"><button class="faq-trigger" data-faq="${i}" aria-expanded="${i === openFaq}" aria-controls="faq-${i}"><span class="faq-index">${String(i + 1).padStart(2, '0')}</span><span class="faq-question">${q}</span><span class="faq-plus" aria-hidden="true"></span></button><div class="faq-answer" id="faq-${i}" role="region"><div><p>${a}</p></div></div></div>`).join('');

const proxyButton = () => `<button class="pill pill-ink" data-toggle-proxy>${icon(status!.running ? 'stop' : 'bolt')}${t(status!.running ? 'stopProxy' : 'startProxy')}</button>`;
const refreshButton = () => `<button class="pill pill-outline" id="refresh-usage" ${refreshing ? 'disabled' : ''}>${icon('refresh', refreshing ? 'spin' : '')}${t('refreshStatus')}</button>`;
const importedCredentials = () => status?.credentials?.source === 'imported';
const accountContents = () => `<div class="account-avatar">${brand}</div><div class="account-copy"><h2>Amazon Quick</h2><p>${status!.account.signedIn ? t(importedCredentials() ? 'usingImported' : 'usingLocal') : escape(status!.account.error || t('signInFirst'))}</p></div><span class="badge ${status!.account.signedIn ? 'on' : 'warn'}">${t(status!.account.signedIn ? 'signedIn' : 'signedOut')}</span>${isMac && !importedCredentials() ? `<button class="pill pill-outline pill-small" data-authorize-quick>${t('authorizeKeychain')}</button>` : ''}<button class="text-button" data-open-quick>${t('openQuick')}</button>`;
const accountHeading = () => `<div class="account-heading" data-account-heading>${accountContents()}</div>`;

function credentialSummary() {
  const source = t(importedCredentials() ? 'credentialSourceImported' : 'credentialSourceLocal');
  if (!status?.account.expiresAt) return `${source}${status?.account.error || ''}`;
  const expiry = new Date(status.account.expiresAt * 1000).toLocaleString(locale());
  return `${source}${t('credentialValidUntil', { time: expiry })}${t(status.account.enterprise ? 'credentialEnterprise' : status.account.canRefresh ? 'credentialRefreshable' : 'credentialReexport')}`;
}

let diagnosing = false;
async function copyDiagnostics() {
  if (diagnosing) return;
  diagnosing = true;
  toast(t('diagnosticsRunning'));
  try {
    const report = await invoke<string>('diagnostics');
    await invoke('copy_text', { text: report });
    toast(t('diagnosticsCopied'));
  } catch (error) { toast(String(error), true); }
  finally { diagnosing = false; }
}

async function credentialAction(mode: string) {
  if (credentialBusy) return;
  if (activeRequest) { toast(t('stopBeforeCredentials'), true); return; }
  const filters = [{ name: t('credentialFileType'), extensions: ['qwc'] }];
  credentialBusy = true;
  credentialRevision++;
  let changedSource = false;
  chrome();
  try {
    if (mode === 'export') {
      const path = await save({ title: t('exportCredentialsTitle'), defaultPath: `QuickWay-${new Date().toISOString().replace(/[:.]/g, '-')}.qwc`, filters });
      if (!path) return;
      await invoke('export_credentials', { path });
      toast(t('credentialsExported'));
    } else if (mode === 'import') {
      const path = await open({ title: t('importCredentialsTitle'), multiple: false, directory: false, filters });
      if (!path || Array.isArray(path)) return;
      await invoke('import_credentials', { path });
      changedSource = true;
      toast(t('credentialsImported'));
    } else if (mode === 'forget') {
      await invoke('forget_credentials');
      changedSource = true;
      toast(t('credentialsForgotten'));
    }
    if (changedSource) { usage = null; usageError = ''; }
  } catch (failure) { toast(String(failure), true); }
  finally {
    credentialBusy = false;
    credentialRevision++;
    await refreshStatus();
    renderView();
    if (changedSource) void refreshUsage();
  }
}
const baseUrl = () => `http://127.0.0.1:${status!.preferences.port}`;
const maskedKey = '••••••••••••';
const shellLabel = isWindows ? 'PowerShell' : 'cURL';

/** Request examples for the local API. `key` is the real key only when copying or revealed. */
function apiExample(format: ApiFormat, language: ApiLanguage, key: string) {
  const base = baseUrl();
  const model = hasSelectedModel() ? status!.preferences.model : t('pickCloudModel');
  if (format === 'openai') {
    if (language === 'python') return `from openai import OpenAI

client = OpenAI(base_url="${base}/v1", api_key="${key}")
reply = client.chat.completions.create(
    model="${model}",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(reply.choices[0].message.content)`;
    if (language === 'javascript') return `import OpenAI from "openai";

const client = new OpenAI({ baseURL: "${base}/v1", apiKey: "${key}" });
const reply = await client.chat.completions.create({
  model: "${model}",
  messages: [{ role: "user", content: "Hello!" }],
});
console.log(reply.choices[0].message.content);`;
    if (isWindows) return `$body = @{
  model = "${model}"
  messages = @(@{ role = "user"; content = "Hello!" })
} | ConvertTo-Json -Depth 5
$reply = Invoke-RestMethod -Method Post -Uri "${base}/v1/chat/completions" \`
  -Headers @{ Authorization = "Bearer ${key}" } \`
  -ContentType "application/json" -Body $body
$reply.choices[0].message.content`;
    return `curl ${base}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${key}" \\
  -d '{
    "model": "${model}",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'`;
  }
  if (language === 'python') return `import anthropic

client = anthropic.Anthropic(base_url="${base}", api_key="${key}")
message = client.messages.create(
    model="${model}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello!"}],
)
print(message.content[0].text)`;
  if (language === 'javascript') return `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({ baseURL: "${base}", apiKey: "${key}" });
const message = await client.messages.create({
  model: "${model}",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello!" }],
});
console.log(message.content[0].text);`;
  if (isWindows) return `$body = @{
  model = "${model}"
  max_tokens = 1024
  messages = @(@{ role = "user"; content = "Hello!" })
} | ConvertTo-Json -Depth 5
$reply = Invoke-RestMethod -Method Post -Uri "${base}/v1/messages" \`
  -Headers @{ "x-api-key" = "${key}"; "anthropic-version" = "2023-06-01" } \`
  -ContentType "application/json" -Body $body
$reply.content[0].text`;
  return `curl ${base}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "x-api-key: ${key}" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{
    "model": "${model}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'`;
}

async function ensureKey() {
  if (!apiKey) apiKey = await invoke<string>('api_key');
  return apiKey;
}

function apiCardHtml() {
  const tab = (attr: string, value: string, label: string, active: boolean) => `<button class="${active ? 'active' : ''}" ${attr}="${value}" aria-pressed="${active}">${label}</button>`;
  const shownKey = revealKey && apiKey ? apiKey : maskedKey;
  return `<div class="api-head"><h2>${t('apiExamples')}</h2><div class="segmented" role="group" aria-label="${t('apiFormat')}">${tab('data-api-format', 'openai', 'OpenAI', apiFormat === 'openai')}${tab('data-api-format', 'anthropic', 'Anthropic', apiFormat === 'anthropic')}</div></div>
    <dl class="api-meta"><div><dt>Base URL</dt><dd><code>${escape(baseUrl())}${apiFormat === 'openai' ? '/v1' : ''}</code></dd></div><div><dt>${t('model')}</dt><dd><code>${escape(hasSelectedModel() ? status!.preferences.model : t('notSelected'))}</code></dd></div></dl>
    <div class="api-tools"><div class="lang-tabs" role="group" aria-label="${t('exampleLanguage')}">${tab('data-api-language', 'shell', shellLabel, apiLanguage === 'shell')}${tab('data-api-language', 'python', 'Python', apiLanguage === 'python')}${tab('data-api-language', 'javascript', 'JavaScript', apiLanguage === 'javascript')}</div><button class="text-button key-toggle" data-reveal-key>${icon(revealKey ? 'eyeOff' : 'eye')}${t(revealKey ? 'hideKey' : 'showKey')}</button><button class="pill pill-outline pill-small" data-copy-example ${hasSelectedModel() ? '' : 'disabled'}>${icon('copy')}${t('copy')}</button></div>
    <pre class="code-block" id="api-code"><code>${escape(apiExample(apiFormat, apiLanguage, shownKey))}</code></pre>
    <p class="field-help">${t('apiHelp')}</p>`;
}

function ccSwitchHtml() {
  const shownKey = revealKey && apiKey ? apiKey : maskedKey;
  return `<div class="ccswitch-top">${figure('plug', 'ccswitch-figure')}<div><span class="eyebrow">CC Switch</span><h2>${t('ccTitle')}</h2></div></div>
    <p class="ccswitch-copy">${t('ccBody')}</p>
    <dl class="import-preview"><div><dt>${t('ccProvider')}</dt><dd>QuickWay</dd></div><div><dt>${t('ccAddress')}</dt><dd>${escape(baseUrl())}</dd></div><div><dt>${t('apiKey')}</dt><dd>${escape(shownKey)}</dd></div><div><dt>${t('defaultModel')}</dt><dd>${escape(hasSelectedModel() ? status!.preferences.model : t('notSelected'))}</dd></div></dl>
    <button class="pill pill-ink ccswitch-import" data-import-ccswitch ${hasSelectedModel() ? '' : 'disabled'}>${icon('external')}${t('ccImport')}</button>
    <p class="field-help">${t('ccHelp')} <button class="text-button" data-open-ccswitch-site>${t('ccDownload')}</button></p>`;
}

function modelListHtml() {
  if (!availableModels().length) return `<p class="field-help">${t('noModelsSignIn')}</p>`;
  return availableModels().map(m => `<div class="model-row"><span class="model-name">${escape(modelName(m.id))}</span><code>${escape(m.id)}</code><button class="subtle-icon" data-copy-model="${escape(m.id)}" title="${t('copyModelId')}" aria-label="${escape(t('copyValue', { value: m.id }))}">${icon('copy')}</button></div>`).join('');
}

function renderDashboard() {
  const prefs = status!.preferences;
  el('workspace')!.innerHTML = `<div class="page-scroll"><div class="page">
    <header class="page-header"${headerDrag}><div><span class="eyebrow">Local API</span><h1>${t('dashboardTitle')}</h1><p>${t('dashboardIntro')}</p></div><div class="page-actions">${refreshButton()}${proxyButton()}</div></header>
    <div class="bento">
      <section class="status-card ${status!.running ? 'is-running' : ''}" data-proxy-state><span class="eyebrow">${t('localProxy')}</span><strong class="status-word" data-proxy-label>${t(status!.running ? 'running' : 'stopped')}</strong><div class="endpoint-row"><code>${escape(baseUrl())}</code><button class="pill pill-outline pill-small" data-copy-base>${icon('copy')}${t('copyAddress')}</button><button class="text-button" id="copy-key">${t('copyApiKey')}</button></div><p class="status-note">${t('sharedEndpoint')}</p>${figure('plug', 'status-figure')}</section>
      <div class="bento-stack">
        <section class="scene port-summary"><span class="eyebrow">${t('listenPort')}</span><strong class="port-big">${prefs.port}</strong><p>${t('localOnly')}</p><button class="text-button" data-go="settings">${t('changePort')}</button></section>
        <section class="models-card"><div class="models-top"><span class="eyebrow">${t('modelCatalog')}</span><strong data-model-count>${availableModels().length}</strong></div><p data-model-summary>${escape(catalogSummary())}</p><div class="models-actions"><button class="pill pill-paper pill-small" data-toggle-models aria-expanded="${showModels}" aria-controls="model-list">${t(showModels ? 'hideModels' : 'showModelIds')}</button><button class="icon-button on-ink" data-refresh-models title="${t('refreshModels')}" aria-label="${t('refreshModels')}">${icon('refresh')}</button></div></section>
      </div>
      <section class="scene model-list" id="model-list" ${showModels ? '' : 'hidden'}>${modelListHtml()}</section>
      <section class="scene recent-model" data-recent-model role="status">${recentModelHtml()}</section>
      <section class="scene account-strip">${accountHeading()}<div id="usage-content">${usageHtml()}</div></section>
    </div>
    <div class="api-split">
      <section class="scene api-card" id="api-card">${apiCardHtml()}</section>
      <section class="scene ccswitch-card" id="ccswitch-card">${ccSwitchHtml()}</section>
    </div>
    </div></div>`;
  el('refresh-usage')!.onclick = () => void refreshUsage(true);
  el('copy-key')!.onclick = () => void action('copy_connection', { keyOnly: true }, t('apiKeyCopied')).catch(() => {});
}

function syncApiCards() {
  const api = el('api-card');
  if (api) api.innerHTML = apiCardHtml();
  const ccSwitch = el('ccswitch-card');
  if (ccSwitch) ccSwitch.innerHTML = ccSwitchHtml();
}

function renderSettings() {
  const prefs = status!.preferences;
  el('workspace')!.innerHTML = `<div class="page-scroll"><div class="page">
    <header class="page-header"${headerDrag}><div><span class="eyebrow">Settings</span><h1>${t('navSettings')}</h1><p>${t('settingsIntro')}</p></div><div class="page-actions">${proxyButton()}</div></header>
    <section class="scene proxy-settings"><div class="panel-title"><h2>${t('localProxy')}</h2><span class="badge ${status!.running ? 'on' : ''}" data-proxy-label>${t(status!.running ? 'running' : 'stopped')}</span></div>
      <div class="endpoint-row"><code>http://127.0.0.1:${prefs.port}</code><button class="pill pill-outline pill-small" data-copy-connection>${icon('copy')}${t('copyTerminalConfig', { terminal: terminalName() })}</button><button class="text-button" id="copy-key">${t('copyApiKey')}</button></div>
      <div class="port-row"><div><strong>${t('listenPort')}</strong><p>${t('portEditable')}</p></div><input class="port-input" id="port" type="number" min="1024" max="65535" value="${prefs.port}" ${status!.running ? 'disabled' : ''} aria-label="${t('listenPort')}"></div>
      <div class="port-row"><div><strong>${t('autoStart')}</strong><p>${t('autoStartNote')}</p></div><label class="toggle"><input id="auto-start" type="checkbox" ${prefs.autoStart ? 'checked' : ''} aria-label="${t('autoStartLabel')}"><span></span></label></div>
    </section>
    <section class="scene account-strip">${accountHeading()}</section>
    <section class="scene credential-settings"><div class="panel-title"><h2>${t('crossMachine')}</h2><span class="badge">${t('noPassword')}</span></div>
      <p id="credential-state">${escape(credentialSummary())}</p><p>${t('crossMachineBody')}</p><p>${t('crossMachineWarning')}</p>
      <div class="credential-actions"><button class="pill pill-ink" data-credentials="export">${t('exportCredentials')}</button><button class="pill pill-outline" data-credentials="import">${t('importCredentials')}</button><button class="text-button" data-credentials="forget" ${importedCredentials() ? '' : 'hidden'}>${t('forgetCredentials')}</button></div>
    </section>
    <section class="scene diagnostics-settings"><div class="panel-title"><h2>${t('diagnosticsTitle')}</h2></div>
      <p>${t('diagnosticsBody')}</p>
      <div class="credential-actions"><button class="pill pill-outline" data-diagnostics>${icon('copy')}${t('copyDiagnostics')}</button></div>
    </section>
    <p class="catalog-help">${t('catalogHelp')}</p>
    <section class="faq"><div class="faq-rail"><span class="eyebrow">Questions</span><h2>${t('faqTitle')}</h2>${figure('q', 'faq-figure')}</div><div class="faq-list">${faqHtml()}</div></section>
    </div></div>`;
  el<HTMLInputElement>('port')!.onchange = async e => { const port = Number((e.target as HTMLInputElement).value); if (!Number.isInteger(port) || port < 1024 || port > 65535) { toast(t('portRange'), true); return; } try { await savePrefs({ port }); const endpoint = document.querySelector('.endpoint-row code'); if (endpoint) endpoint.textContent = `http://127.0.0.1:${port}`; } catch {} };
  el<HTMLInputElement>('auto-start')!.onchange = async e => { try { await savePrefs({ autoStart: (e.target as HTMLInputElement).checked }); } catch { renderSettings(); } };
  el('copy-key')!.onclick = () => void action('copy_connection', { keyOnly: true }, t('apiKeyCopied')).catch(() => {});
}

function renderView() {
  if (!status) { el('workspace')!.innerHTML = `<div class="loading">${figure('q', 'loading-figure')}<p>${t('loadingWorkspace')}</p></div>`; return; }
  document.querySelectorAll('[data-view]').forEach(e => { const active = e.getAttribute('data-view') === view; e.classList.toggle('active', active); if (active) e.setAttribute('aria-current', 'page'); else e.removeAttribute('aria-current'); });
  document.title = `QuickWay · ${t(viewTitleKeys[view])}`;
  document.documentElement.dataset.page = view;
  openPicker = null;
  if (view === 'dashboard') renderDashboard(); else if (view === 'chat') renderChat(); else if (view === 'code') renderCode(); else renderSettings();
  renderRecents();
  chrome();
}

function savePrefs(patch: Partial<Preferences>) {
  pendingPreferences = pendingPreferences.catch(() => {}).then(async () => {
    if (!status) return;
    const preferences = { ...status.preferences, ...patch };
    preferenceRevision++;
    try { await action('save_preferences', { preferences }); status.preferences = preferences; }
    finally { preferenceRevision++; }
    chrome();
  });
  return pendingPreferences;
}
function newConversation() {
  if (!status) return;
  if (activeRequest) { toast(t('stopBeforeNewChat')); return; }
  selected = '';
  draft = '';
  view = 'chat';
  renderView();
  el('prompt')?.focus();
}

async function sendMessage() {
  try { await pendingPreferences; } catch { return; }
  if (activeRequest || !status || !draft.trim()) return;
  const text = draft.trim();
  let conversation = current();
  if (!(conversation?.model || status.preferences.model)) { toast(t('pickModelFirst'), true); return; }
  if (!isKnownModel(conversation?.model || status!.preferences.model)) { toast(t('modelGone'), true); return; }
  if (!conversation) {
    conversation = { id: crypto.randomUUID(), title: text.replace(/\s+/g, ' ').slice(0, 30), model: status.preferences.model, thinkingEffort: effortValue(status.preferences.thinkingEffort), messages: [], updated: Date.now() };
    conversations.unshift(conversation); selected = conversation.id;
  }
  const user: Message = { id: crypto.randomUUID(), role: 'user', content: text };
  conversation.messages.push(user);
  conversation.updated = Date.now();
  const history = conversation.messages.filter(m => m.content && !m.error).map(m => ({ role: m.role, content: m.content }));
  const thinkingEffort = effortValue(conversation.thinkingEffort);
  const assistant: Message = { id: crypto.randomUUID(), role: 'assistant', content: '', model: conversation.model, thinkingEffort };
  conversation.messages.push(assistant);
  const requestId = crypto.randomUUID();
  activeRequest = requestId;
  draft = '';
  persist();
  renderView();
  const channel = new Channel<ChatEvent>();
  channel.onmessage = event => {
    if (event.type === 'delta') assistant.content += event.text;
    if (event.type === 'model' && validServedModel(event.served)) assistant.servedModel = event.served;
    if (event.type === 'usage') { assistant.input = event.input; assistant.output = event.output; }
    if (event.type === 'cancelled') assistant.interrupted = true;
    if (event.type === 'error') assistant.error = event.message;
    if (event.type === 'done' && event.reason === 'max_tokens') assistant.interrupted = true;
    updateMessage(assistant);
  };
  try { await invoke('chat', { model: conversation.model, thinkingEffort, messages: history, requestId, onEvent: channel }); }
  catch (error) { assistant.error = String(error); }
  finally {
    activeRequest = null;
    persist();
    renderView();
    await refreshStatus();
    el('prompt')?.focus();
  }
}

function updateMessage(message: Message) {
  const target = el(`message-${message.id}`);
  const scroll = el('chat-scroll');
  if (!target || !scroll) return;
  const stick = scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 140;
  target.outerHTML = messageHtml(message);
  const modelStatus = el('chat-model-status');
  if (modelStatus) modelStatus.innerHTML = actualModelHtml(message);
  if (stick) scroll.scrollTop = scroll.scrollHeight;
}

async function refreshStatus() {
  if (authorizing || credentialBusy) return status;
  const revision = preferenceRevision;
  const sourceRevision = credentialRevision;
  try {
    const wasSignedIn = !!status?.account.signedIn;
    const next = await invoke<Status>('get_status');
    await pendingPreferences.catch(() => {});
    if (sourceRevision !== credentialRevision) return status;
    if (status && revision !== preferenceRevision) next.preferences = status.preferences;
    const changed = JSON.stringify(status?.models) !== JSON.stringify(next.models);
    status = next;
    if (!next.account.signedIn && wasSignedIn) { usage = null; usageError = ''; syncUsage(); }
    if (changed) { updateModelChoices(); const list = el('model-list'); if (list && !list.hidden) list.innerHTML = modelListHtml(); syncApiCards(); }
    chrome();
    // Login can become readable after startup, including after Keychain authorization.
    if (next.account.signedIn && !wasSignedIn && !usage) void refreshUsage();
    return status;
  } catch (error) { toast(t('statusFailed', { error: String(error) }), true); return null; }
}
async function refreshModels() {
  if (refreshingModels) return;
  refreshingModels = true;
  chrome();
  try {
    const catalog = await invoke<ModelCatalog & { models: Model[] }>('refresh_models');
    await refreshStatus();
    toast(catalog.error || t('catalogUpdated', { count: catalog.models.filter(m => Object.hasOwn(tierKeys, m.id)).length }), !!catalog.error);
  } catch (error) { toast(String(error), true); }
  finally { refreshingModels = false; chrome(); }
}

async function refreshUsage(notify = false) {
  if (refreshing) return;
  refreshing = true;
  const sourceRevision = credentialRevision;
  syncUsage();
  try {
    const next = await invoke<Usage | null>('refresh_usage');
    if (sourceRevision !== credentialRevision) return;
    if (!next || typeof next !== 'object' || (!next.monthlyUsage && !next.sessionUsage)) throw new Error(t('usageEmpty'));
    usage = next;
    usageError = '';
    await refreshStatus();
    if (notify) toast(t('usageUpdated'));
  } catch (error) { if (sourceRevision === credentialRevision) { usageError = String(error); if (notify) toast(usageError, true); } }
  finally { refreshing = false; syncUsage(); }
}
function syncUsage() {
  const box = el('usage-content');
  if (box) box.innerHTML = usageHtml();
  const button = el<HTMLButtonElement>('refresh-usage');
  if (button) { button.disabled = refreshing; button.innerHTML = `${icon('refresh', refreshing ? 'spin' : '')}${t('refreshStatus')}`; }
}

document.addEventListener('click', e => {
  const target = e.target as HTMLElement;
  const pickerRoot = target.closest<HTMLElement>('[data-picker]');
  // A pick can rebuild the popover before this runs, so a detached target is not an outside click.
  if (openPicker && target.isConnected && pickerRoot?.dataset.picker !== openPicker) closePicker();
  if (pickerRoot) {
    const scope = pickerRoot.dataset.picker as PickerScope;
    if (target.closest('.mode-trigger')) setPicker(openPicker === scope ? null : scope);
    const option = target.closest<HTMLElement>('[data-model-option]')?.dataset.modelOption;
    if (option) void pickModel(scope, option);
    const stop = target.closest<HTMLElement>('[data-effort-stop]')?.dataset.effortStop;
    if (stop) void pickEffort(scope, effortValue(stop));
  }
  const credentialMode = target.closest<HTMLElement>('[data-credentials]')?.dataset.credentials;
  if (credentialMode) void credentialAction(credentialMode).catch(error => toast(String(error), true));
  // Sidebar items carry data-view; in-page links use data-go so they never take the nav highlight.
  const navigation = target.closest<HTMLElement>('[data-view], [data-go]');
  if (navigation) { view = (navigation.dataset.view || navigation.dataset.go) as View; renderView(); }
  const tool = target.closest<HTMLElement>('[data-coding-tool]')?.dataset.codingTool as CodingTool | undefined;
  if (tool && tool !== codingTool) { codingTool = tool; try { localStorage.setItem('quickway.codingTool', tool); } catch {} openPicker = null; renderCode(); chrome(); }
  const recent = target.closest<HTMLElement>('[data-conversation]');
  if (recent) { if (activeRequest && recent.dataset.conversation !== selected) { toast(t('stopBeforeSwitch')); return; } selected = recent.dataset.conversation!; draft = ''; view = 'chat'; renderView(); }
  const suggestion = target.closest<HTMLElement>('[data-prompt]');
  if (suggestion) { draft = suggestion.dataset.prompt!.replace(/\\n/g, '\n'); const prompt = el<HTMLTextAreaElement>('prompt')!; prompt.value = draft; resizePrompt(); prompt.focus(); }
  if (target.closest('[data-authorize-quick]') && !authorizing) {
    authorizing = true; chrome();
    void action('authorize_quick').then(async () => { await action('refresh_models'); await refreshUsage(); toast(t('quickAuthorized')); }).catch(() => {}).finally(async () => { authorizing = false; await refreshStatus(); renderView(); });
  }
  if (target.closest('[data-refresh-models]')) void refreshModels();
  if (target.closest('[data-retry-usage]')) void refreshUsage(true);
  if (target.closest('[data-diagnostics]')) void copyDiagnostics();
  if (target.closest('[data-copy-codex-config]')) void pendingPreferences.then(() => action('copy_codex_config', undefined, t('codexConfigCopied'))).catch(() => {});
  if (target.closest('[data-copy-connection]')) void pendingPreferences.then(() => action('copy_connection', { keyOnly: false }, t('terminalConfigCopied', { terminal: terminalName() }))).catch(() => {});
  if (target.closest('[data-open-quick]')) void action('open_quick').catch(() => {});
  const toggle = target.closest<HTMLButtonElement>('[data-toggle-proxy]');
  if (toggle && status) { toggle.disabled = true; void pendingPreferences.then(() => action(status!.running ? 'stop_proxy' : 'start_proxy')).then(refreshStatus).then(renderView).catch(() => {}).finally(() => { if (toggle.isConnected) toggle.disabled = false; }); }
  const copy = target.closest<HTMLElement>('[data-copy-message]');
  if (copy) { const message = current()?.messages.find(m => m.id === copy.dataset.copyMessage); if (message) void action('copy_text', { text: message.content }, t('replyCopied')).catch(() => {}); }
  if (target.closest('#copy-chat')) { const conversation = current(); if (conversation) void action('copy_text', { text: conversation.messages.map(m => `${m.role === 'user' ? t('you') : modelName(m.model)}\n${m.content}`).join('\n\n') }, t('chatCopied')).catch(() => {}); }
  const windowAction = target.closest<HTMLElement>('[data-window]')?.dataset.window;
  if (windowAction) {
    try {
      const current = getCurrentWindow();
      // Close goes through CloseRequested, which hides QuickWay to the tray like the system button did.
      void (windowAction === 'minimize' ? current.minimize() : windowAction === 'maximize' ? current.toggleMaximize() : current.close()).catch(error => toast(String(error), true));
    } catch (error) { toast(String(error), true); }
  }
  const format = target.closest<HTMLElement>('[data-api-format]')?.dataset.apiFormat;
  if (format) { apiFormat = format as ApiFormat; syncApiCards(); }
  const language = target.closest<HTMLElement>('[data-api-language]')?.dataset.apiLanguage;
  if (language) { apiLanguage = language as ApiLanguage; syncApiCards(); }
  if (target.closest('[data-reveal-key]')) {
    if (revealKey) { revealKey = false; syncApiCards(); }
    else void ensureKey().then(() => { revealKey = true; syncApiCards(); }).catch(error => toast(String(error), true));
  }
  // Copies always carry the real key, even while it is hidden on screen.
  if (target.closest('[data-copy-example]')) void pendingPreferences.then(ensureKey).then(key => action('copy_text', { text: apiExample(apiFormat, apiLanguage, key) }, t('exampleCopied'))).catch(() => {});
  if (target.closest('[data-copy-base]') && status) void action('copy_text', { text: baseUrl() }, t('addressCopied')).catch(() => {});
  const modelId = target.closest<HTMLElement>('[data-copy-model]')?.dataset.copyModel;
  if (modelId) void action('copy_text', { text: modelId }, t('copiedValue', { value: modelId })).catch(() => {});
  if (target.closest('[data-toggle-models]')) {
    showModels = !showModels;
    const list = el('model-list');
    if (list) { list.hidden = !showModels; list.innerHTML = modelListHtml(); }
    const button = target.closest<HTMLElement>('[data-toggle-models]')!;
    button.textContent = t(showModels ? 'hideModels' : 'showModelIds');
    button.setAttribute('aria-expanded', String(showModels));
  }
  const importButton = target.closest<HTMLButtonElement>('[data-import-ccswitch]');
  if (importButton) {
    importButton.disabled = true;
    // A model picked a moment ago must be saved before the link is built from preferences.
    void pendingPreferences.then(() => action('import_cc_switch', undefined, t('ccOpened'))).catch(() => {}).finally(() => { if (importButton.isConnected) importButton.disabled = false; });
  }
  if (target.closest('[data-open-ccswitch-site]')) void action('open_external', { url: 'https://github.com/farion1231/cc-switch' }).catch(() => {});
  const faq = target.closest<HTMLElement>('[data-faq]');
  if (faq) {
    const index = Number(faq.dataset.faq);
    openFaq = openFaq === index ? -1 : index;
    document.querySelectorAll<HTMLElement>('.faq-row').forEach((row, i) => { row.classList.toggle('open', i === openFaq); row.querySelector('.faq-trigger')!.setAttribute('aria-expanded', String(i === openFaq)); });
  }
  const link = target.closest<HTMLAnchorElement>('.markdown a');
  if (link) { e.preventDefault(); if (/^https?:\/\//i.test(link.href)) void action('open_external', { url: link.href }).catch(() => {}); }
});
// Tray labels live on the native side; send them whenever the language changes.
function syncTray() {
  void invoke('set_tray_language', { show: t('trayShow'), quit: t('trayQuit'), tooltip: t('trayTooltip') }).catch(() => {});
}
function changeLanguage(next: Language) {
  if (next === language()) return;
  setLanguage(next);
  renderShell();
  renderView();
  syncTray();
  el<HTMLSelectElement>('language')?.focus();
}
document.addEventListener('keydown', e => { if ((isMac ? e.metaKey : e.ctrlKey) && !e.altKey && !e.shiftKey && !e.isComposing && e.keyCode !== 229 && e.key.toLowerCase() === 'n') { e.preventDefault(); newConversation(); } });
renderShell();
renderView();
syncTray();
await refreshStatus();
renderView();
setInterval(() => void refreshStatus(), 7000);
