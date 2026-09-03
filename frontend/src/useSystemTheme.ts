import { useEffect, useState } from "react";

export type ColorScheme = "light" | "dark";

/**
 * OS(Windows)のライト/ダーク設定を購読し、切り替えに追従するフック。
 *
 * Tauriのウィンドウテーマ(`@tauri-apps/api/window`)を優先的に使用し、
 * 通常のブラウザ(`npm run dev`をTauri抜きで開いた場合など)では
 * `prefers-color-scheme` メディアクエリにフォールバックする。
 */
export function useSystemTheme(): ColorScheme {
  const [scheme, setScheme] = useState<ColorScheme>(() =>
    window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light",
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const useMediaQueryFallback = () => {
      const mql = window.matchMedia("(prefers-color-scheme: dark)");
      const update = (e: MediaQueryList | MediaQueryListEvent) =>
        setScheme(e.matches ? "dark" : "light");
      update(mql);
      mql.addEventListener("change", update);
      unlisten = () => mql.removeEventListener("change", update);
    };

    (async () => {
      try {
        // TODO: 将来的にユーザーが手動でテーマを選択できる設定画面を追加する場合、
        // ここでの自動追従と手動選択のマージ方法を検討する。
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        const current = await win.theme();
        if (cancelled) return;
        if (current) setScheme(current);

        unlisten = await win.onThemeChanged(({ payload }) => {
          setScheme(payload);
        });
      } catch {
        // Tauriコンテキスト外(通常のブラウザ)で実行された場合のフォールバック。
        if (!cancelled) useMediaQueryFallback();
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return scheme;
}
