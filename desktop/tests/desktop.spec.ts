import { test, expect, type Page } from '@playwright/test';
import zh from '../src/locales/zh';
import en from '../src/locales/en';
import ja from '../src/locales/ja';
import { languages } from '../src/i18n';

// The app opens on the API dashboard; chat is a separate test page.
async function openChat(page: Page) {
  await page.locator('[data-view="chat"]').click();
  await expect(page.locator('#prompt')).toBeVisible();
}


type Scope = 'chat' | 'claude' | 'codex';
// The model and thinking effort share one popover per page: effort levels on top, the model list below.
async function openPicker(page: Page, scope: Scope) {
  const trigger = page.locator(`#${scope}-picker`);
  if (await trigger.getAttribute('aria-expanded') !== 'true') await trigger.click();
  await expect(page.locator(`#${scope}-picker-pop`)).toBeVisible();
  return page.locator(`#${scope}-picker-pop`);
}
async function openModels(page: Page, scope: Scope) {
  const pop = await openPicker(page, scope);
  await expect(pop.locator('.mode-models')).toBeVisible();
  return pop;
}
async function closePicker(page: Page, scope: Scope) {
  const pop = page.locator(`#${scope}-picker-pop`);
  while (await pop.isVisible()) await page.keyboard.press('Escape');
}
async function chooseModel(page: Page, scope: Scope, id: string) {
  const pop = await openModels(page, scope);
  await pop.locator(`[data-model-option="${id}"]`).click();
  await expect(page.locator(`#${scope}-picker`)).toHaveAttribute('data-model', id);
  await closePicker(page, scope);
}
async function chooseEffort(page: Page, scope: Scope, effort: string) {
  const pop = await openPicker(page, scope);
  await pop.locator(`[data-effort-stop="${effort}"]`).click();
  await expect(page.locator(`#${scope}-picker`)).toHaveAttribute('data-effort', effort);
  await closePicker(page, scope);
}
async function modelOptions(page: Page, scope: Scope) {
  return (await openModels(page, scope)).locator('[data-model-option]');
}

// Browser tests exercise the actual frontend with an explicit Tauri IPC fixture.
// Native Keychain, Terminal and HTTP checks are run separately on macOS.
test.beforeEach(async ({ page }, info) => {
  await page.addInitScript(({ mac }) => {
    Object.defineProperty(navigator, 'platform', { value: mac ? 'MacIntel' : 'Win32' });
    const w = window as any;
    w.calls = [];
    w.preferences = { port: 8741, model: 'quick-smart', projectDir: '', autoStart: true, thinkingEffort: 'off', codexModel: '', codexEffort: 'medium' };
    let running = true;
    let finish: (() => void) | undefined;
    let pendingChannel: any;
    w.models = [
      { id: 'quick-smart', name: 'Claude Opus 4.8', family: 'Anthropic', upstream: 'fixture' },
      { id: 'quick-advanced', name: 'GPT-6 Astra', family: 'OpenAI', upstream: 'fixture' },
    ];
    w.modelCatalog = { source: 'cloud', lastUpdated: 1, error: null };
    // Window state for the custom title bar. Event listeners are kept so tests can emit window events.
    w.maximized = false;
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners: { event: string; handler: number }[] = [];
    let nextCallback = 1;
    w.emitWindowEvent = (event: string, payload: unknown) => listeners.filter(l => l.event === event).forEach(l => callbacks.get(l.handler)?.({ event, id: l.handler, payload }));
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
      transformCallback: (callback?: (event: unknown) => void) => { const id = nextCallback++; if (callback) callbacks.set(id, callback); return id; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any = {}) => {
        w.calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        switch (command) {
          case 'get_status': return { preferences: structuredClone(w.preferences), running, credentials: w.credentials || { source: 'local' }, account: { signedIn: w.credentials?.source === 'imported' ? !w.credentialUnavailable : !w.lockedKeychain, canRefresh: true, expiresAt: 2000000000, enterprise: true, error: w.credentialUnavailable ? '导入凭证文件无法读取' : w.lockedKeychain ? '钥匙串尚未授权' : null }, models: structuredClone(w.models), lastCloudModel: w.lastCloudModel || null, modelCatalog: structuredClone(w.modelCatalog), claudeInstalled: true, codexInstalled: w.codexInstalled ?? true, version: '0.3.0' };
          case 'export_credentials': w.exportedPath = args.path; return;
          case 'import_credentials':
            if (w.credentialError) throw w.credentialError;
            w.credentials = { source: 'imported' }; return;
          case 'forget_credentials': w.credentials = { source: 'local' }; return;
          case 'plugin:dialog|save': return w.cancelSave ? null : '/synthetic/QuickWay.qwc';
          case 'authorize_quick': w.lockedKeychain = false; return;
          case 'refresh_models':
            if (w.refreshError) w.modelCatalog = { source: 'cache', lastUpdated: 1, error: '网络不可用，保留上次目录' };
            else { if (w.nextModels) w.models = w.nextModels; w.modelCatalog = { source: 'cloud', lastUpdated: 2, error: null }; }
            return { ...w.modelCatalog, models: structuredClone(w.models) };
          case 'refresh_usage':
            if (w.usageError) throw w.usageError;
            if (Object.hasOwn(w, 'usageResponse')) return structuredClone(w.usageResponse);
            return { monthlyUsage: { usedPercentage: 25, provisionedUnits: 100 }, sessionUsage: { usedPercentage: 10 } };
          case 'save_preferences':
            if (w.delayPreferenceSaves) await new Promise<void>(resolve => { w.releasePreferences = resolve; });
            w.preferences = structuredClone(args.preferences); return;
          case 'plugin:dialog|open': return args.options?.directory ? (mac ? "/Users/测试/项目 'quoted' $(echo inert)" : "C:\\项目\\测试") : (w.cancelOpen ? null : '/synthetic/QuickWay.qwc');
          case 'start_proxy': running = true; w.startedPort = w.preferences.port; return;
          case 'stop_proxy': running = false; return;
          case 'copy_text': w.copied = args.text; return;
          case 'diagnostics': if (w.diagnosticsError) throw w.diagnosticsError; return '{"report":"quickway.diagnostics"}';
          case 'check_update':
            if (w.updateError) throw w.updateError;
            return { current: '0.3.0', version: null, notes: null, installable: true, url: 'https://github.com/hnewcity/QuickWay/releases/latest', ...w.update };
          case 'install_update':
            if (w.installError) throw w.installError;
            // A real install exits the app, so the call never settles; tests drive the channel from here.
            w.updateChannel = args.onEvent;
            args.onEvent.onmessage({ type: 'progress', downloaded: 512, total: 2048 });
            return new Promise<void>(() => {});
          case 'set_tray_language': w.tray = structuredClone(args); return;
          case 'chat':
            if (args.messages.at(-1).content === 'hold') {
              pendingChannel = args.onEvent;
              args.onEvent.onmessage({ type: 'delta', text: '正在生成' });
              return new Promise<void>(resolve => { finish = resolve; });
            }
            args.onEvent.onmessage({ type: 'delta', text: '**模拟回答** <script>window.xss = true</script>' });
            const served = Object.hasOwn(w, 'servedModel') ? w.servedModel : 'us.anthropic.claude-opus-5-5';
            if (served !== null) args.onEvent.onmessage({ type: 'model', served });
            w.lastCloudModel = { mode: args.model.replace('quick-', ''), servedModel: served, at: 1794458793 };
            args.onEvent.onmessage({ type: 'usage', input: 12, output: 8 });
            args.onEvent.onmessage({ type: 'done', reason: 'end_turn' });
            return;
          case 'cancel_chat': pendingChannel.onmessage({ type: 'cancelled' }); finish?.(); return;
          case 'launch_claude': w.launched = structuredClone(w.preferences); return;
          case 'launch_codex': w.codexLaunched = structuredClone(w.preferences); return;
          case 'copy_codex_config': w.codexConfigCopies = (w.codexConfigCopies || 0) + 1; return;
          case 'copy_connection': return;
          case 'api_key': return 'fixture-key';
          case 'import_cc_switch': if (w.ccSwitchMissing) throw '没有找到 CC Switch。请先安装 CC Switch，再点导入。'; w.ccSwitchImports = (w.ccSwitchImports || 0) + 1; return;
          case 'open_external': w.opened = args.url; return;
          case 'plugin:event|listen': listeners.push({ event: args.event, handler: args.handler }); return args.handler;
          case 'plugin:event|unlisten': return;
          case 'plugin:window|is_maximized': return w.maximized;
          case 'plugin:window|toggle_maximize': w.maximized = !w.maximized; w.emitWindowEvent('tauri://resize', { width: 1, height: 1 }); return;
          case 'plugin:window|minimize': case 'plugin:window|close': return;
          default: throw new Error(`Unexpected IPC: ${command}`);
        }
      },
    };
  }, { mac: info.project.name.startsWith('mac') });
  await page.goto('/');
  await expect(page.locator('[data-view="dashboard"]')).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.status-word')).toHaveText('运行中');
  await openChat(page);
});

test('Chinese composition, Enter, history and platform new-chat shortcut', async ({ page }, info) => {
  const prompt = page.locator('#prompt');
  await prompt.fill('中文输入');
  await prompt.dispatchEvent('keydown', { key: 'Enter', isComposing: true });
  await prompt.dispatchEvent('keydown', { key: 'Enter', keyCode: 229 });
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'chat').length)).toBe(0);
  await prompt.press('Enter');
  await expect(page.locator('.assistant .message-content')).toContainText('模拟回答');
  expect(await page.evaluate(() => (window as any).xss)).toBeUndefined();
  await page.keyboard.press(info.project.name.startsWith('mac') ? 'Meta+n' : 'Control+n');
  await expect(page.locator('.messages')).toHaveCount(0);
  await expect(prompt).toBeFocused();
  await page.locator('.recent-item').click();
  await expect(page.locator('.assistant .message-content')).toContainText('模拟回答');
  await page.reload();
  await openChat(page);
  await expect(page.locator('.assistant .message-content')).toContainText('模拟回答');
});

test('actual model is confirmed by the response and persists independently of the selected tier', async ({ page }) => {
  await expect(page.locator('#chat-model-status')).toContainText(zh.actualModelIdle);
  await page.evaluate(() => { (window as any).servedModel = 'us.anthropic.claude-opus-4-8'; });
  await page.locator('#prompt').fill('model metadata');
  await page.locator('#send').click();
  await expect(page.locator('.assistant .actual-model')).toContainText('Claude Opus 4.8');
  await expect(page.locator('.assistant .actual-model')).toHaveAttribute('title', 'us.anthropic.claude-opus-4-8');
  await expect(page.locator('.assistant .message-author')).toContainText(zh.tierSmart);
  await chooseModel(page, 'chat', 'quick-advanced');
  await expect(page.locator('#chat-model-status')).toContainText('Claude Opus 4.8');
  await page.reload();
  await openChat(page);
  await expect(page.locator('.assistant .actual-model')).toContainText('Claude Opus 4.8');
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-advanced');
  await page.locator('#new-chat').click();
  await expect(page.locator('#chat-model-status')).toContainText(zh.actualModelIdle);
  await page.locator('#prompt').fill('hold');
  await page.locator('#send').click();
  await expect(page.locator('#chat-model-status')).toContainText(zh.actualModelPending);
  await expect(page.locator('#chat-model-status')).not.toContainText('Opus');
  await page.locator('#send').click();
  await expect(page.locator('#chat-model-status')).toContainText(zh.actualModelUnknown);
});

test('missing and malformed model metadata stays unknown instead of using the requested tier', async ({ page }) => {
  for (const served of [null, '<img src=x onerror="window.modelXss=true">', 'https://invalid/token']) {
    await page.evaluate(served => { (window as any).servedModel = served; }, served);
    await page.locator('#new-chat').click();
    await page.locator('#prompt').fill('no confirmed model');
    await page.locator('#send').click();
    await expect(page.locator('.assistant .actual-model')).toContainText(zh.actualModelUnknown);
    await expect(page.locator('#chat-model-status')).not.toContainText('Claude');
  }
  expect(await page.evaluate(() => (window as any).modelXss)).toBeUndefined();
});

test('latest proxy model is shown on dashboard and coding page without changing the tier', async ({ page }) => {
  await page.clock.install();
  await page.reload();
  await page.evaluate(() => { (window as any).lastCloudModel = { mode: 'fast', servedModel: 'us.anthropic.claude-haiku-5-5', at: 1794458793 }; });
  await page.clock.runFor(7100);
  await page.locator('[data-view="dashboard"]').click();
  await expect(page.locator('[data-recent-model]')).toContainText('Claude Haiku 5.5');
  await expect(page.locator('[data-recent-model]')).toContainText(zh.tierFast);
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('[data-recent-model]')).toContainText('Claude Haiku 5.5');
  await expect(page.locator('#claude-picker')).toHaveAttribute('data-model', 'quick-smart');
  await page.evaluate(() => { (window as any).lastCloudModel = null; });
  await page.clock.runFor(7100);
  await expect(page.locator('[data-recent-model]')).toContainText(zh.actualModelIdle);
});

test('old fixed selections and history require a tier choice and never invent the served model', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).preferences.model = 'claude-sonnet-5-5';
    localStorage.setItem('quickway.conversations.v1', JSON.stringify([{ id: 'old', title: 'Old chat', model: 'claude-sonnet-5-5', updated: 1, messages: [
      { id: 'old-reply', role: 'assistant', model: 'claude-sonnet-5-5', content: 'An old reply' },
    ] }]));
  });
  await page.reload();
  await openChat(page);
  await expect(page.locator('#chat-picker')).toContainText(zh.chooseModel);
  await expect(page.locator('.assistant .actual-model')).toContainText(zh.actualModelUnknown);
  await expect(page.locator('#workspace')).not.toContainText('claude-sonnet-5-5');
  await page.locator('#prompt').fill('continue');
  await page.locator('#send').click();
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'chat'))).toBe(false);
  await chooseModel(page, 'chat', 'quick-smart');
  await page.locator('#send').click();
  await expect(page.locator('.assistant .actual-model').last()).toContainText('Claude Opus 5.5');
});

test('folder selection, launch preferences and platform connection copy', async ({ page }, info) => {
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('#launch-claude')).toBeDisabled();
  await page.locator('#pick-folder').click();
  await expect(page.locator('#project-path')).not.toHaveValue('');
  await chooseModel(page, 'claude', 'quick-advanced');
  await chooseEffort(page, 'claude', 'high');
  await page.locator('#launch-claude').click();
  expect(await page.evaluate(() => (window as any).launched)).toMatchObject({ model: 'quick-advanced', thinkingEffort: 'high' });
  await page.locator('[data-copy-connection]').click();
  await expect(page.locator('#toast')).toContainText(info.project.name.startsWith('mac') ? '终端 连接配置已复制' : 'PowerShell 连接配置已复制');
});

test('Coding page switches between Claude Code and Codex, each with its own model and effort', async ({ page }) => {
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('[data-view="code"]')).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.side-nav .nav-item')).toHaveCount(4);
  await expect(page.locator('[data-coding-tool="claude"]')).toHaveAttribute('aria-pressed', 'true');
  await page.locator('[data-coding-tool="codex"]').click();
  await expect(page.locator('[data-coding-tool="codex"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#launch-claude')).toHaveCount(0);
  // With no Codex choice yet, the picker follows the shared default model.
  await expect(page.locator('#codex-picker')).toHaveAttribute('data-model', 'quick-smart');
  await expect(page.locator('#codex-picker')).toHaveAttribute('data-effort', 'medium');
  await expect(page.locator('#launch-codex')).toBeDisabled();
  await page.locator('#pick-folder').click();
  await expect(page.locator('#launch-codex')).toBeEnabled();
  await chooseModel(page, 'codex', 'quick-advanced');
  await chooseEffort(page, 'codex', 'high');
  await page.locator('#launch-codex').click();
  await expect(page.locator('#toast')).toContainText(zh.codexLaunched);
  expect(await page.evaluate(() => (window as any).codexLaunched)).toMatchObject({ codexModel: 'quick-advanced', codexEffort: 'high' });
  // Claude Code's choices are untouched, and the project folder is shared.
  const prefs = await page.evaluate(() => (window as any).preferences);
  expect(prefs).toMatchObject({ model: 'quick-smart', thinkingEffort: 'off' });
  await page.locator('[data-coding-tool="claude"]').click();
  await expect(page.locator('#claude-picker')).toHaveAttribute('data-model', 'quick-smart');
  await expect(page.locator('#project-path')).not.toHaveValue('');
  await expect(page.locator('#launch-claude')).toBeEnabled();
  await expect(page.locator('[data-copy-codex-config]')).toHaveCount(0);
  await page.locator('[data-coding-tool="codex"]').click();
  await expect(page.locator('#codex-picker')).toHaveAttribute('data-model', 'quick-advanced');
  await page.locator('[data-copy-codex-config]').click();
  await expect(page.locator('#toast')).toContainText(zh.codexConfigCopied);
  expect(await page.evaluate(() => (window as any).codexConfigCopies)).toBe(1);
  // The last tool picked is still selected after a restart.
  await page.reload();
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('[data-coding-tool="codex"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#launch-codex')).toBeVisible();
});

test('Quick cloud modes preserve saved selection and launch with Claude Code and Codex', async ({ page }) => {
  const modes = ['fast', 'balanced', 'smart', 'advanced'];
  await page.evaluate(modes => {
    const w = window as any;
    w.nextModels = [...w.models, ...modes.map(mode => ({
      id: `quick-${mode}`, name: `Quick ${mode} (cloud)`, family: 'Amazon Quick', upstream: `quick-cloud:${mode}`,
    }))];
  }, modes);
  await page.locator('[data-refresh-models]').click();
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-smart');
  const pop = await openModels(page, 'chat');
  await expect(pop.locator('.mode-family').first()).toHaveAttribute('aria-label', 'Amazon Quick');
  expect(await pop.locator('[data-model-option]').evaluateAll(options => options.slice(0, 4).map(option => option.getAttribute('data-model-option'))))
    .toEqual(modes.map(mode => `quick-${mode}`));
  await closePicker(page, 'chat');
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('#claude-picker')).toHaveAttribute('data-model', 'quick-smart');
  await chooseModel(page, 'claude', 'quick-smart');
  await page.locator('#pick-folder').click();
  await page.locator('#launch-claude').click();
  await expect.poll(() => page.evaluate(() => (window as any).launched?.model)).toBe('quick-smart');
  await page.locator('[data-coding-tool="codex"]').click();
  await expect(page.locator('#codex-picker')).toHaveAttribute('data-model', 'quick-smart');
  await chooseModel(page, 'codex', 'quick-smart');
  await page.locator('#launch-codex').click();
  await expect.poll(() => page.evaluate(() => (window as any).codexLaunched?.codexModel)).toBe('quick-smart');
});

test('Codex launch stays disabled until Codex is installed', async ({ page }) => {
  // Init scripts rerun on reload, after the fixture, so this survives it.
  await page.addInitScript(() => { (window as any).codexInstalled = false; });
  await page.reload();
  await page.locator('[data-view="code"]').click();
  await page.locator('[data-coding-tool="codex"]').click();
  await page.locator('#pick-folder').click();
  await expect(page.locator('.launch-panel .badge')).toHaveText(zh.codexMissing);
  await expect(page.locator('#launch-codex')).toBeDisabled();
  await page.locator('#codex-install-info').click();
  await expect.poll(() => page.evaluate(() => (window as any).opened)).toBe('https://developers.openai.com/codex/cli');
  // Claude Code is unaffected by a missing Codex.
  await page.locator('[data-coding-tool="claude"]').click();
  await expect(page.locator('.launch-panel .badge')).toHaveText(zh.claudeDetected);
  await expect(page.locator('#launch-claude')).toBeEnabled();
});

test('proxy stop, port change and restart', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('#port')).toBeDisabled();
  await page.locator('[data-toggle-proxy]').click();
  await expect(page.locator('#port')).toBeEnabled();
  await page.locator('#port').fill('8877');
  // The click itself blurs the input; saving must not replace its button before mouseup.
  await page.locator('[data-toggle-proxy]').click();
  await expect(page.locator('#port')).toBeDisabled();
  await expect(page.locator('.endpoint-row code')).toHaveText('http://127.0.0.1:8877');
  expect(await page.evaluate(() => (window as any).startedPort)).toBe(8877);
  await expect(page.locator('#sidebar-endpoint')).toHaveText('127.0.0.1:8877');
});

test('starting immediately after editing a port waits for its pending save', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await page.locator('[data-toggle-proxy]').click();
  await expect(page.locator('#port')).toBeEnabled();
  await page.evaluate(() => { (window as any).delayPreferenceSaves = true; });
  await page.locator('#port').fill('8877');
  await page.locator('[data-toggle-proxy]').click();
  await expect.poll(() => page.evaluate(() => typeof (window as any).releasePreferences)).toBe('function');
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'start_proxy'))).toBe(false);
  await page.evaluate(() => { (window as any).releasePreferences(); });
  await expect.poll(() => page.evaluate(() => (window as any).startedPort)).toBe(8877);
  await expect(page.locator('#sidebar-endpoint')).toHaveText('127.0.0.1:8877');
});

test('stream cancellation and clipboard', async ({ page }) => {
  await page.locator('#prompt').fill('hold');
  await page.locator('#prompt').press('Enter');
  await expect(page.locator('#send')).toHaveAttribute('aria-label', '停止生成');
  await page.locator('#send').click();
  await expect(page.locator('#send')).toHaveAttribute('aria-label', '发送消息');
  await page.locator('[data-copy-message]').click();
  expect(await page.evaluate(() => (window as any).copied)).toBe('正在生成');
});

test('minimum supported window keeps composer and controls visible', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 640 });
  await expect(page.locator('#prompt')).toBeInViewport();
  await expect(page.locator('#send')).toBeInViewport();
  await expect(page.locator('#chat-picker')).toBeInViewport();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('picker shows effort levels and models together, follows the keyboard and saves each choice', async ({ page }) => {
  const saved = () => page.evaluate(() => (window as any).preferences.thinkingEffort);
  const trigger = page.locator('#chat-picker');
  await expect(trigger).toContainText('不思考');
  const pop = await openPicker(page, 'chat');
  const levels = pop.locator('[data-effort-stop]');
  await expect(levels).toHaveText(['关闭', '低', '中', '高']);
  await expect(pop.locator('[data-model-option]')).toHaveCount(2);
  const off = page.locator('#chat-effort-off');
  await expect(off).toBeFocused();
  await expect(off).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#chat-effort-low')).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('#chat-effort-low')).toBeFocused();
  await expect.poll(saved).toBe('low');
  await page.keyboard.press('End');
  await expect.poll(saved).toBe('high');
  await expect(trigger).toContainText('思考 · 高');
  await expect(pop.locator('.mode-hint')).toHaveText('深入推理，耗时和用量最多');
  await pop.locator('[data-effort-stop="medium"]').click();
  await expect.poll(saved).toBe('medium');
  await expect(pop).toBeVisible();
  // ↓ moves from the levels into the list, starting at the current model; ↑ from the top goes back.
  await page.locator('#chat-effort-medium').focus();
  await page.keyboard.press('ArrowDown');
  await expect(pop.locator('[data-model-option="quick-smart"]')).toBeFocused();
  await expect(pop.locator('[data-model-option="quick-smart"]')).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ArrowUp');
  await expect(page.locator('#chat-effort-medium')).toBeFocused();
  // Picking a model closes the popover and returns focus to the pill.
  await pop.locator('[data-model-option="quick-advanced"]').click();
  await expect(pop).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).preferences.model)).toBe('quick-advanced');
  // Clicking outside closes it; Escape closes and returns focus to the pill.
  await trigger.click();
  await expect(pop.locator('[data-model-option="quick-advanced"]')).toHaveAttribute('aria-selected', 'true');
  await page.locator('#prompt').click();
  await expect(pop).toBeHidden();
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await trigger.click();
  await page.keyboard.press('Escape');
  await expect(pop).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(trigger).toContainText(zh.tierAdvanced);
  await expect(trigger).toContainText('思考 · 中');
});

test('dynamic refresh adds models safely without losing draft, selection or launch choice', async ({ page }) => {
  const name = 'Claude <img src=x onerror="window.catalogXss=true"> & new';
  await page.locator('#prompt').fill('保留这段草稿');
  await page.evaluate(name => { const w = window as any; w.nextModels = [...w.models, { id: 'quick-balanced', name, family: 'Anthropic', upstream: 'fixture' }]; }, name);
  await page.locator('[data-refresh-models]').click();
  await expect(await modelOptions(page, 'chat')).toHaveCount(3);
  await closePicker(page, 'chat');
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-smart');
  await expect(page.locator('#prompt')).toHaveValue('保留这段草稿');
  await expect((await openModels(page, 'chat')).locator('[data-model-option="quick-balanced"] .mode-option-name')).toHaveText(zh.tierBalanced);
  await closePicker(page, 'chat');
  expect(await page.evaluate(() => (window as any).catalogXss)).toBeUndefined();
  await chooseModel(page, 'chat', 'quick-balanced');
  await page.locator('#send').click();
  await expect.poll(() => page.evaluate(() => (window as any).calls.find((c: any) => c.command === 'chat')?.args.model)).toBe('quick-balanced');
  await page.locator('[data-view="code"]').click();
  await expect(page.locator('#claude-picker')).toHaveAttribute('data-model', 'quick-balanced');
  await page.locator('#pick-folder').click();
  await page.locator('#launch-claude').click();
  expect(await page.evaluate(() => (window as any).launched.model)).toBe('quick-balanced');
});

test('removed selection remains explicit and cannot silently send to another model', async ({ page }) => {
  await chooseModel(page, 'chat', 'quick-advanced');
  await page.locator('#prompt').fill('保留草稿');
  await page.evaluate(() => { const w = window as any; w.nextModels = w.models.filter((m: any) => m.id !== 'quick-advanced'); });
  await page.locator('[data-refresh-models]').click();
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-advanced');
  await expect((await openModels(page, 'chat')).locator('.mode-missing')).toContainText('当前目录中不可用');
  await closePicker(page, 'chat');
  await page.locator('#send').click();
  await expect(page.locator('#toast')).toContainText('当前档位已不在目录中');
  await expect(page.locator('#prompt')).toHaveValue('保留草稿');
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'chat'))).toBe(false);
  await chooseModel(page, 'chat', 'quick-smart');
  await page.locator('#send').click();
  await expect(page.locator('.assistant .message-content')).toContainText('模拟回答');
});

test('failed refresh keeps choices and active stream intact', async ({ page }) => {
  await page.locator('#prompt').fill('hold');
  await page.locator('#send').click();
  await expect(page.locator('#chat-picker')).toBeDisabled();
  await page.evaluate(() => { (window as any).refreshError = true; });
  await page.locator('[data-refresh-models]').click();
  await expect(page.locator('[data-model-summary]')).toContainText('沿用上次目录');
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-smart');
  await expect(page.locator('#chat-picker')).toBeDisabled();
  await expect(page.locator('.assistant .message-content')).toContainText('正在生成');
  await page.locator('#send').click();
  await expect(page.locator('#send')).toHaveAttribute('aria-label', '发送消息');
  await expect(await modelOptions(page, 'chat')).toHaveCount(2);
});

test('periodic status updates choices without disturbing focused composer', async ({ page }) => {
  await page.clock.install();
  await page.reload();
  await openChat(page);
  await page.locator('#prompt').fill('定时刷新时的草稿');
  await page.evaluate(() => { (window as any).models.push({id:'quick-fast',name:'Future GPT',family:'OpenAI',upstream:'fixture'}); });
  await page.clock.runFor(7100);
  await expect(page.locator('#prompt')).toBeFocused();
  await expect(page.locator('#prompt')).toHaveValue('定时刷新时的草稿');
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-smart');
  await expect(await modelOptions(page, 'chat')).toHaveCount(3);
});

test('background polling does not request keychain authorization; only the explicit button does', async ({ page }, info) => {
  test.skip(!info.project.name.startsWith('mac'), 'macOS Keychain only');
  await page.clock.install();
  await page.reload();
  await page.evaluate(() => { (window as any).lockedKeychain = true; });
  await page.clock.runFor(7100);
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('.account-heading')).toContainText('钥匙串尚未授权');
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'authorize_quick').length)).toBe(0);
  await page.locator('[data-authorize-quick]').click();
  await expect(page.locator('.account-heading')).toContainText('已登录');
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'authorize_quick').length)).toBe(1);
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'refresh_models').length)).toBe(1);
});

test('credential failure preserves cached new models and selection while showing connection status', async ({ page }) => {
  await page.clock.install();
  await page.reload();
  await openChat(page);
  await page.evaluate(() => {
    const w = window as any;
    w.models.push({ id: 'quick-balanced', name: 'Claude Opus 5.5', family: 'Anthropic', upstream: 'fixture', origin: 'cloud' });
  });
  await page.clock.runFor(7100);
  await chooseModel(page, 'chat', 'quick-balanced');
  await page.locator('#prompt').fill('授权恢复后继续这段草稿');
  await page.evaluate(() => {
    const w = window as any;
    w.lockedKeychain = true;
    w.modelCatalog = { source: 'cache', lastUpdated: 1, error: '钥匙串尚未授权' };
  });
  await page.clock.runFor(7100);
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-balanced');
  await expect(page.locator('[data-model-summary]')).toContainText('沿用上次目录');
  await expect(page.locator('#login-notice')).toBeVisible();
  await expect(page.locator('#login-notice')).toContainText('钥匙串尚未授权');
  await expect(page.locator('#prompt')).toBeFocused();
  await expect(page.locator('#prompt')).toHaveValue('授权恢复后继续这段草稿');
  await expect(await modelOptions(page, 'chat')).toHaveCount(3);
  await closePicker(page, 'chat');
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'authorize_quick'))).toBe(false);
  await page.evaluate(() => { (window as any).lockedKeychain = false; });
  await page.clock.runFor(7100);
  await expect(page.locator('#login-notice')).toBeHidden();
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', 'quick-balanced');
});

test('Windows draws its own caption buttons over a draggable ink band', async ({ page }, info) => {
  test.skip(info.project.name.startsWith('mac'), 'Windows title bar only');
  const calls = () => page.evaluate(() => (window as any).calls.map((c: any) => c.command).filter((c: string) => c.startsWith('plugin:window|')));
  await expect(page.locator('.window-control')).toHaveCount(3);
  await expect(page.locator('.titlebar')).toHaveAttribute('data-tauri-drag-region', '');
  await expect(page.locator('.sidebar-drag')).toHaveAttribute('data-tauri-drag-region', '');
  // The sheet starts right under the 34px band, flush with the caption buttons.
  expect(await page.locator('#workspace').evaluate(e => Math.round(e.getBoundingClientRect().top))).toBe(34);
  const maximize = page.locator('[data-window="maximize"]');
  await expect(maximize).toHaveAttribute('aria-label', '最大化');
  await maximize.click();
  await expect(maximize).toHaveAttribute('aria-label', '还原');
  await maximize.click();
  await expect(maximize).toHaveAttribute('aria-label', '最大化');
  await page.locator('[data-window="minimize"]').click();
  await page.locator('[data-window="close"]').click();
  await expect.poll(calls).toEqual(expect.arrayContaining(['plugin:window|toggle_maximize', 'plugin:window|minimize', 'plugin:window|close']));
  // Headers stay ordinary content on Windows; the band is the drag handle.
  await expect(page.locator('.chat-header')).not.toHaveAttribute('data-tauri-drag-region', /.*/);
});

test('macOS keeps the system traffic lights and leaves room for them', async ({ page }, info) => {
  test.skip(!info.project.name.startsWith('mac'), 'macOS title bar only');
  await expect(page.locator('.window-control')).toHaveCount(0);
  await expect(page.locator('.titlebar')).toHaveCount(0);
  // trafficLightPosition puts the buttons inside the top 38px; the brand starts below them.
  expect(await page.locator('.brand').evaluate(e => e.getBoundingClientRect().top)).toBeGreaterThanOrEqual(46);
  await expect(page.locator('.sidebar-drag')).toHaveAttribute('data-tauri-drag-region', '');
  await expect(page.locator('.chat-header')).toHaveAttribute('data-tauri-drag-region', 'deep');
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('.page-header')).toHaveAttribute('data-tauri-drag-region', 'deep');
});

test('dashboard examples use the live port, model and a hidden key that copies for real', async ({ page }, info) => {
  const mac = info.project.name.startsWith('mac');
  await page.locator('[data-view="dashboard"]').click();
  await expect(page.locator('#recents-panel')).toBeHidden();
  await expect(page.locator('.status-card .endpoint-row code')).toHaveText('http://127.0.0.1:8741');
  const code = page.locator('#api-code');
  await expect(page.locator('[data-api-language="shell"]')).toHaveText(mac ? 'cURL' : 'PowerShell');
  await expect(code).toContainText('http://127.0.0.1:8741/v1/chat/completions');
  await expect(code).toContainText('"quick-smart"');
  await expect(code).toContainText(mac ? 'curl ' : 'Invoke-RestMethod');
  await expect(code).not.toContainText('fixture-key');
  await page.locator('[data-api-format="anthropic"]').click();
  await expect(code).toContainText('/v1/messages');
  await expect(code).toContainText('anthropic-version');
  await page.locator('[data-api-language="python"]').click();
  await expect(code).toContainText('anthropic.Anthropic(base_url="http://127.0.0.1:8741"');
  await page.locator('[data-copy-example]').click();
  await expect.poll(() => page.evaluate(() => (window as any).copied)).toContain('api_key="fixture-key"');
  await page.locator('[data-reveal-key]').click();
  await expect(code).toContainText('fixture-key');
  await expect(page.locator('.import-preview')).toContainText('fixture-key');
  // A changed default model reaches the examples on the next visit.
  await page.locator('[data-view="code"]').click();
  await chooseModel(page, 'claude', 'quick-advanced');
  await page.locator('[data-view="dashboard"]').click();
  await expect(page.locator('#api-code')).toContainText('"quick-advanced"');
  await expect(page.locator('[data-api-language="python"]')).toHaveAttribute('aria-pressed', 'true');
});

test('model IDs can be listed and copied from the dashboard', async ({ page }) => {
  await page.locator('[data-view="dashboard"]').click();
  await expect(page.locator('#model-list')).toBeHidden();
  await page.locator('[data-toggle-models]').click();
  await expect(page.locator('.model-row')).toHaveCount(2);
  await page.locator('[data-copy-model="quick-advanced"]').click();
  await expect.poll(() => page.evaluate(() => (window as any).copied)).toBe('quick-advanced');
  await page.locator('[data-toggle-models]').click();
  await expect(page.locator('#model-list')).toBeHidden();
  await page.locator('[data-go="settings"]').click();
  await expect(page.locator('[data-view="settings"]')).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('#port')).toBeVisible();
});

test('CC Switch import waits for a pending model save and reports a missing app', async ({ page }) => {
  await page.locator('[data-view="code"]').click();
  await page.evaluate(() => { (window as any).delayPreferenceSaves = true; });
  await chooseModel(page, 'claude', 'quick-advanced');
  await page.locator('[data-view="dashboard"]').click();
  await page.locator('[data-import-ccswitch]').click();
  await expect.poll(() => page.evaluate(() => typeof (window as any).releasePreferences)).toBe('function');
  expect(await page.evaluate(() => (window as any).ccSwitchImports)).toBeUndefined();
  await page.evaluate(() => { (window as any).releasePreferences(); (window as any).delayPreferenceSaves = false; });
  await expect.poll(() => page.evaluate(() => (window as any).ccSwitchImports)).toBe(1);
  await expect(page.locator('#toast')).toContainText('请在弹出的窗口里确认导入');
  await page.evaluate(() => { (window as any).ccSwitchMissing = true; });
  await page.locator('[data-import-ccswitch]').click();
  await expect(page.locator('#toast')).toContainText('没有找到 CC Switch');
  await page.locator('[data-open-ccswitch-site]').click();
  await expect.poll(() => page.evaluate(() => (window as any).opened)).toBe('https://github.com/farion1231/cc-switch');
});

test('conversation history only shows on the chat test page', async ({ page }) => {
  await expect(page.locator('#recents-panel')).toBeVisible();
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('#recents-panel')).toBeHidden();
  await expect(page.locator('.page-header h1')).toHaveText('连接设置');
  await page.locator('#new-chat').evaluate((e: HTMLElement) => e.click());
  await expect(page.locator('#prompt')).toBeFocused();
  await expect(page.locator('#recents-panel')).toBeVisible();
});

test('fixed model catalog entries stay hidden in chat, both launchers and dashboard', async ({ page }) => {
  const extra = [
    { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', family: 'Anthropic', upstream: 'us.anthropic.claude-sonnet-5-5' },
    { id: 'gpt-6-astra', name: 'GPT-6 Astra', family: 'OpenAI', upstream: 'us.openai.gpt-6-astra' },
    { id: 'vendor.future-1', name: 'Future', family: 'Vendor <img src=x onerror="window.familyXss=true">', upstream: 'vendor.future-1' },
  ];
  await page.evaluate(extra => { const w = window as any; w.nextModels = [...w.models, ...extra]; }, extra);
  await page.locator('[data-refresh-models]').click();
  await expect(await modelOptions(page, 'chat')).toHaveCount(2);
  await expect(page.locator('#chat-picker-pop')).not.toContainText('Claude Sonnet');
  await expect(page.locator('#chat-picker-pop')).not.toContainText('GPT-6');
  await closePicker(page, 'chat');
  await page.locator('[data-view="code"]').click();
  await expect(await modelOptions(page, 'claude')).toHaveCount(2);
  await closePicker(page, 'claude');
  await page.locator('[data-coding-tool="codex"]').click();
  await expect(await modelOptions(page, 'codex')).toHaveCount(2);
  await closePicker(page, 'codex');
  await page.locator('[data-view="dashboard"]').click();
  await page.locator('[data-toggle-models]').click();
  await expect(page.locator('#model-list .model-row')).toHaveCount(2);
  for (const model of extra) await expect(page.locator('#model-list')).not.toContainText(model.id);
  expect(await page.evaluate(() => (window as any).familyXss)).toBeUndefined();
});

test('successful empty cloud refresh removes all available choices and blocks stale launch/import', async ({ page }) => {
  await page.locator('#prompt').fill('保留草稿');
  await page.evaluate(() => { (window as any).nextModels = []; });
  await page.locator('[data-refresh-models]').click();
  await expect(page.locator('[data-model-summary]')).toContainText('0 个档位 · 已同步');
  await expect(await modelOptions(page, 'chat')).toHaveCount(0);
  await closePicker(page, 'chat');
  await expect((await openModels(page, 'chat')).locator('.mode-missing')).toContainText('当前目录中不可用');
  await closePicker(page, 'chat');
  await page.locator('#send').click();
  await expect(page.locator('#toast')).toContainText('当前档位已不在目录中');
  await expect(page.locator('#prompt')).toHaveValue('保留草稿');
  await page.locator('[data-view="code"]').click();
  await page.locator('#pick-folder').click();
  await expect(page.locator('#launch-claude')).toBeDisabled();
  await page.locator('[data-view="dashboard"]').click();
  await page.locator('[data-toggle-models]').click();
  await expect(page.locator('#model-list .model-row')).toHaveCount(0);
  await expect(page.locator('[data-import-ccswitch]')).toBeDisabled();
  await expect(page.locator('[data-copy-example]')).toBeDisabled();
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'chat' || c.command === 'launch_claude'))).toBe(false);
});

test('an empty saved selection survives unavailable cloud data and recovers on refresh', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    // Preserve an explicitly empty older preference; new native defaults use quick-smart.
    w.preferences.model = '';
    w.models = [];
    w.modelCatalog = { source: 'unavailable', lastUpdated: null, error: '网络不可用' };
  });
  await page.reload();
  await openChat(page);
  await expect(await modelOptions(page, 'chat')).toHaveCount(0);
  await closePicker(page, 'chat');
  await expect((await openModels(page, 'chat')).locator('.mode-missing')).toHaveText('暂无云端档位，请刷新目录');
  await closePicker(page, 'chat');
  await expect(page.locator('[data-model-summary]')).toContainText('尚未获取云端目录');
  await page.locator('#prompt').fill('等待目录');
  await page.locator('#send').click();
  await expect(page.locator('#toast')).toContainText('请先从云端目录选择档位');
  await page.evaluate(() => { (window as any).nextModels = [{ id: 'quick-fast', name: 'Fable 5.1', family: 'Anthropic', upstream: 'us.anthropic.quick-fast' }]; });
  await page.locator('[data-refresh-models]').click();
  await expect(await modelOptions(page, 'chat')).toHaveCount(1);
  await closePicker(page, 'chat');
  await expect(page.locator('#chat-picker')).toHaveAttribute('data-model', '');
  await chooseModel(page, 'chat', 'quick-fast');
  await page.locator('#send').click();
  await expect.poll(() => page.evaluate(() => (window as any).calls.find((c: any) => c.command === 'chat')?.args.model)).toBe('quick-fast');
});

test('authorizing after a signed-out startup also loads usage', async ({ page }, info) => {
  test.skip(!info.project.name.startsWith('mac'), 'macOS Keychain only');
  await page.addInitScript(() => { (window as any).lockedKeychain = true; });
  await page.reload();
  await expect(page.locator('.account-heading')).toContainText('未登录');
  await expect(page.locator('.meter-top strong').first()).toHaveText('—');
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => c.command === 'refresh_usage'))).toBe(false);
  await page.locator('[data-authorize-quick]').click();
  await expect(page.locator('.account-heading')).toContainText('已登录');
  await expect(page.locator('.meter-top strong').first()).toHaveText('25%');
  await expect(page.locator('.meter-top strong').nth(1)).toHaveText('10%');
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'refresh_usage').length)).toBe(1);
});

test('login recovered by status polling loads usage without restarting', async ({ page }) => {
  await page.addInitScript(() => { (window as any).lockedKeychain = true; });
  await page.clock.install();
  await page.reload();
  await expect(page.locator('.meter-top strong').first()).toHaveText('—');
  await page.evaluate(() => { (window as any).lockedKeychain = false; });
  await page.clock.runFor(7100);
  await expect(page.locator('.meter-top strong').first()).toHaveText('25%');
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === 'refresh_usage').length)).toBe(1);
});

test('usage errors stay visible and retry recovers the numbers', async ({ page }) => {
  await page.addInitScript(() => { (window as any).usageError = 'HTTP 503 <img src=x onerror="window.usageXss=true">'; });
  await page.reload();
  await expect(page.locator('#usage-status')).toContainText('额度读取失败：HTTP 503');
  await expect(page.locator('.meter-top strong').first()).toHaveText('—');
  expect(await page.evaluate(() => (window as any).usageXss)).toBeUndefined();
  await page.evaluate(() => { (window as any).usageError = ''; });
  await page.locator('[data-retry-usage]').click();
  await expect(page.locator('.meter-top strong').first()).toHaveText('25%');
  await expect(page.locator('#usage-status')).toHaveCount(0);
});

test('diagnostics report is copied from a usage error and from settings', async ({ page }) => {
  await page.addInitScript(() => { (window as any).usageError = 'Quick session validation returned HTTP 401'; });
  await page.reload();
  await page.locator('#usage-status [data-diagnostics]').click();
  await expect(page.locator('#toast')).toContainText('诊断报告已复制');
  expect(await page.evaluate(() => (window as any).copied)).toBe('{"report":"quickway.diagnostics"}');
  await page.evaluate(() => { const w = window as any; w.copied = undefined; w.diagnosticsError = '生成失败'; });
  await page.locator('[data-go="settings"]').first().click();
  await page.locator('.diagnostics-settings [data-diagnostics]').click();
  await expect(page.locator('#toast')).toContainText('生成失败');
  expect(await page.evaluate(() => (window as any).copied)).toBeUndefined();
});

test('an available update is announced and installs with progress', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('#update-badge')).toHaveText(zh.updateLatest);
  await expect(page.locator('#update-notice')).toBeHidden();
  await page.evaluate(() => { (window as any).update = { version: '0.5.0', notes: '新功能 <b>粗体</b>' }; });
  await page.locator('[data-check-update]').click();
  await expect(page.locator('#update-notice')).toContainText('v0.5.0');
  // Release notes are shown as text, never as markup.
  await expect(page.locator('.update-notes')).toHaveText('新功能 <b>粗体</b>');
  await expect(page.locator('.update-settings')).toContainText(zh.updateInstallBody);
  await page.locator('[data-view="chat"]').click();
  await page.locator('#update-notice').click();
  await expect(page.locator('[data-view="settings"]')).toHaveAttribute('aria-current', 'page');
  await page.locator('[data-install-update]').click();
  await expect(page.locator('.update-state')).toHaveText(zh.updateDownloading.replace('{percent}', '25'));
  await expect(page.locator('[data-install-update]')).toBeDisabled();
  await expect(page.locator('[data-check-update]')).toBeDisabled();
  await page.evaluate(() => (window as any).updateChannel.onmessage({ type: 'installing' }));
  await expect(page.locator('.update-state')).toHaveText(zh.updateInstalling);
});

test('a copy that cannot replace itself links to the release page', async ({ page }) => {
  await page.evaluate(() => { (window as any).update = { version: '0.5.0', installable: false }; });
  await page.locator('[data-view="settings"]').click();
  await page.locator('[data-check-update]').click();
  await expect(page.locator('.update-settings')).toContainText(zh.updateManualBody);
  await expect(page.locator('[data-install-update]')).toHaveCount(0);
  await page.locator('[data-download-update]').click();
  await expect.poll(() => page.evaluate(() => (window as any).opened)).toBe('https://github.com/hnewcity/QuickWay/releases/latest');
});

test('update check failures stay quiet at launch and are reported on request', async ({ page }) => {
  await page.addInitScript(() => { (window as any).updateError = '网络不可用'; });
  await page.reload();
  await expect(page.locator('.status-word')).toHaveText('运行中');
  await expect(page.locator('#toast')).not.toHaveClass(/visible/);
  await page.locator('[data-view="settings"]').click();
  const failed = zh.updateCheckFailed.replace('{error}', '网络不可用');
  await expect(page.locator('.update-state')).toHaveText(failed);
  await page.locator('[data-check-update]').click();
  await expect(page.locator('#toast')).toContainText(failed);
  await page.evaluate(() => { const w = window as any; w.updateError = undefined; w.update = { version: '0.5.0' }; w.installError = '签名校验失败'; });
  await page.locator('[data-check-update]').click();
  await page.locator('[data-install-update]').click();
  await expect(page.locator('#toast')).toContainText('签名校验失败');
  await expect(page.locator('[data-install-update]')).toBeEnabled();
});

test('missing usage percentages stay unknown while real zero and remaining units are shown', async ({ page }) => {
  await page.addInitScript(() => { (window as any).usageResponse = { monthlyUsage: { availableUnits: 1074, provisionedUnits: 1080 }, sessionUsage: { usedPercentage: 0 } }; });
  await page.reload();
  await expect(page.locator('.meter-top strong').first()).toHaveText('—');
  await expect(page.locator('.meter-top strong').nth(1)).toHaveText('0%');
  await expect(page.locator('.meter').first()).toContainText('剩余 1,074 / 1,080 单位');
  await page.evaluate(() => { (window as any).usageResponse = null; });
  await page.locator('#refresh-usage').click();
  await expect(page.locator('#usage-status')).toContainText('Quick 暂未返回额度信息');
  await expect(page.locator('#usage-status')).toContainText('当前显示上次获取的数据');
});

test('credential export opens the save dialog directly without a password', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await page.locator('[data-credentials="export"]').click();
  await expect.poll(() => page.evaluate(() => (window as any).exportedPath)).toBe('/synthetic/QuickWay.qwc');
  const args = await page.evaluate(() => (window as any).calls.find((c: any) => c.command === 'export_credentials').args);
  expect(args).toEqual({ path: '/synthetic/QuickWay.qwc' });
  await expect(page.locator('input[type="password"]')).toHaveCount(0);
  await expect(page.locator('#toast')).toContainText('凭证已导出');
});

test('credential import failure preserves the source and valid import is immediate', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await page.evaluate(() => { (window as any).credentialError = '凭证文件已损坏。'; });
  await page.locator('[data-credentials="import"]').click();
  await expect(page.locator('#toast')).toContainText('文件已损坏');
  await expect(page.locator('#credential-state')).toContainText('本机 Quick 登录');
  await page.evaluate(() => { (window as any).credentialError = ''; });
  await page.locator('[data-credentials="import"]').click();
  await expect(page.locator('#credential-state')).toContainText('当前使用导入凭证');
  await expect(page.locator('#credential-state')).toContainText('有效至');
  await expect(page.locator('[data-authorize-quick]')).toHaveCount(0);
  const args = await page.evaluate(() => (window as any).calls.find((c: any) => c.command === 'import_credentials').args);
  expect(args).toEqual({ path: '/synthetic/QuickWay.qwc' });
  await page.locator('[data-credentials="forget"]').click();
  await expect(page.locator('#credential-state')).toContainText('本机 Quick 登录');
});

test('imported credentials are usable immediately after restart without native Quick', async ({ page }) => {
  await page.addInitScript(() => { (window as any).credentials = { source: 'imported' }; (window as any).lockedKeychain = true; });
  await page.reload();
  await page.locator('[data-view="settings"]').click();
  await expect(page.locator('#credential-state')).toContainText('当前使用导入凭证');
  await expect(page.locator('[data-credentials="export"]')).toBeEnabled();
  await expect(page.locator('input[type="password"], [data-credentials="unlock"], [data-authorize-quick]')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => /^(unlock_credentials|authorize_quick)$/.test(c.command)))).toBe(false);
});

test('cancelling credential file dialogs performs no credential writes', async ({ page }) => {
  await page.locator('[data-view="settings"]').click();
  await page.evaluate(() => { (window as any).cancelOpen = true; (window as any).cancelSave = true; });
  await page.locator('[data-credentials="import"]').click();
  await expect(page.locator('[data-credentials="import"]')).toBeEnabled();
  await page.locator('[data-credentials="export"]').click();
  await expect(page.locator('[data-credentials="export"]')).toBeEnabled();
  expect(await page.evaluate(() => (window as any).calls.some((c: any) => /^(import|export)_credentials$/.test(c.command)))).toBe(false);
});

test('credential controls remain accessible at minimum window size', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 640 });
  await page.locator('[data-view="settings"]').click();
  await page.locator('.credential-settings').scrollIntoViewIfNeeded();
  await expect(page.locator('[data-credentials="export"]')).toBeInViewport();
  await expect(page.locator('[data-credentials="import"]')).toBeInViewport();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('language picker relabels the app, keeps the draft, persists and updates the tray', async ({ page }) => {
  await expect(page.locator('#language')).toHaveValue('zh');
  await expect.poll(() => page.evaluate(() => (window as any).tray)).toEqual({ show: zh.trayShow, quit: zh.trayQuit, tooltip: zh.trayTooltip });
  await page.locator('#prompt').fill('keep me');
  await page.locator('#language').selectOption('en');
  await expect(page.locator('[data-view="dashboard"]')).toHaveText(en.navDashboard);
  await expect(page.locator('#prompt')).toHaveAttribute('placeholder', en.composerPlaceholder);
  await expect(page.locator('#prompt')).toHaveValue('keep me');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-US');
  await expect(page).toHaveTitle(`QuickWay · ${en.navChat}`);
  await expect(page.locator('#language')).toBeFocused();
  expect(await page.evaluate(() => (window as any).tray)).toEqual({ show: en.trayShow, quit: en.trayQuit, tooltip: en.trayTooltip });
  await page.reload();
  await expect(page.locator('.status-word')).toHaveText(en.running);
  await expect(page.locator('#language')).toHaveValue('en');
});

test('first launch follows the system language and falls back to English', async ({ page }) => {
  await page.addInitScript(() => { localStorage.removeItem('quickway.language'); Object.defineProperty(navigator, 'languages', { value: JSON.parse(sessionStorage.getItem('systemLanguages') || '["ja-JP", "en-US"]') }); });
  await page.reload();
  await expect(page.locator('[data-view="dashboard"]')).toHaveText(ja.navDashboard);
  await page.evaluate(() => sessionStorage.setItem('systemLanguages', '["fr-FR", "de-DE"]'));
  await page.reload();
  await expect(page.locator('[data-view="dashboard"]')).toHaveText(en.navDashboard);
});

test('every language catalog has the source keys and placeholders', async () => {
  const placeholders = (text: string) => [...text.matchAll(/\{\w+\}|<br>/g)].map(m => m[0]).sort().join();
  expect(languages.map(l => l.code).sort()).toEqual(['en', 'es', 'id', 'ja', 'ko', 'pt', 'ru', 'vi', 'zh']);
  for (const { code } of languages) {
    const catalog = (await import(`../src/locales/${code}.ts`)).default as Record<string, string>;
    expect(Object.keys(catalog).sort(), code).toEqual(Object.keys(zh).sort());
    for (const [key, text] of Object.entries(zh)) {
      expect(catalog[key].trim(), `${code}.${key}`).not.toBe('');
      expect(placeholders(catalog[key]), `${code}.${key}`).toBe(placeholders(text));
    }
  }
});
