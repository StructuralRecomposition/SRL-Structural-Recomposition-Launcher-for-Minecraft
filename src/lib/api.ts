import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export class ApiError extends Error {
  code: string;
  detail?: string;
  retryable: boolean;
  constructor(code: string, message: string, detail?: string, retryable = false) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.detail = detail;
    this.retryable = retryable;
  }
}

function normalizeError(raw: unknown): ApiError {
  if (raw && typeof raw === "object" && "code" in raw && "message" in raw) {
    const e = raw as { code: string; message: string; detail?: string; retryable?: boolean };
    return new ApiError(e.code, e.message, e.detail, Boolean(e.retryable));
  }
  if (raw instanceof Error) return new ApiError("INTERNAL", raw.message);
  return new ApiError("INTERNAL", String(raw));
}

export async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new ApiError("NOT_DESKTOP", "此功能需要在 SRL 桌面应用中运行。");
  }
  try {
    return await invoke<T>(name, args);
  } catch (raw) {
    throw normalizeError(raw);
  }
}

/**
 * Invoke a command with a raw byte body. Tauri sends the `Uint8Array` as an
 * `InvokeBody::Raw` payload instead of JSON, so large mod/shader files do not
 * pay base64 overhead.
 */
export async function commandBytes<T>(
  name: string,
  bytes: Uint8Array,
  headers: Record<string, string>,
): Promise<T> {
  if (!isTauri()) {
    throw new ApiError("NOT_DESKTOP", "此功能需要在 SRL 桌面应用中运行。");
  }
  try {
    return await invoke<T>(name, bytes, { headers });
  } catch (raw) {
    throw normalizeError(raw);
  }
}

export function onEvent<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(() => undefined);
  return listen<T>(event, (e) => handler(e.payload));
}

// --- Types mirroring the Rust DTOs -----------------------------------------

export interface Settings {
  gameDir: string;
  javaPath: string | null;
  maxConcurrentDownloads: number;
  theme: string;
  downloadAssetsOnInstall: boolean;
  language: string;
  allowCustomHosts: boolean;
  useMirror: boolean;
  microsoftClientId: string | null;
  applyOfflineSkin: boolean;
  assistantModel: string;
  assistantBaseUrl: string;
}


export interface AssistantStatus {
  configured: boolean;
  model: string;
  baseUrl: string;
  auditPath: string;
}

export interface AssistantHistoryItem {
  role: "user" | "assistant";
  content: string;
}

export interface AssistantToolTrace {
  name: string;
  summary: string;
  ok: boolean;
}

export interface AssistantProposal {
  kind: "repair" | "mod_install" | string;
  id: string;
  title: string;
  summary: string;
  plan: unknown;
}

export interface AssistantReply {
  content: string;
  toolTraces: AssistantToolTrace[];
  proposals: AssistantProposal[];
  model: string;
  conversationId: string;
  title: string;
}

export interface AssistantConversationSummary {
  id: string;
  title: string;
  instanceId: string | null;
  updatedAt: string;
  messageCount: number;
}

export interface AssistantStoredMessage {
  role: "user" | "assistant";
  content: string;
  traces: AssistantToolTrace[];
  proposals: AssistantProposal[];
  createdAt: string;
}

export interface AssistantConversation {
  id: string;
  title: string;
  instanceId: string | null;
  createdAt: string;
  updatedAt: string;
  messages: AssistantStoredMessage[];
}

export interface ModPlanEntry {
  projectId: string;
  title: string;
  versionId: string;
  versionNumber: string;
  fileName: string;
  url: string;
  size: number;
  sha1: string;
  requiredBy: string[];
}

export interface ModInstallPlan {
  id: string;
  instanceId: string;
  instanceName: string;
  createdAt: string;
  rootProject: string;
  gameVersion: string;
  loader: string;
  entries: ModPlanEntry[];
  warnings: string[];
  notes: string[];
}

export interface ModInstallResult {
  token: string;
  backupDir: string;
  installed: string[];
  warnings: string[];
}

export interface AssistantLineEvidence {
  line: number;
  text: string;
}

export interface AssistantException {
  kind: string;
  message: string;
  line: number;
}

export interface AssistantCrashEvidence {
  fileName: string;
  size: number;
  lineCount: number;
  description: string | null;
  minecraftVersion: string | null;
  javaVersion: string | null;
  loader: string | null;
  crashTime: string | null;
  signals: string[];
  exceptions: AssistantException[];
  causedBy: AssistantLineEvidence[];
  frames: AssistantLineEvidence[];
  suspectedMods: { name: string; reason: string; evidenceLine: number | null }[];
  modList: string[];
  excerpt: string;
}

export interface AssistantLogEvidence {
  fileName: string;
  lineCount: number;
  errorLines: AssistantLineEvidence[];
  errorContext: AssistantLineEvidence[];
  modList: string[];
  signals: string[];
  excerpt: string;
}


export interface RepairAction {
  kind: "set_memory" | "disable_mod" | string;
  description: string;
  target: string;
  from: string | null;
  to: string | null;
  evidence: string[];
  risk: string;
}

export interface RepairPlan {
  id: string;
  instanceId: string;
  instanceName: string;
  createdAt: string;
  actions: RepairAction[];
  notes: string[];
  rollback: string;
}

export interface RepairResult {
  token: string;
  backupDir: string;
  applied: string[];
  warnings: string[];
}

export interface AssistantLocalReport {
  instanceId: string;
  instanceName: string;
  versionId: string;
  loader: string | null;
  memoryMb: number;
  javaPath: string | null;
  mods: string[];
  crash: AssistantCrashEvidence | null;
  log: AssistantLogEvidence | null;
  signals: string[];
  facts: string[];
  limits: string[];
}

export interface DeviceCodeView {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
  message: string;
}

export interface MicrosoftPoll {
  status: "pending" | "slowDown" | "authorized" | "expired" | "denied" | "error";
  interval: number | null;
  message: string | null;
  account: Account | null;
}

export interface FeatureState {
  id: string;
  available: boolean;
  reason: string | null;
}

export interface PathsView {
  dataDir: string;
  gameDir: string;
  logsDir: string;
}

export interface Bootstrap {
  appVersion: string;
  platform: string;
  settings: Settings;
  paths: PathsView;
  instances: Instance[];
  accounts: AccountView[];
  java: JavaInfo[];
  installedVersions: InstalledVersion[];
  runningInstances: string[];
  features: FeatureState[];
}

export interface ModLoaderInfo {
  kind: string;
  version: string;
  installed: boolean;
}

export interface Instance {
  id: string;
  name: string;
  versionId: string;
  gameDir: string;
  javaPath: string | null;
  memoryMb: number;
  jvmArgs: string[];
  gameArgs: string[];
  createdAt: string;
  lastLaunchedAt: string | null;
  accountId: string | null;
  resolutionWidth: number | null;
  resolutionHeight: number | null;
  modLoader: ModLoaderInfo | null;
}

export interface Account {
  id: string;
  username: string;
  uuid: string;
  kind: "offline" | "microsoft";
  status: "offline" | "authorized" | "expired" | "notConfigured";
  createdAt: string;
  lastUsedAt: string | null;
  skinId?: string | null;
  skinModel?: SkinModel | null;
}

export type SkinModel = "classic" | "slim";
export type SkinSource = "builtin" | "local" | "mojang";

export interface Skin {
  id: string;
  name: string;
  model: SkinModel;
  source: SkinSource;
  width: number;
  height: number;
  sha1: string;
  owner: string | null;
  createdAt: string;
  /** data:image/png;base64,... of the normalized 64x64 texture. */
  preview: string;
}

export interface SkinListView {
  skins: Skin[];
  builtinVersion: string | null;
  builtinHint: string | null;
}

export interface SkinApplyResult {
  message: string;
}

export interface AccountView extends Account {
  active: boolean;
  note: string;
}

export interface JavaInfo {
  path: string;
  versionString: string;
  major: number;
  vendor: string | null;
  arch: string | null;
  source: string;
}

export interface JavaRuntimeOptions {
  majors: number[];
  lts: number[];
  recommended: number;
}

export interface LoaderVersionOption {
  version: string;
  label: string;
  variant: string | null;
  stable: boolean | null;
}

export interface InstalledVersion {
  id: string;
  versionType: string | null;
  hasJson: boolean;
  hasJar: boolean;
  javaMajor: number | null;
  mainClass: string | null;
  sizeBytes: number;
  modifiedAt: string | null;
  inheritsFrom: string | null;
  loader: string | null;
  path: string;
}

export interface VersionDeletePlan {
  id: string;
  dependents: string[];
  instances: string[];
  sizeBytes: number;
}

export interface VersionDeleteResult {
  deleted: string[];
}

export interface ManifestVersion {
  id: string;
  /** Mojang manifest field is `type` (release/snapshot/old_beta/old_alpha). */
  type: string;
  url: string;
  time: string | null;
  releaseTime: string | null;
  sha1: string | null;
  complianceLevel: number | null;
}

export interface VersionList {
  latestRelease: string;
  latestSnapshot: string;
  versions: ManifestVersion[];
  installed: string[];
  source: "network" | "cache";
  warning: string | null;
}

export type TaskStatus =
  | "queued"
  | "running"
  | "verifying"
  | "completed"
  | "cancelled"
  | "retrying"
  | "failed";

export interface AppErrorPayload {
  code: string;
  message: string;
  detail: string | null;
  retryable: boolean;
}

export interface TaskView {
  id: string;
  kind: string;
  title: string;
  status: TaskStatus;
  totalBytes: number;
  doneBytes: number;
  totalFiles: number;
  doneFiles: number;
  currentFile: string | null;
  error: AppErrorPayload | null;
  startedAt: string;
  finishedAt: string | null;
  speedBps: number;
  etaSeconds: number | null;
}

export interface JavaCheck {
  compatibility: "compatible" | "warning" | "incompatible";
  requiredMajor: number;
  actualMajor: number;
  message: string;
}

export interface VersionDetail {
  id: string;
  mainClass: string;
  versionType: string;
  javaMajor: number;
  libraryCount: number;
  nativeCount: number;
  legacyArguments: boolean;
  installed: boolean;
  issues: string[];
  javaCheck: JavaCheck | null;
}

export interface CrashFile {
  fileName: string;
  size: number;
  modifiedAt: string | null;
}

export interface CrashCause {
  title: string;
  detail: string;
  advice: string;
}

export interface CrashAnalysis {
  fileName: string;
  description: string | null;
  minecraftVersion: string | null;
  javaVersion: string | null;
  loader: string | null;
  causes: CrashCause[];
  suspectMods: string[];
  excerpt: string;
}

export interface ModEntry {
  fileName: string;
  size: number;
  enabled: boolean;
  modifiedAt: string | null;
}

export interface ImportOutcome {
  fileName: string;
  ok: boolean;
  error: string | null;
}

export type ShaderEntry = ModEntry;

export interface ShaderSupport {
  iris: string | null;
  optifine: string | null;
}

export interface WorldEntry {
  folder: string;
  name: string;
  sizeBytes: number;
  levelDatBytes: number;
  regionFiles: number;
  modifiedAt: string | null;
}

export type ResourceKind = "mod" | "modpack" | "shader" | "resourcepack" | "map";
export type ResourceSource = "modrinth" | "curseforge";

export interface ResourceSearchQuery {
  query: string;
  kind: ResourceKind;
  gameVersion?: string | null;
  loader?: string | null;
  category?: string | null;
  index?: string;
  offset?: number;
  limit?: number;
}

export interface ResourceHit {
  id: string;
  kind: ResourceKind;
  source: ResourceSource;
  slug: string;
  title: string;
  description: string;
  author: string;
  categories: string[];
  versions: string[];
  downloads: number;
  follows: number;
  iconUrl: string | null;
  dateModified: string | null;
  latestVersion: string | null;
  license: string | null;
  clientSide: string | null;
  serverSide: string | null;
}

export interface ResourceSearchPage {
  hits: ResourceHit[];
  offset: number;
  limit: number;
  totalHits: number;
}

export interface ResourceProject {
  id: string;
  kind: ResourceKind;
  source: ResourceSource;
  slug: string;
  title: string;
  description: string;
  author: string;
  categories: string[];
  downloads: number;
  follows: number;
  iconUrl: string | null;
  gameVersions: string[];
  loaders: string[];
  license: string | null;
  sourceUrl: string | null;
  issuesUrl: string | null;
  wikiUrl: string | null;
  websiteUrl: string;
  published: string | null;
  updated: string | null;
}

export interface ResourceFile {
  filename: string;
  url: string;
  size: number;
  primary: boolean;
  sha1: string | null;
  sha512: string | null;
  fileType: string | null;
}

export interface ResourceVersion {
  id: string;
  projectId: string;
  name: string;
  versionNumber: string;
  versionType: "release" | "beta" | "alpha" | string;
  datePublished: string | null;
  downloads: number;
  gameVersions: string[];
  loaders: string[];
  files: ResourceFile[];
  dependencies: { versionId: string | null; projectId: string | null; dependencyType: string }[];
}

export interface GameVersionInfo {
  version: string;
  versionType: string;
  date: string | null;
  major: boolean;
}

export type ResourceTarget =
  | { kind: "instance"; instanceId: string }
  | { kind: "directory"; path: string };

export interface ResourceTaskAccepted {
  taskId: string;
  title: string;
}

export interface ResourcePackEntry {
  fileName: string;
  isDir: boolean;
  size: number;
  modifiedAt: string | null;
  enabled: boolean;
  managed: boolean;
}

export interface InstanceArchiveManifest {
  format: string;
  exportedAt: string;
  appVersion: string;
  name: string;
  versionId: string;
  memoryMb: number;
  jvmArgs: string[];
  gameArgs: string[];
  resolutionWidth: number | null;
  resolutionHeight: number | null;
  modLoader: ModLoaderInfo | null;
}

export interface ImportPreview {
  manifest: InstanceArchiveManifest;
  fileCount: number;
  totalBytes: number;
  warnings: string[];
}

export interface ImportCheck extends ImportPreview {
  versionInstalled: boolean;
}

export interface ExportSummary {
  path: string;
  files: number;
  bytes: number;
  skipped: number;
}

export interface ModpackExportSummary {
  path: string;
  name: string;
  versionId: string;
  minecraftVersion: string;
  resolvedMods: number;
  bundledMods: number;
  overrides: number;
  bytes: number;
  warnings: string[];
}

export interface VerificationIssue {
  path: string;
  kind: string;
  reason: string;
}

export interface VerificationReport {
  checked: number;
  ok: boolean;
  issues: VerificationIssue[];
  checkedAssets: number;
}

export interface LaunchResult {
  instanceId: string;
  pid: number;
  logPath: string;
  warnings: string[];
}

export interface LaunchEvent {
  instanceId: string;
  kind: "log" | "exit" | string;
  stream: string | null;
  line: string | null;
  code: number | null;
  message: string | null;
}

export type CheckStatus = "ok" | "warning" | "error";

export interface CheckItem {
  id: string;
  label: string;
  status: CheckStatus;
  detail: string;
  fix: string | null;
}

export interface DiagnosticsReport {
  generatedAt: string;
  platform: string;
  gameDir: string;
  checks: CheckItem[];
  java: JavaInfo[];
  installedVersions: number;
  instances: number;
}

export interface LogFileInfo {
  path: string;
  name: string;
  size: number;
  modified: string | null;
}

export interface LogCleanup {
  removed: number;
  bytes: number;
}

export interface FabricLoader {
  version: string;
  stable: boolean;
  gameVersion: string;
}

export type QuiltLoader = FabricLoader;

export interface LoaderRecommendation {
  id: string;
  name: string;
  version: string | null;
  status: "recommended" | "available" | "unavailable" | "unsupported";
  note: string;
  installSupported: boolean;
  downloadUrl: string | null;
  versions: string[];
}

export interface LoaderRecommendations {
  gameVersion: string;
  items: LoaderRecommendation[];
}

export interface LoaderInstallAccepted {
  taskId: string;
  versionId: string;
}

export interface InstallAccepted {
  taskId: string;
  versionId: string;
  includeAssets: boolean;
}


// --- Minecraft Multiplayer (52FRP) -----------------------------------------

export type MultiplayerEdition = "java" | "bedrock";

export interface MultiplayerAccount {
  provider: string;
  userId: number;
  displayName: string;
  email: string | null;
  group: string | null;
  verified: boolean | null;
  tunnelCount: number | null;
  tunnelLimit: number | null;
  balance: number | null;
  trafficRemaining: number | null;
}

export interface MultiplayerLocalService {
  found: boolean;
  pid: number | null;
  edition: MultiplayerEdition;
  ports: number[];
  detectedPort: number | null;
  source: string;
  message: string;
}

export interface MultiplayerNode {
  id: number;
  name: string;
  host: string;
  region: string | null;
  description: string | null;
  allowUdp: boolean;
  allowBigTraffic: boolean;
  needIcp: boolean;
  latencyMs: number | null;
  clients: number | null;
  load: number | null;
  score: number;
  available: boolean;
  reason: string | null;
}

export interface MultiplayerFrpc {
  running: boolean;
  version: string | null;
  path: string | null;
  sha256: string | null;
  logTail: string[];
}

export interface MultiplayerRoom {
  tunnelId: number;
  tunnelName: string;
  edition: MultiplayerEdition;
  instanceId: string;
  instanceName: string;
  versionId: string;
  nodeId: number;
  nodeName: string;
  publicHost: string;
  publicPort: number;
  localIp: string;
  localPort: number;
  localPid: number | null;
  latencyMs: number | null;
  createdAt: string;
  state: "online" | "offline" | "retained" | string;
  frpcRunning: boolean;
  frpcPid: number | null;
  message: string | null;
}

export interface MultiplayerOrphan {
  tunnelId: number;
  name: string;
  tunnelType: string;
  localPort: number;
  nodeName: string | null;
}

export interface MultiplayerStatus {
  provider: string;
  providerName: string;
  tokenConfigured: boolean;
  loggedIn: boolean;
  account: MultiplayerAccount | null;
  room: MultiplayerRoom | null;
  frpc: MultiplayerFrpc;
  lastError: string | null;
}

export interface MultiplayerProgress {
  step: string;
  label: string;
  status: "pending" | "running" | "ok" | "error" | string;
  detail: string | null;
}

export interface MultiplayerCreateRequest {
  instanceId: string;
  edition: MultiplayerEdition;
  localIp?: string | null;
  localPort?: number | null;
  nodeId?: number | null;
  autoNode?: boolean;
  name?: string | null;
}

export interface MultiplayerStopResult {
  localStopped: boolean;
  tunnelDeleted: boolean;
  message: string;
}

// --- Command wrappers ------------------------------------------------------

export const api = {
  bootstrap: () => command<Bootstrap>("get_bootstrap"),
  getSettings: () => command<Settings>("get_settings"),
  updateSettings: (settings: Settings) => command<Settings>("update_settings", { settings }),
  listVersions: (refresh = false) => command<VersionList>("list_versions", { refresh }),
  listInstalledVersions: () => command<InstalledVersion[]>("list_installed_versions"),
  installVersion: (versionId: string, includeAssets: boolean) =>
    command<InstallAccepted>("install_version", { versionId, includeAssets }),
  listTasks: () => command<TaskView[]>("list_tasks"),
  cancelTask: (taskId: string) => command<void>("cancel_task", { taskId }),
  clearFinishedTasks: () => command<void>("clear_finished_tasks"),
  versionDetail: (versionId: string) => command<VersionDetail>("get_version_detail", { versionId }),
  verifyVersion: (versionId: string, deepAssets = false) =>
    command<VerificationReport>("verify_version", { versionId, deepAssets }),
  listInstances: () => command<Instance[]>("list_instances"),
  createInstance: (request: {
    name: string;
    versionId: string;
    memoryMb?: number;
    modLoader?: ModLoaderInfo;
  }) => command<Instance>("create_instance", { request }),
  updateInstance: (instanceId: string, request: Record<string, unknown>) =>
    command<Instance>("update_instance", { instanceId, request }),
  deleteInstance: (instanceId: string, deleteFiles: boolean) =>
    command<void>("delete_instance", { instanceId, deleteFiles }),
  duplicateInstance: (instanceId: string, newName?: string) =>
    command<Instance>("duplicate_instance", { instanceId, newName }),
  exportInstanceArchive: (instanceId: string, destPath: string, overwrite: boolean) =>
    command<ExportSummary>("export_instance_archive", { instanceId, destPath, overwrite }),
  exportInstanceModpack: (
    instanceId: string,
    destPath: string,
    overwrite: boolean,
    name?: string,
    versionId?: string,
  ) =>
    command<ModpackExportSummary>("export_instance_modpack", {
      instanceId,
      destPath,
      overwrite,
      name,
      versionId,
    }),
  listMods: (instanceId: string) => command<ModEntry[]>("list_mods", { instanceId }),
  toggleMod: (instanceId: string, fileName: string, enabled: boolean) =>
    command<ModEntry>("toggle_mod", { instanceId, fileName, enabled }),
  deleteMod: (instanceId: string, fileName: string) =>
    command<void>("delete_mod", { instanceId, fileName }),
  importMod: (instanceId: string, sourcePath: string, overwrite: boolean) =>
    command<ModEntry>("import_mod", { instanceId, sourcePath, overwrite }),
  importMods: (instanceId: string, sourcePaths: string[], overwrite: boolean) =>
    command<ImportOutcome[]>("import_mods", { instanceId, sourcePaths, overwrite }),
  importModBytes: (instanceId: string, fileName: string, bytes: Uint8Array, overwrite: boolean) =>
    commandBytes<ModEntry>("import_mod_bytes", bytes, {
      "x-sr-instance": instanceId,
      "x-sr-name": encodeURIComponent(fileName),
      "x-sr-overwrite": overwrite ? "1" : "0",
    }),
  listShaders: (instanceId: string) =>
    command<ShaderEntry[]>("list_shaders", { instanceId }),
  toggleShader: (instanceId: string, fileName: string, enabled: boolean) =>
    command<ShaderEntry>("toggle_shader", { instanceId, fileName, enabled }),
  deleteShader: (instanceId: string, fileName: string) =>
    command<void>("delete_shader", { instanceId, fileName }),
  listWorlds: (instanceId: string) => command<WorldEntry[]>("list_worlds", { instanceId }),
  openWorldFolder: (instanceId: string, folder: string) =>
    command<void>("open_world_folder", { instanceId, folder }),
  listResourcepacks: (instanceId: string) =>
    command<ResourcePackEntry[]>("list_resourcepacks", { instanceId }),
  importResourcepacks: (instanceId: string, sourcePaths: string[], overwrite: boolean) =>
    command<ImportOutcome[]>("import_resourcepacks", { instanceId, sourcePaths, overwrite }),
  importResourcepackBytes: (
    instanceId: string,
    fileName: string,
    bytes: Uint8Array,
    overwrite: boolean,
  ) =>
    commandBytes<ResourcePackEntry>("import_resourcepack_bytes", bytes, {
      "x-sr-instance": instanceId,
      "x-sr-name": encodeURIComponent(fileName),
      "x-sr-overwrite": overwrite ? "1" : "0",
    }),
  setResourcepackEnabled: (instanceId: string, fileName: string, enabled: boolean) =>
    command<ResourcePackEntry>("set_resourcepack_enabled", { instanceId, fileName, enabled }),
  deleteResourcepack: (instanceId: string, fileName: string) =>
    command<void>("delete_resourcepack", { instanceId, fileName }),
  importShaders: (instanceId: string, sourcePaths: string[], overwrite: boolean) =>
    command<ImportOutcome[]>("import_shaders", { instanceId, sourcePaths, overwrite }),
  importShaderBytes: (
    instanceId: string,
    fileName: string,
    bytes: Uint8Array,
    overwrite: boolean,
  ) =>
    commandBytes<ShaderEntry>("import_shader_bytes", bytes, {
      "x-sr-instance": instanceId,
      "x-sr-name": encodeURIComponent(fileName),
      "x-sr-overwrite": overwrite ? "1" : "0",
    }),
  shaderSupport: (instanceId: string) =>
    command<ShaderSupport>("shader_support", { instanceId }),
  installIris: (instanceId: string) =>
    command<ModEntry>("install_iris", { instanceId }),
  installOptiFine: (instanceId: string) =>
    command<ModEntry>("install_optifine", { instanceId }),
  deleteVersion: (versionId: string, force = false) =>
    command<VersionDeleteResult>("delete_version", { versionId, force }),
  versionDeletePlan: (versionId: string) =>
    command<VersionDeletePlan>("get_version_delete_plan", { versionId }),
  openLauncherPath: (target: string, id?: string) =>
    command<void>("open_launcher_path", { target, id }),
  inspectInstanceArchive: (path: string) =>
    command<ImportCheck>("inspect_instance_archive", { path }),
  importInstanceArchive: (path: string, name?: string) =>
    command<Instance>("import_instance_archive", { path, name }),
  launchInstance: (instanceId: string) => command<LaunchResult>("launch_instance", { instanceId }),
  stopInstance: (instanceId: string) => command<void>("stop_instance", { instanceId }),
  runningInstances: () => command<string[]>("running_instances"),
  listJava: (refresh = false) => command<JavaInfo[]>("list_java", { refresh }),
  addJavaPath: (path: string) => command<JavaInfo>("add_java_path", { path }),
  javaRuntimeOptions: () => command<JavaRuntimeOptions>("java_runtime_options"),
  installJavaRuntime: (major: number) =>
    command<JavaInfo>("install_java_runtime", { major }),
  checkJavaForVersion: (versionId: string) =>
    command<JavaCheck>("check_java_for_version", { versionId }),
  listAccounts: () => command<AccountView[]>("list_accounts"),
  addOfflineAccount: (username: string, skinId?: string | null, skinModel?: SkinModel | null) =>
    command<Account>("add_offline_account", { username, skinId, skinModel }),
  listSkins: () => command<SkinListView>("list_skins"),
  searchResources: (query: ResourceSearchQuery) =>
    command<ResourceSearchPage>("search_resources", { query }),
  searchCurseforge: (params: {
    kind: ResourceKind;
    query: string;
    gameVersion?: string | null;
    categoryId?: number | null;
    loader?: string | null;
    sort: string;
    offset?: number;
    limit?: number;
  }) => command<ResourceSearchPage>("search_curseforge", params),
  getCurseforgeProject: (id: number, kind: ResourceKind, gameVersion?: string | null) =>
    command<ResourceProject>("get_curseforge_project", { id, kind, gameVersion }),
  getCurseforgeFiles: (
    id: number,
    kind: ResourceKind,
    gameVersion?: string | null,
    loader?: string | null,
  ) => command<ResourceVersion[]>("get_curseforge_files", { id, kind, gameVersion, loader }),
  installCurseforge: (
    projectId: number,
    fileId: number,
    kind: ResourceKind,
    target: ResourceTarget,
  ) => command<ResourceTaskAccepted>("install_curseforge", { projectId, fileId, kind, target }),
  resourceGameVersions: () =>
    command<GameVersionInfo[]>("resource_game_versions"),
  getResourceProject: (id: string) =>
    command<ResourceProject>("get_resource_project", { id }),
  getResourceVersions: (id: string, gameVersion?: string | null, loader?: string | null) =>
    command<ResourceVersion[]>("get_resource_versions", { id, gameVersion, loader }),
  installResource: (projectId: string, versionId: string, target: ResourceTarget) =>
    command<ResourceTaskAccepted>("install_resource", { projectId, versionId, target }),
  importModpack: (instanceId: string, path: string, allowIncomplete: boolean) =>
    command<ResourceTaskAccepted>("import_modpack", {
      instanceId,
      path,
      allowIncomplete,
    }),
  importModpackBytes: (
    instanceId: string,
    fileName: string,
    bytes: Uint8Array,
    allowIncomplete: boolean,
  ) =>
    commandBytes<ResourceTaskAccepted>("import_modpack_bytes", bytes, {
      "x-sr-instance": instanceId,
      "x-sr-name": encodeURIComponent(fileName),
      "x-sr-allow-incomplete": allowIncomplete ? "1" : "0",
    }),
  curseforgeKeyStatus: () => command<string>("curseforge_key_status"),
  setCurseforgeApiKey: (key: string) => command<void>("set_curseforge_api_key", { key }),
  clearCurseforgeApiKey: () => command<void>("clear_curseforge_api_key"),
  testCurseforgeApiKey: () => command<void>("test_curseforge_api_key"),
  assistantStatus: () => command<AssistantStatus>("assistant_status"),
  setDeepseekApiKey: (key: string) => command<void>("set_deepseek_api_key", { key }),
  clearDeepseekApiKey: () => command<void>("clear_deepseek_api_key"),
  testDeepseekApiKey: () => command<void>("test_deepseek_api_key"),
  assistantLocalReport: (instanceId: string, crashFile?: string | null) =>
    command<AssistantLocalReport>("assistant_local_report", { instanceId, crashFile }),
  assistantConversations: () =>
    command<AssistantConversationSummary[]>("assistant_conversations"),
  assistantConversation: (id: string) =>
    command<AssistantConversation>("assistant_conversation", { id }),
  assistantDeleteConversation: (id: string) =>
    command<void>("assistant_delete_conversation", { id }),
  assistantChat: (
    conversationId: string | null,
    instanceId: string | null,
    message: string,
  ) => command<AssistantReply>("assistant_chat", { conversationId, instanceId, message }),
  assistantRepairPlan: (instanceId: string, crashFile?: string | null) =>
    command<RepairPlan>("assistant_repair_plan", { instanceId, crashFile }),
  assistantApplyRepair: (planId: string) =>
    command<RepairResult>("assistant_apply_repair", { planId }),
  assistantRollbackRepair: (instanceId: string, token: string) =>
    command<void>("assistant_rollback_repair", { instanceId, token }),
  assistantModPlan: (instanceId: string, projectId: string, versionId?: string | null) =>
    command<ModInstallPlan>("assistant_mod_plan", { instanceId, projectId, versionId }),
  assistantApplyModPlan: (planId: string) =>
    command<ModInstallResult>("assistant_apply_mod_plan", { planId }),
  assistantRollbackModInstall: (instanceId: string, token: string) =>
    command<void>("assistant_rollback_mod_install", { instanceId, token }),
  defaultResourceDir: () => command<string>("default_resource_dir"),
  importSkinFile: (path: string, model: SkinModel) =>
    command<Skin>("import_skin_file", { path, model }),
  importSkinBytes: (fileName: string, bytes: Uint8Array, model: SkinModel) =>
    commandBytes<Skin>("import_skin_bytes", bytes, {
      "x-sr-name": encodeURIComponent(fileName),
      "x-sr-model": model,
    }),
  fetchSkinByName: (username: string) =>
    command<Skin>("fetch_skin_by_name", { username }),
  deleteSkin: (skinId: string) => command<void>("delete_skin", { skinId }),
  setAccountSkin: (accountId: string, skinId: string | null, model?: SkinModel | null) =>
    command<Account>("set_account_skin", { accountId, skinId, model }),
  applyAccountSkin: (accountId: string) =>
    command<SkinApplyResult>("apply_account_skin", { accountId }),
  removeAccount: (accountId: string) => command<void>("remove_account", { accountId }),
  setActiveAccount: (accountId: string) => command<void>("set_active_account", { accountId }),
  beginMicrosoftLogin: () => command<DeviceCodeView>("begin_microsoft_login"),
  refreshMicrosoftAccount: (accountId: string) =>
    command<Account>("refresh_microsoft_account", { accountId }),
  pollMicrosoftLogin: (deviceCode: string) =>
    command<MicrosoftPoll>("poll_microsoft_login", { deviceCode }),
  loaderRecommendations: (gameVersion: string) =>
    command<LoaderRecommendations>("get_loader_recommendations", { gameVersion }),
  openTrustedUrl: (url: string) => command<void>("open_trusted_url", { url }),
  multiplayerStatus: () => command<MultiplayerStatus>("multiplayer_status"),
  multiplayerConnect: (token: string) =>
    command<MultiplayerAccount>("multiplayer_connect", { token }),
  multiplayerLogout: () => command<void>("multiplayer_logout"),
  multiplayerNodes: (edition: MultiplayerEdition) =>
    command<MultiplayerNode[]>("multiplayer_nodes", { edition }),
  multiplayerDetect: (instanceId: string, edition: MultiplayerEdition) =>
    command<MultiplayerLocalService>("multiplayer_detect", { instanceId, edition }),
  multiplayerCreateRoom: (request: MultiplayerCreateRequest) =>
    command<MultiplayerRoom>("multiplayer_create_room", { request }),
  multiplayerStopRoom: (deleteTunnel: boolean) =>
    command<MultiplayerStopResult>("multiplayer_stop_room", { deleteTunnel }),
  multiplayerRestartFrpc: () => command<MultiplayerFrpc>("multiplayer_restart_frpc"),
  multiplayerOrphans: () => command<MultiplayerOrphan[]>("multiplayer_orphans"),
  multiplayerDeleteOrphan: (tunnelId: number) =>
    command<void>("multiplayer_delete_orphan", { tunnelId }),
  listFabricLoaders: (gameVersion: string) =>
    command<FabricLoader[]>("list_fabric_loaders", { gameVersion }),
  installFabric: (gameVersion: string, loaderVersion: string) =>
    command<LoaderInstallAccepted>("install_fabric", { gameVersion, loaderVersion }),
  listQuiltLoaders: (gameVersion: string) =>
    command<QuiltLoader[]>("list_quilt_loaders", { gameVersion }),
  installQuilt: (gameVersion: string, loaderVersion: string) =>
    command<LoaderInstallAccepted>("install_quilt", { gameVersion, loaderVersion }),
  loaderResourceVersions: (loader: string, gameVersion: string) =>
    command<LoaderVersionOption[]>("loader_resource_versions", { loader, gameVersion }),
  downloadLoaderResource: (
    loader: string,
    gameVersion: string,
    loaderVersion: string,
    variant: string | null,
    directory: string,
  ) =>
    command<ResourceTaskAccepted>("download_loader_resource", {
      loader,
      gameVersion,
      loaderVersion,
      variant,
      directory,
    }),
  installLoaderResource: (
    loader: string,
    gameVersion: string,
    loaderVersion: string,
    variant: string | null,
    instanceId: string,
  ) =>
    command<ResourceTaskAccepted>("install_loader_resource", {
      loader,
      gameVersion,
      loaderVersion,
      variant,
      instanceId,
    }),
  listForgeVersions: (gameVersion: string) =>
    command<string[]>("list_forge_versions", { gameVersion }),
  listNeoForgeVersions: (gameVersion: string) =>
    command<string[]>("list_neoforge_versions", { gameVersion }),
  installForge: (gameVersion: string, loaderVersion?: string) =>
    command<LoaderInstallAccepted>("install_forge", { gameVersion, loaderVersion }),
  installNeoForge: (gameVersion: string, loaderVersion?: string) =>
    command<LoaderInstallAccepted>("install_neoforge", { gameVersion, loaderVersion }),
  diagnostics: (runNetworkCheck: boolean) =>
    command<DiagnosticsReport>("get_diagnostics", { runNetworkCheck }),
  listLogs: (limit = 20) => command<LogFileInfo[]>("list_logs", { limit }),
  readLog: (path: string, lines = 200) => command<string>("read_log", { path, lines }),
  cleanupLogs: () => command<LogCleanup>("cleanup_logs"),
  deleteLog: (path: string) => command<void>("delete_log", { path }),
  exportDiagnostics: (runNetworkCheck: boolean) =>
    command<string>("export_diagnostics", { runNetworkCheck }),
  listCrashReports: (instanceId: string) =>
    command<CrashFile[]>("list_crash_reports", { instanceId }),
  analyzeCrash: (instanceId: string, fileName: string) =>
    command<CrashAnalysis>("analyze_crash", { instanceId, fileName }),
  deleteCrashReport: (instanceId: string, fileName: string) =>
    command<void>("delete_crash_report", { instanceId, fileName }),
};
