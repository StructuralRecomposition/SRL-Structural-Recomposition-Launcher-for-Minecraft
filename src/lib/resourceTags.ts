import type { Instance, ResourceKind, ResourceVersion } from "./api";

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
  mod: "模组",
  modpack: "整合包",
  shader: "光影",
  resourcepack: "材质",
  map: "地图",
};

export const RESOURCE_KINDS: { id: ResourceKind | "map" | "loader"; label: string }[] = [
  { id: "mod", label: "模组" },
  { id: "modpack", label: "整合包" },
  { id: "shader", label: "光影" },
  { id: "resourcepack", label: "材质" },
  { id: "map", label: "地图" },
  { id: "loader", label: "加载器" },
];

export const RESOURCE_SOURCES = [
  { value: "modrinth", label: "Modrinth" },
  { value: "curseforge", label: "CurseForge" },
] as const;

export const RESOURCE_SORTS = [
  { value: "relevance", label: "相关度" },
  { value: "downloads", label: "下载量" },
  { value: "follows", label: "关注数" },
  { value: "newest", label: "最新发布" },
  { value: "updated", label: "最近更新" },
];

export const RESOURCE_LOADERS = [
  { value: "", label: "全部" },
  { value: "fabric", label: "Fabric" },
  { value: "forge", label: "Forge" },
  { value: "quilt", label: "Quilt" },
  { value: "neoforge", label: "NeoForge" },
];

export const RESOURCE_TAGS: Record<ResourceKind, ResourceTag[]> = {
  mod: [
    { value: "adventure", label: "冒险", facet: "adventure" },
    { value: "magic", label: "魔幻", facet: "magic" },
    { value: "technology", label: "科技", facet: "technology" },
    { value: "worldgen", label: "世界生成", facet: "worldgen" },
    { value: "decoration", label: "装饰", facet: "decoration" },
    { value: "equipment", label: "装备", facet: "equipment" },
    { value: "food", label: "食物", facet: "food" },
    { value: "storage", label: "存储", facet: "storage" },
    { value: "transportation", label: "交通", facet: "transportation" },
    { value: "economy", label: "经济", facet: "economy" },
    { value: "game-mechanics", label: "机制", facet: "game-mechanics" },
    { value: "optimization", label: "优化", facet: "optimization" },
    { value: "utility", label: "实用", facet: "utility" },
    { value: "library", label: "前置库", facet: "library" },
    { value: "cursed", label: "诡异", facet: "cursed" },
    { value: "survival", label: "生存", query: "survival" },
    { value: "horror", label: "恐怖", query: "horror" },
  ],
  modpack: [
    { value: "adventure", label: "冒险", facet: "adventure" },
    { value: "magic", label: "魔幻", facet: "magic" },
    { value: "technology", label: "科技", facet: "technology" },
    { value: "combat", label: "战斗", facet: "combat" },
    { value: "challenging", label: "挑战", facet: "challenging" },
    { value: "kitchen-sink", label: "大杂烩", facet: "kitchen-sink" },
    { value: "lightweight", label: "轻量", facet: "lightweight" },
    { value: "multiplayer", label: "多人", facet: "multiplayer" },
    { value: "optimization", label: "优化", facet: "optimization" },
    { value: "quests", label: "任务", facet: "quests" },
    { value: "survival", label: "生存", query: "survival" },
    { value: "horror", label: "恐怖", query: "horror" },
  ],
  shader: [
    { value: "realistic", label: "写实", facet: "realistic" },
    { value: "semi-realistic", label: "半写实", facet: "semi-realistic" },
    { value: "fantasy", label: "奇幻", facet: "fantasy" },
    { value: "cartoon", label: "卡通", facet: "cartoon" },
    { value: "vanilla-like", label: "原版风", facet: "vanilla-like" },
    { value: "cursed", label: "诡异", facet: "cursed" },
  ],
  map: [
    { value: "adventure", label: "冒险", facetId: 248 },
    { value: "survival", label: "生存", facetId: 253 },
    { value: "puzzle", label: "解谜", facetId: 252 },
    { value: "creation", label: "建筑/创作", facetId: 249 },
    { value: "parkour", label: "跑酷", facetId: 251 },
    { value: "game-map", label: "游戏地图", facetId: 250 },
    { value: "modded", label: "模组世界", facetId: 4464 },
  ],
  resourcepack: [
    { value: "realistic", label: "写实", facet: "realistic" },
    { value: "simplistic", label: "简约", facet: "simplistic" },
    { value: "themed", label: "主题", facet: "themed" },
    { value: "vanilla-like", label: "原版风", facet: "vanilla-like" },
    { value: "combat", label: "战斗", facet: "combat" },
    { value: "decoration", label: "装饰", facet: "decoration" },
    { value: "modded", label: "模组", facet: "modded" },
    { value: "tweaks", label: "调整", facet: "tweaks" },
    { value: "utility", label: "实用", facet: "utility" },
    { value: "cursed", label: "诡异", facet: "cursed" },
  ],
};

/** CurseForge category ids (top-level categories per class). */
export const RESOURCE_CF_TAGS: Record<ResourceKind, ResourceTag[]> = {
  mod: [
    { value: "cf-406", label: "世界生成", facetId: 406 },
    { value: "cf-412", label: "科技", facetId: 412 },
    { value: "cf-419", label: "魔法", facetId: 419 },
    { value: "cf-422", label: "冒险/RPG", facetId: 422 },
    { value: "cf-421", label: "前置库", facetId: 421 },
    { value: "cf-434", label: "装备武器", facetId: 434 },
    { value: "cf-420", label: "存储", facetId: 420 },
    { value: "cf-436", label: "食物", facetId: 436 },
    { value: "cf-4558", label: "红石", facetId: 4558 },
    { value: "cf-424", label: "装饰", facetId: 424 },
    { value: "cf-6814", label: "优化", facetId: 6814 },
    { value: "cf-5191", label: "实用/QoL", facetId: 5191 },
    { value: "cf-423", label: "地图与信息", facetId: 423 },
    { value: "cf-435", label: "服务器工具", facetId: 435 },
    { value: "cf-426", label: "附属", facetId: 426 },
    { value: "cf-6821", label: "Bug 修复", facetId: 6821 },
    { value: "cf-425", label: "杂项", facetId: 425 },
    { value: "cf-10775", label: "恐怖", facetId: 10775 },
  ],
  modpack: [
    { value: "cf-4472", label: "科技", facetId: 4472 },
    { value: "cf-4473", label: "魔法", facetId: 4473 },
    { value: "cf-4475", label: "冒险/RPG", facetId: 4475 },
    { value: "cf-4476", label: "探索", facetId: 4476 },
    { value: "cf-4478", label: "任务", facetId: 4478 },
    { value: "cf-4479", label: "硬核", facetId: 4479 },
    { value: "cf-4736", label: "空岛", facetId: 4736 },
    { value: "cf-4477", label: "小游戏", facetId: 4477 },
    { value: "cf-4484", label: "多人", facetId: 4484 },
    { value: "cf-4483", label: "战斗/PvP", facetId: 4483 },
    { value: "cf-4482", label: "大型", facetId: 4482 },
    { value: "cf-4481", label: "轻量", facetId: 4481 },
    { value: "cf-5128", label: "原版+", facetId: 5128 },
    { value: "cf-4474", label: "科幻", facetId: 4474 },
    { value: "cf-4480", label: "地图向", facetId: 4480 },
    { value: "cf-7418", label: "恐怖", facetId: 7418 },
    { value: "cf-9243", label: "专家", facetId: 9243 },
    { value: "cf-4487", label: "FTB 官方", facetId: 4487 },
  ],
  shader: [
    { value: "cf-6553", label: "写实", facetId: 6553 },
    { value: "cf-6554", label: "奇幻", facetId: 6554 },
    { value: "cf-6555", label: "原版风", facetId: 6555 },
  ],
  resourcepack: [
    { value: "cf-400", label: "写实", facetId: 400 },
    { value: "cf-393", label: "16x", facetId: 393 },
    { value: "cf-394", label: "32x", facetId: 394 },
    { value: "cf-395", label: "64x", facetId: 395 },
    { value: "cf-396", label: "128x", facetId: 396 },
    { value: "cf-397", label: "256x", facetId: 397 },
    { value: "cf-398", label: "512x+", facetId: 398 },
    { value: "cf-402", label: "中世纪", facetId: 402 },
    { value: "cf-401", label: "现代", facetId: 401 },
    { value: "cf-399", label: "蒸汽朋克", facetId: 399 },
    { value: "cf-403", label: "传统", facetId: 403 },
    { value: "cf-404", label: "动画", facetId: 404 },
    { value: "cf-4465", label: "模组支持", facetId: 4465 },
    { value: "cf-5244", label: "字体", facetId: 5244 },
    { value: "cf-5193", label: "数据包", facetId: 5193 },
    { value: "cf-405", label: "杂项", facetId: 405 },
  ],
  map: [
    { value: "cf-248", label: "冒险", facetId: 248 },
    { value: "cf-253", label: "生存", facetId: 253 },
    { value: "cf-252", label: "解谜", facetId: 252 },
    { value: "cf-249", label: "建筑/创作", facetId: 249 },
    { value: "cf-251", label: "跑酷", facetId: 251 },
    { value: "cf-250", label: "游戏地图", facetId: 250 },
    { value: "cf-4464", label: "模组世界", facetId: 4464 },
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
