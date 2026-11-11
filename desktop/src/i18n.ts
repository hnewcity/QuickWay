// UI languages, matching KiroaaS: the same nine languages, a saved choice, and system-language detection.
import zh, { type MessageKey, type Messages } from './locales/zh';
import en from './locales/en';
import ru from './locales/ru';
import es from './locales/es';
import id from './locales/id';
import pt from './locales/pt';
import ja from './locales/ja';
import ko from './locales/ko';
import vi from './locales/vi';

export type Language = 'zh' | 'en' | 'ru' | 'es' | 'id' | 'pt' | 'ja' | 'ko' | 'vi';
export type { MessageKey };

const catalogs: Record<Language, Messages> = { zh, en, ru, es, id, pt, ja, ko, vi };
/** Menu order and native names, as in the KiroaaS language switcher. `locale` drives number and date formats. */
export const languages: { code: Language; label: string; locale: string }[] = [
  { code: 'en', label: 'English', locale: 'en-US' },
  { code: 'zh', label: '中文', locale: 'zh-CN' },
  { code: 'ru', label: 'Русский', locale: 'ru-RU' },
  { code: 'es', label: 'Español', locale: 'es-ES' },
  { code: 'id', label: 'Bahasa Indonesia', locale: 'id-ID' },
  { code: 'pt', label: 'Português (Brasil)', locale: 'pt-BR' },
  { code: 'ja', label: '日本語', locale: 'ja-JP' },
  { code: 'ko', label: '한국어', locale: 'ko-KR' },
  { code: 'vi', label: 'Tiếng Việt', locale: 'vi-VN' },
];
const STORAGE_KEY = 'quickway.language';
const isLanguage = (value: unknown): value is Language => typeof value === 'string' && Object.hasOwn(catalogs, value);

function detect(): Language {
  try { const saved = localStorage.getItem(STORAGE_KEY); if (isLanguage(saved)) return saved; } catch { /* Storage unavailable. */ }
  if (typeof navigator === 'undefined') return 'en';
  for (const tag of navigator.languages?.length ? navigator.languages : [navigator.language]) {
    const primary = tag?.toLowerCase().split('-')[0];
    if (isLanguage(primary)) return primary;
  }
  return 'en';
}

let current: Language = detect();
export const language = () => current;
export const locale = () => languages.find(l => l.code === current)!.locale;

/** Looks up `key` in the current language, falling back to English, and fills `{name}` placeholders. */
export function t(key: MessageKey, vars?: Record<string, string | number>) {
  const text = catalogs[current][key] ?? en[key] ?? key;
  return vars ? text.replace(/\{(\w+)\}/g, (match, name: string) => name in vars ? String(vars[name]) : match) : text;
}

export function setLanguage(next: Language) {
  current = next;
  try { localStorage.setItem(STORAGE_KEY, next); } catch { /* The choice still applies to this session. */ }
}
