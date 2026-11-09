import type { Instance, ResourceKind, ResourceVersion } from "./api";
import { t } from "./i18n";


export interface ResourceTag {
  value: string;
  label: string;
  /** Modrinth category name. */
  facet?: string;
  /** Keyword appended to the search query when there is no category. */
  query?: string;
  /** CurseForge category id (maps). */
  facetId?: number;
}

export const RESOURCE_KIND_LABEL: Record<ResourceKind, string> = {
  mod: t("模组"),
  modpack: t("整合包"),
  shader: t("光影"),
  resourcepack: t("材质"),
  map: t("地图"),
};

export const RESOURCE_KINDS: { id: ResourceKind | "map" | "loader"; label: string }[] = [
  { id: "mod", label: t("模组") },
  { id: "modpack", label: t("整合包") },
  { id: "shader", label: t("光影") },
  { id: "resourcepack", label: t("材质") },
  { id: "map", label: t("地图") },
  { id: "loader", label: t("加载器") },
];

export const RESOURCE_SOURCES = [
  { value: "modrinth", label: "Modrinth" },
  { value: "curseforge", label: "CurseForge" },
] as const;

export const RESOURCE_SORTS = [
  { value: "relevance", label: t("相关度") },
  { value: "downloads", label: t("下载量") },
  { value: "follows", label: t("关注数") },
  { value: "newest", label: t("最新发布") },
  { value: "updated", label: t("最近更新") },
];

export const RESOURCE_LOADERS = [
  { value: "", label: t("全部") },
  { value: "fabric", label: "Fabric" },
  { value: "forge", label: "Forge" },
  { value: "quilt", label: "Quilt" },
  { value: "neoforge", label: "NeoForge" },
];

export const RESOURCE_TAGS: Record<ResourceKind, ResourceTag[]> = {
  mod: [
    { value: "adventure", label: t("冒险"), facet: "adventure" },
    { value: "magic", label: t("魔幻"), facet: "magic" },
    { value: "technology", label: t("科技"), facet: "technology" },
    { value: "worldgen", label: t("世界生成"), facet: "worldgen" },
    { value: "decoration", label: t("装饰"), facet: "decoration" },
    { value: "equipment", label: t("装备"), facet: "equipment" },
    { value: "food", label: t("食物"), facet: "food" },
    { value: "storage", label: t("存储"), facet: "storage" },
    { value: "transportation", label: t("交通"), facet: "transportation" },
    { value: "economy", label: t("经济"), facet: "economy" },
    { value: "game-mechanics", label: t("机制"), facet: "game-mechanics" },
    { value: "optimization", label: t("优化"), facet: "optimization" },
    { value: "utility", label: t("实用"), facet: "utility" },
    { value: "library", label: t("前置库"), facet: "library" },
    { value: "cursed", label: t("诡异"), facet: "cursed" },
    { value: "survival", label: t("生存"), query: "survival" },
    { value: "horror", label: t("恐怖"), query: "horror" },
  ],
  modpack: [
    { value: "adventure", label: t("冒险"), facet: "adventure" },
    { value: "magic", label: t("魔幻"), facet: "magic" },
    { value: "technology", label: t("科技"), facet: "technology" },
    { value: "combat", label: t("战斗"), facet: "combat" },
    { value: "challenging", label: t("挑战"), facet: "challenging" },
    { value: "kitchen-sink", label: t("大杂烩"), facet: "kitchen-sink" },
    { value: "lightweight", label: t("轻量"), facet: "lightweight" },
    { value: "multiplayer", label: t("多人"), facet: "multiplayer" },
    { value: "optimization", label: t("优化"), facet: "optimization" },
    { value: "quests", label: t("任务"), facet: "quests" },
    { value: "survival", label: t("生存"), query: "survival" },
    { value: "horror", label: t("恐怖"), query: "horror" },
  ],
  shader: [
    { value: "realistic", label: t("写实"), facet: "realistic" },
    { value: "semi-realistic", label: t("半写实"), facet: "semi-realistic" },
    { value: "fantasy", label: t("奇幻"), facet: "fantasy" },
    { value: "cartoon", label: t("卡通"), facet: "cartoon" },
    { value: "vanilla-like", label: t("原版风"), facet: "vanilla-like" },
    { value: "cursed", label: t("诡异"), facet: "cursed" },
  ],
  map: [
    { value: "adventure", label: t("冒险"), facetId: 248 },
    { value: "survival", label: t("生存"), facetId: 253 },
    { value: "puzzle", label: t("解谜"), facetId: 252 },
    { value: "creation", label: t("建筑/创作"), facetId: 249 },
    { value: "parkour", label: t("跑酷"), facetId: 251 },
    { value: "game-map", label: t("游戏地图"), facetId: 250 },
    { value: "modded", label: t("模组世界"), facetId: 4464 },
  ],
  resourcepack: [
    { value: "realistic", label: t("写实"), facet: "realistic" },
    { value: "simplistic", label: t("简约"), facet: "simplistic" },
    { value: "themed", label: t("主题"), facet: "themed" },
    { value: "vanilla-like", label: t("原版风"), facet: "vanilla-like" },
    { value: "combat", label: t("战斗"), facet: "combat" },
    { value: "decoration", label: t("装饰"), facet: "decoration" },
    { value: "modded", label: t("模组"), facet: "modded" },
    { value: "tweaks", label: t("调整"), facet: "tweaks" },
    { value: "utility", label: t("实用"), facet: "utility" },
    { value: "cursed", label: t("诡异"), facet: "cursed" },
  ],
};

/** CurseForge category ids (top-level categories per class). */
export const RESOURCE_CF_TAGS: Record<ResourceKind, ResourceTag[]> = {
  mod: [
    { value: "cf-406", label: t("世界生成"), facetId: 406 },
    { value: "cf-412", label: t("科技"), facetId: 412 },
    { value: "cf-419", label: t("魔法"), facetId: 419 },
    { value: "cf-422", label: t("冒险/RPG"), facetId: 422 },
    { value: "cf-421", label: t("前置库"), facetId: 421 },
    { value: "cf-434", label: t("装备武器"), facetId: 434 },
    { value: "cf-420", label: t("存储"), facetId: 420 },
    { value: "cf-436", label: t("食物"), facetId: 436 },
    { value: "cf-4558", label: t("红石"), facetId: 4558 },
    { value: "cf-424", label: t("装饰"), facetId: 424 },
    { value: "cf-6814", label: t("优化"), facetId: 6814 },
    { value: "cf-5191", label: t("实用/QoL"), facetId: 5191 },
    { value: "cf-423", label: t("地图与信息"), facetId: 423 },
    { value: "cf-435", label: t("服务器工具"), facetId: 435 },
    { value: "cf-426", label: t("附属"), facetId: 426 },
    { value: "cf-6821", label: t("Bug 修复"), facetId: 6821 },
    { value: "cf-425", label: t("杂项"), facetId: 425 },
    { value: "cf-10775", label: t("恐怖"), facetId: 10775 },
  ],
  modpack: [
    { value: "cf-4472", label: t("科技"), facetId: 4472 },
    { value: "cf-4473", label: t("魔法"), facetId: 4473 },
    { value: "cf-4475", label: t("冒险/RPG"), facetId: 4475 },
    { value: "cf-4476", label: t("探索"), facetId: 4476 },
    { value: "cf-4478", label: t("任务"), facetId: 4478 },
    { value: "cf-4479", label: t("硬核"), facetId: 4479 },
    { value: "cf-4736", label: t("空岛"), facetId: 4736 },
    { value: "cf-4477", label: t("小游戏"), facetId: 4477 },
    { value: "cf-4484", label: t("多人"), facetId: 4484 },
    { value: "cf-4483", label: t("战斗/PvP"), facetId: 4483 },
    { value: "cf-4482", label: t("大型"), facetId: 4482 },
    { value: "cf-4481", label: t("轻量"), facetId: 4481 },
    { value: "cf-5128", label: t("原版+"), facetId: 5128 },
    { value: "cf-4474", label: t("科幻"), facetId: 4474 },
    { value: "cf-4480", label: t("地图向"), facetId: 4480 },
    { value: "cf-7418", label: t("恐怖"), facetId: 7418 },
    { value: "cf-9243", label: t("专家"), facetId: 9243 },
    { value: "cf-4487", label: t("FTB 官方"), facetId: 4487 },
  ],
  shader: [
    { value: "cf-6553", label: t("写实"), facetId: 6553 },
    { value: "cf-6554", label: t("奇幻"), facetId: 6554 },
    { value: "cf-6555", label: t("原版风"), facetId: 6555 },
  ],
  resourcepack: [
    { value: "cf-400", label: t("写实"), facetId: 400 },
    { value: "cf-393", label: "16x", facetId: 393 },
    { value: "cf-394", label: "32x", facetId: 394 },
    { value: "cf-395", label: "64x", facetId: 395 },
    { value: "cf-396", label: "128x", facetId: 396 },
    { value: "cf-397", label: "256x", facetId: 397 },
    { value: "cf-398", label: "512x+", facetId: 398 },
    { value: "cf-402", label: t("中世纪"), facetId: 402 },
    { value: "cf-401", label: t("现代"), facetId: 401 },
    { value: "cf-399", label: t("蒸汽朋克"), facetId: 399 },
    { value: "cf-403", label: t("传统"), facetId: 403 },
    { value: "cf-404", label: t("动画"), facetId: 404 },
    { value: "cf-4465", label: t("模组支持"), facetId: 4465 },
    { value: "cf-5244", label: t("字体"), facetId: 5244 },
    { value: "cf-5193", label: t("数据包"), facetId: 5193 },
    { value: "cf-405", label: t("杂项"), facetId: 405 },
  ],
  map: [
    { value: "cf-248", label: t("冒险"), facetId: 248 },
    { value: "cf-253", label: t("生存"), facetId: 253 },
    { value: "cf-252", label: t("解谜"), facetId: 252 },
    { value: "cf-249", label: t("建筑/创作"), facetId: 249 },
    { value: "cf-251", label: t("跑酷"), facetId: 251 },
    { value: "cf-250", label: t("游戏地图"), facetId: 250 },
    { value: "cf-4464", label: t("模组世界"), facetId: 4464 },
  ],
};

export function baseVersionOfInstance(instance: Instance): string {
  const id = instance.versionId;
  const forge = id.split("-forge-")[0];
  if (forge !== id) return forge;
  const neoforge = id.split("-neoforge-")[0];
  if (neoforge !== id) return neoforge;
  if (id.startsWith("fabric-loader-") || id.startsWith("quilt-loader-")) {
    return id.split("-").slice(-1)[0] ?? id;
  }
  return id;
}

export function instanceLoaderKind(instance: Instance): string {
  if (instance.modLoader?.kind) return instance.modLoader.kind.toLowerCase();
  const id = instance.versionId.toLowerCase();
  if (id.includes("-forge-")) return "forge";
  if (id.includes("-neoforge-")) return "neoforge";
  if (id.includes("fabric-loader-")) return "fabric";
  if (id.includes("quilt-loader-")) return "quilt";
  return "";
}

export function isVersionCompatible(
  kind: ResourceKind,
  version: ResourceVersion,
  instance: Instance,
): boolean {
  if (kind === "shader" || kind === "resourcepack" || kind === "map") return true;
  const loader = instanceLoaderKind(instance);
  if (!loader) return false;
  if (version.loaders.length > 0 && !version.loaders.includes(loader)) return false;
  const base = baseVersionOfInstance(instance);
  if (version.gameVersions.length > 0 && !version.gameVersions.includes(base)) return false;
  return true;
}
