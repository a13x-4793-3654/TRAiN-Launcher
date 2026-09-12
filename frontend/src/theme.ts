import { createDarkTheme, createLightTheme } from "@fluentui/react-components";
import type { BrandVariants, Theme } from "@fluentui/react-components";

/**
 * TRAiN Launcherのブランドカラー(チール系)。
 *
 * ロゴ(`assets/branding/train-launcher-logo.svg`)のアクセントカラー(`#8FE3E0`)・
 * 濃紺パネル(`#0D1526` 〜 `#182742`)に合わせたチール基調のブランドランプ。
 * Fluent UIの `createLightTheme`/`createDarkTheme` に渡すことで、ボタンやリンクなどの
 * ブランドカラーをロゴのイメージに揃える。
 */
const trainBrand: BrandVariants = {
  10: "#071817",
  20: "#0B2827",
  30: "#113635",
  40: "#174543",
  50: "#1E5755",
  60: "#276866",
  70: "#317D7A",
  80: "#3A8D8A",
  90: "#43A3A0",
  100: "#54B6B2",
  110: "#72C0BD",
  120: "#92C9C7",
  130: "#B1D3D2",
  140: "#CDDFDF",
  150: "#E1EAE9",
  160: "#F1F4F3",
};

export const trainLightTheme: Theme = createLightTheme(trainBrand);

export const trainDarkTheme: Theme = {
  ...createDarkTheme(trainBrand),
  // ロゴの濃紺パネル(`#0D1526`〜`#182742`)に合わせた背景色。
  colorNeutralBackground1: "#0D1526",
  colorNeutralBackground2: "#111C30",
  colorNeutralBackground3: "#182742",
  colorNeutralBackground4: "#1E2E4C",
};
