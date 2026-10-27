import { createInstance, type TFunction, type ParseKeys } from "i18next";
import { derived, get, writable } from "svelte/store";
import { en } from "./en";
import { de } from "./de";

export type UiLanguage = "en" | "de";
export type Translator = TFunction<"translation">;
export const LANGUAGES = [
  { value: "en", label: "English" },
  { value: "de", label: "Deutsch" },
] as const;

const engine = createInstance();
void engine.init({
  initAsync: false,
  lng: "en",
  fallbackLng: "en",
  supportedLngs: ["en", "de"],
  resources: { en: { translation: en }, de: { translation: de } },
  // Svelte renders every translated message as text, including interpolated user names.
  interpolation: { escapeValue: false },
  returnNull: false,
  returnEmptyString: false,
});

const language = writable<UiLanguage>("en");
export const locale = { subscribe: language.subscribe };
export const t = derived(language, (value) => engine.getFixedT(value));
export const displayLocale = derived(language, (value) => value === "de" ? "de-DE" : "en-GB");

export function setLanguage(value: unknown): UiLanguage {
  const next = value === "de" ? "de" : "en";
  void engine.changeLanguage(next);
  language.set(next);
  if (typeof document !== "undefined") {
    document.documentElement.lang = next;
    document.documentElement.dir = "ltr";
  }
  return next;
}

/** For event handlers and non-component helpers. Reactive callers use the t store. */
export const translate: Translator = engine.t.bind(engine);

export type MessageKey = ParseKeys<"translation">;
export type UiText = string | { key: MessageKey; params: Record<string, UiText | number | boolean | null | undefined> };

/** Retain a notification's meaning so an already-visible message follows language changes. */
export function notice(key: MessageKey, params: Exclude<UiText, string>["params"] = {}): UiText {
  return { key, params };
}

export function renderMessage(value: UiText, translator: Translator = translate): string {
  if (typeof value === "string") return value;
  const params = Object.fromEntries(Object.entries(value.params).map(([key, item]) =>
    [key, typeof item === "object" && item !== null ? renderMessage(item, translator) : item]));
  // MessageKey is checked on creation; a stored descriptor has a union of all possible params.
  return (translator as (key: string, params: Record<string, unknown>) => string)(value.key, params);
}

export function formatDate(value: number | Date, languageTag = get(displayLocale)): string {
  return new Intl.DateTimeFormat(languageTag, {
    day: "numeric", month: "short", year: "numeric",
  }).format(value);
}

export function formatNumber(value: number, options?: Intl.NumberFormatOptions, languageTag = get(displayLocale)): string {
  return new Intl.NumberFormat(languageTag, options).format(value);
}
