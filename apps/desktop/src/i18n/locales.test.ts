import { describe, expect, test } from "bun:test";

import {
  DEFAULT_LOCALE,
  LOCALE_OPTIONS,
  TRANSLATIONS,
  isLocale,
  translate,
  type Locale,
  type TranslationKey,
} from "./locales";

describe("desktop i18n locales", () => {
  test("supports English, Korean, and Japanese with English as fallback", () => {
    expect(DEFAULT_LOCALE).toBe("en");
    expect(LOCALE_OPTIONS.map((option) => option.id)).toEqual(["en", "ko", "ja"]);
    expect(isLocale("en")).toBe(true);
    expect(isLocale("ko")).toBe(true);
    expect(isLocale("ja")).toBe(true);
    expect(isLocale("fr")).toBe(false);
  });

  test("every locale has the same translation keys", () => {
    const englishKeys = Object.keys(TRANSLATIONS.en).sort();

    for (const locale of Object.keys(TRANSLATIONS) as Locale[]) {
      expect(Object.keys(TRANSLATIONS[locale]).sort()).toEqual(englishKeys);
    }
  });

  test("falls back to English for missing key values", () => {
    expect(translate("ko", "nav.knowledge")).toBe("지식");
    expect(translate("ja", "settings.general.title")).toBe("一般");
    expect(translate("fr", "nav.settings")).toBe("Settings");
  });

  test("translation key type includes operational desktop copy", () => {
    const keys: TranslationKey[] = [
      "app.startup.title",
      "settings.ai.refresh",
      "workspace.empty.title",
      "workspace.import.partial",
      "workspace.answer.grounded",
    ];

    expect(keys.map((key) => translate(DEFAULT_LOCALE, key))).toEqual([
      "HyprDuck failed to start",
      "Refresh",
      "Add private docs",
      "Partial import",
      "Grounded",
    ]);
  });
});
