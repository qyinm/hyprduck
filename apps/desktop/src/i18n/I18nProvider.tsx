import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import {
  DEFAULT_LOCALE,
  LOCALE_OPTIONS,
  isLocale,
  translate,
  type Locale,
  type TranslationKey,
} from "./locales";

export const UI_LANGUAGE_STORAGE_KEY = "etyma.uiLanguage";

interface I18nContextValue {
  locale: Locale;
  localeOptions: typeof LOCALE_OPTIONS;
  setLocale: (locale: Locale) => void;
  t: (
    key: TranslationKey,
    replacements?: Record<string, string | number>,
  ) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

function readStoredLocale(): Locale {
  if (typeof window === "undefined") {
    return DEFAULT_LOCALE;
  }

  const storedLanguage = window.localStorage.getItem(UI_LANGUAGE_STORAGE_KEY);
  return storedLanguage && isLocale(storedLanguage)
    ? storedLanguage
    : DEFAULT_LOCALE;
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => readStoredLocale());

  useEffect(() => {
    window.localStorage.setItem(UI_LANGUAGE_STORAGE_KEY, locale);
    document.documentElement.lang = locale;
  }, [locale]);

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      localeOptions: LOCALE_OPTIONS,
      setLocale: setLocaleState,
      t: (key, replacements) => translate(locale, key, replacements),
    }),
    [locale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error("useI18n must be used inside I18nProvider.");
  }
  return context;
}
