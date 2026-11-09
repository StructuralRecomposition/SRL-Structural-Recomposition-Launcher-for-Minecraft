import { EN_BASE } from "./i18n.en.base";
import { EN_MANUAL_1 } from "./i18n.en.manual1";
import { EN_MANUAL_2 } from "./i18n.en.manual2";
import { EN_MANUAL_3 } from "./i18n.en.manual3";

// Base entries are machine-assisted and normalized; manual entries override
// them and cover strings added after the first translation pass.
export const EN: Record<string, string> = {
  ...EN_BASE,
  ...EN_MANUAL_1,
  ...EN_MANUAL_2,
  ...EN_MANUAL_3,
};
