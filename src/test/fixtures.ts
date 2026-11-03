import type { Bootstrap, VersionDetail } from "../lib/api";

export const fixtureBootstrap: Bootstrap = {
  appVersion: "0.1.0",
  platform: "macos x86_64",
  settings: {
    gameDir: "/tmp/sr-test/minecraft",
    javaPath: null,
    maxConcurrentDownloads: 6,
    theme: "dark",
    downloadAssetsOnInstall: true,
    language: "zh-CN",
    allowCustomHosts: false,
    useMirror: true,
    microsoftClientId: null,
    applyOfflineSkin: true,
    assistantModel: "deepseek-chat",
    assistantBaseUrl: "https://api.deepseek.com",
  },
  paths: {
    dataDir: "/tmp/sr-test",
    gameDir: "/tmp/sr-test/minecraft",
    logsDir: "/tmp/sr-test/logs",
  },
  instances: [
    {
      id: "abc123def456",
      name: "生存世界",
      versionId: "1.20.4",
      gameDir: "/tmp/sr-test/instances/abc123def456",
      javaPath: null,
      memoryMb: 4096,
      jvmArgs: [],
      gameArgs: [],
      createdAt: "2026-10-01T10:00:00Z",
      lastLaunchedAt: null,
      accountId: null,
      resolutionWidth: null,
      resolutionHeight: null,
      modLoader: null,
    },
  ],
  accounts: [
    {
      id: "acc1",
      username: "Steve",
      uuid: "00000000-0000-3000-8000-000000000000",
      kind: "offline",
      status: "offline",
      createdAt: "2026-10-01T10:00:00Z",
      lastUsedAt: null,
      skinId: null,
      skinModel: null,
      active: true,
      note: "离线档案：仅用于本地游戏，不是已验证的正版账号。",
    },
  ],
  java: [
    {
      path: "/Library/Java/JavaVirtualMachines/temurin-21/Contents/Home/bin/java",
      versionString: "21.0.4",
      major: 21,
      vendor: "Eclipse Adoptium",
      arch: "x86_64",
      source: "scan",
    },
  ],
  installedVersions: [
    {
      id: "1.20.4",
      versionType: "release",
      hasJson: true,
      hasJar: true,
      javaMajor: 17,
      mainClass: "net.minecraft.client.main.Main",
      sizeBytes: 12345678,
      modifiedAt: "2026-10-23T10:00:00Z",
      inheritsFrom: null,
      loader: null,
      path: "/tmp/game/versions/1.20.4",
    },
  ],
  runningInstances: [],
  features: [
    { id: "offline_accounts", available: true, reason: null },
    {
      id: "microsoft_login",
      available: false,
      reason: "未配置 Azure 应用注册（client id / redirect URI）",
    },
  ],
};

const PREVIEW =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

export const fixtureSkinList = {
  skins: [
    {
      id: "builtin-classic-steve",
      name: "Steve",
      model: "classic",
      source: "builtin",
      width: 64,
      height: 64,
      sha1: "classic-steve",
      owner: null,
      createdAt: "2026-10-01T10:00:00Z",
      preview: PREVIEW,
    },
    {
      id: "builtin-slim-alex",
      name: "Alex",
      model: "slim",
      source: "builtin",
      width: 64,
      height: 64,
      sha1: "slim-alex",
      owner: null,
      createdAt: "2026-10-01T10:00:00Z",
      preview: PREVIEW,
    },
    {
      id: "skin-local-1",
      name: "我的皮肤",
      model: "classic",
      source: "local",
      width: 64,
      height: 64,
      sha1: "local-1",
      owner: null,
      createdAt: "2026-10-02T10:00:00Z",
      preview: PREVIEW,
    },
  ],
  builtinVersion: "1.20.4",
  builtinHint: null,
};

export const fixtureDetail: VersionDetail = {
  id: "1.20.4",
  mainClass: "net.minecraft.client.main.Main",
  versionType: "release",
  javaMajor: 17,
  libraryCount: 40,
  nativeCount: 0,
  legacyArguments: false,
  installed: true,
  issues: [],
  javaCheck: {
    compatibility: "compatible",
    requiredMajor: 17,
    actualMajor: 21,
    message: "Java 21 满足该版本要求（Java 17+）。",
  },
};

export const fixtureWorlds = [
  {
    folder: "test",
    name: "test",
    sizeBytes: 123456,
    levelDatBytes: 39678,
    regionFiles: 4,
    modifiedAt: "2026-10-25T09:36:00Z",
  },
];

export const fixtureResourcePacks = [
  {
    fileName: "test-pack.zip",
    isDir: false,
    size: 12345,
    modifiedAt: "2026-10-25T08:00:00Z",
    enabled: true,
    managed: false,
  },
  {
    fileName: "SRLocalSkin.zip",
    isDir: false,
    size: 93215,
    modifiedAt: "2026-10-25T09:20:00Z",
    enabled: true,
    managed: true,
  },
];
