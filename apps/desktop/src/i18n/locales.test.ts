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
  test("supports configured UI locales with English as fallback", () => {
    expect(DEFAULT_LOCALE).toBe("en");
    expect(LOCALE_OPTIONS.map((option) => option.id)).toEqual([
      "en",
      "ko",
      "ja",
      "es",
      "fr",
      "de",
    ]);
    for (const locale of ["en", "ko", "ja", "es", "fr", "de"]) {
      expect(isLocale(locale)).toBe(true);
    }
    expect(isLocale("it")).toBe(false);
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
    expect(translate("es", "settings.general.title")).toBe("General");
    expect(translate("fr", "nav.settings")).toBe("Paramètres");
    expect(translate("de", "settings.general.title")).toBe("Allgemein");
    expect(translate("it", "nav.settings")).toBe("Settings");
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

test("provider exports the existing settings storage key", async () => {
  const provider = await import("./I18nProvider");

  expect(provider.UI_LANGUAGE_STORAGE_KEY).toBe("hyprduck.uiLanguage");
});
