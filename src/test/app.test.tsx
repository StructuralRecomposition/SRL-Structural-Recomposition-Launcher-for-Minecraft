import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import {
  fixtureBootstrap,
  fixtureDetail,
  fixtureResourcePacks,
  fixtureSkinList,
  fixtureWorlds,
} from "./fixtures";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
let multiplayerLoggedIn = false;
let multiplayerConfigured = false;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

beforeEach(() => {
  multiplayerLoggedIn = false;
  multiplayerConfigured = false;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "get_bootstrap":
        return structuredClone(fixtureBootstrap);
      case "list_tasks":
        return [];
      case "list_java":
        return structuredClone(fixtureBootstrap.java);
      case "get_diagnostics":
        return {
          generatedAt: "2026-10-26T00:00:00Z",
          platform: "macos x86_64",
          gameDir: "/tmp/game",
          checks: [],
          java: [],
          installedVersions: 0,
          instances: 0,
        };
      case "list_logs":
        return [
          {
            path: "/tmp/sr-test/logs/a.log",
            name: "a.log",
            size: 10,
            modified: "2026-10-26T00:00:00Z",
          },
        ];
      case "delete_log":
        return null;
      case "cleanup_logs":
        return { removed: 1, bytes: 10 };
      case "loader_resource_versions":
        if (args?.loader === "iris") {
          return [
            {
              version: "1.8.0",
              label: "1.8.0 (fabric)",
              variant: "fabric",
              stable: true,
            },
          ];
        }
        if (args?.loader === "optifine") {
          return [
            {
              version: "HD_U J1",
              label: "HD_U J1",
              variant: null,
              stable: true,
            },
          ];
        }
        return [
          {
            version: "0.16.10",
            label: "0.16.10",
            variant: null,
            stable: true,
          },
        ];
      case "download_loader_resource":
        return { taskId: "loader-download", title: "下载 Fabric" };
      case "install_loader_resource":
        return { taskId: "loader-install", title: "安装 Fabric" };
      case "java_runtime_options":
        return {
          majors: [8, 11, 17, 21, 25],
          lts: [8, 11, 17, 21, 25],
          recommended: 21,
        };
      case "install_java_runtime":
        return {
          path: "/tmp/sr/java/temurin-21/bin/java",
          versionString: "21.0.9",
          major: 21,
          vendor: "Eclipse Adoptium",
          arch: "x64",
          source: "downloaded",
        };
      case "get_version_detail":
        return structuredClone(fixtureDetail);
      case "launch_instance":
        return {
          instanceId: "abc123def456",
          pid: 4242,
          logPath: "/tmp/sr-test/logs/instances/abc.log",
          warnings: [],
        };
      case "update_settings":
        return args?.settings;
      case "list_accounts":
        return structuredClone(fixtureBootstrap.accounts);
      case "list_skins":
        return structuredClone(fixtureSkinList);
      case "list_worlds":
        return structuredClone(fixtureWorlds);
      case "resource_game_versions":
        return [
          { version: "1.21.1", versionType: "release", date: "2024-08-08T00:00:00Z", major: false },
          { version: "1.20.4", versionType: "release", date: "2023-12-07T00:00:00Z", major: false },
        ];
      case "search_resources":
        return {
          hits: [
            {
              id: "AANobbMI",
              kind: "mod",
              slug: "sodium",
              title: "Sodium",
              description: "A high-performance rendering engine replacement.",
              author: "jellysquid3",
              categories: ["fabric", "optimization"],
              versions: ["1.21.1"],
              downloads: 1234567,
              follows: 1234,
              iconUrl: null,
              dateModified: "2026-10-01T00:00:00Z",
              latestVersion: "v1",
              license: "LGPL-3.0",
              clientSide: "required",
              serverSide: "unsupported",
            },
          ],
          offset: 0,
          limit: 20,
          totalHits: 1,
        };
      case "get_resource_project":
        return {
          id: "AANobbMI",
          kind: "mod",
          slug: "sodium",
          title: "Sodium",
          description: "A high-performance rendering engine replacement.",
          author: "jellysquid3",
          categories: ["fabric", "optimization"],
          downloads: 1234567,
          follows: 1234,
          iconUrl: null,
          gameVersions: ["1.21.1"],
          loaders: ["fabric"],
          license: "LGPL-3.0",
          sourceUrl: "https://github.com/CaffeineMC/sodium",
          issuesUrl: null,
          wikiUrl: null,
          websiteUrl: "https://modrinth.com/mod/sodium",
          published: "2020-01-01T00:00:00Z",
          updated: "2026-10-01T00:00:00Z",
        };
      case "get_resource_versions":
        return [
          {
            id: "ver1",
            projectId: "AANobbMI",
            name: "Sodium 1.0",
            versionNumber: "mc1.21.1-1.0",
            versionType: "release",
            datePublished: "2026-10-01T00:00:00Z",
            downloads: 100,
            gameVersions: ["1.21.1"],
            loaders: ["fabric"],
            files: [
              {
                filename: "sodium.jar",
                url: "https://cdn.modrinth.com/data/AANobbMI/versions/ver1/sodium.jar",
                size: 1024,
                primary: true,
                sha1: "abc123",
                sha512: null,
                fileType: null,
              },
            ],
            dependencies: [],
          },
        ];
      case "default_resource_dir":
        return "/tmp";
      case "curseforge_key_status":
        return "builtin";
      case "multiplayer_status":
        return {
          provider: "frp52",
          providerName: "52FRP",
          tokenConfigured: multiplayerConfigured,
          loggedIn: multiplayerLoggedIn,
          account: multiplayerLoggedIn
            ? {
                provider: "frp52",
                userId: 42,
                displayName: "max",
                email: null,
                group: "VIP",
                verified: true,
                tunnelCount: 0,
                tunnelLimit: 5,
                balance: 12.5,
                trafficRemaining: 1048576,
              }
            : null,
          room: null,
          frpc: { running: false, version: null, path: null, sha256: null, logTail: [] },
          lastError: null,
        };
      case "multiplayer_nodes":
        return [];
      case "multiplayer_detect":
        return {
          found: true,
          pid: 123,
          edition: "java",
          ports: [25565],
          detectedPort: 25565,
          source: "log",
          message: "从游戏日志识别到局域网端口 25565",
        };
      case "multiplayer_orphans":
        return [];
      case "search_curseforge":
        return {
          hits: [
            {
              id: "307740",
              kind: "map",
              source: "curseforge",
              slug: "oneblock-wilderness-bound",
              title: "Oneblock Test",
              description: "A skyblock-style survival map.",
              author: "Crimson Creations",
              categories: ["Survival"],
              versions: ["1.21.1"],
              downloads: 10645801,
              follows: 0,
              iconUrl: null,
              dateModified: "2026-10-01T00:00:00Z",
              latestVersion: "9190157",
              license: null,
              clientSide: null,
              serverSide: null,
            },
          ],
          offset: 0,
          limit: 20,
          totalHits: 1,
        };
      case "get_curseforge_project":
        return {
          id: "307740",
          kind: "map",
          source: "curseforge",
          slug: "oneblock-wilderness-bound",
          title: "Oneblock Test",
          description: "A skyblock-style survival map.",
          author: "Crimson Creations",
          categories: ["Survival"],
          downloads: 10645801,
          follows: 0,
          iconUrl: null,
          gameVersions: ["1.21.1"],
          loaders: [],
          license: null,
          sourceUrl: null,
          issuesUrl: null,
          wikiUrl: null,
          websiteUrl: "https://www.curseforge.com/minecraft/worlds/oneblock-wilderness-bound",
          published: null,
          updated: "2026-10-01T00:00:00Z",
        };
      case "get_curseforge_files":
        return [
          {
            id: "9190157",
            projectId: "307740",
            name: "Oneblock Test.zip",
            versionNumber: "Oneblock Test.zip",
            versionType: "release",
            datePublished: "2026-10-01T00:00:00Z",
            downloads: 0,
            gameVersions: ["1.21.1"],
            loaders: [],
            files: [
              {
                filename: "Oneblock Test.zip",
                url: "https://edge.forgecdn.net/files/9190/157/Oneblock%20Test.zip",
                size: 1024,
                primary: true,
                sha1: "abc123",
                sha512: null,
                fileType: null,
              },
            ],
            dependencies: [],
          },
        ];
      case "install_curseforge":
        return { taskId: "map-task", title: "安装地图" };
      case "list_resourcepacks":
        return structuredClone(fixtureResourcePacks);
      case "open_world_folder":
      case "open_launcher_path":
        return null;
      case "add_offline_account":
        return {
          id: "acc2",
          username: "SkinUser",
          uuid: "11111111-1111-3111-8111-111111111111",
          kind: "offline",
          status: "offline",
          createdAt: "2026-10-25T08:30:00Z",
          lastUsedAt: null,
          skinId: "builtin-slim-alex",
          skinModel: "slim",
        };
      case "list_mods":
        return [];
      case "list_shaders":
        return [];
      case "shader_support":
        return { iris: null, optifine: null };
      case "list_fabric_loaders":
      case "list_quilt_loaders":
        return [];
      case "get_loader_recommendations":
        return {
          gameVersion: "1.20.4",
          items: [
            {
              id: "fabric",
              name: "Fabric",
              version: "0.16.0",
              status: "recommended",
              note: "稳定版",
              installSupported: true,
              downloadUrl: null,
              versions: ["0.16.0"],
            },
            {
              id: "forge",
              name: "Forge",
              version: "49.0.1",
              status: "available",
              note: "官方安装器",
              installSupported: true,
              downloadUrl: "https://bmclapi2.bangbang93.com/forge/download",
              versions: ["49.0.1"],
            },
            {
              id: "optifine",
              name: "OptiFine",
              version: "HD_U J1",
              status: "available",
              note: "光影支持",
              installSupported: false,
              downloadUrl: "https://bmclapi2.bangbang93.com/optifine/1.20.4/HD_U/J1",
              versions: ["HD_U J1"],
            },
          ],
        };
      case "list_installed_versions":
        return structuredClone(fixtureBootstrap.installedVersions);
      case "list_versions":
        return {
          latestRelease: "1.20.4",
          latestSnapshot: "26.4-snapshot-5",
          versions: [
            {
              id: "1.20.3",
              type: "release",
              url: "https://example.invalid/1.20.3.json",
              time: "2026-09-01T10:00:00Z",
              releaseTime: "2026-09-01T10:00:00Z",
              sha1: null,
              complianceLevel: 1,
            },
            {
              id: "1.20.4",
              type: "release",
              url: "https://example.invalid/1.20.4.json",
              time: "2026-10-01T10:00:00Z",
              releaseTime: "2026-10-01T10:00:00Z",
              sha1: null,
              complianceLevel: 1,
            },
          ],
          installed: ["1.20.4"],
          source: "cache",
          warning: null,
        };
      case "list_crash_reports":
        return [];
      case "assistant_conversations":
        return [];
      case "assistant_conversation":
        return {
          id: "conv1",
          title: "历史对话",
          instanceId: "abc123def456",
          createdAt: "2026-10-26T12:00:00Z",
          updatedAt: "2026-10-26T12:00:00Z",
          messages: [
            {
              role: "user",
              content: "之前的问题",
              traces: [],
              proposals: [],
              createdAt: "2026-10-26T12:00:00Z",
            },
            {
              role: "assistant",
              content: "**历史** 回答",
              traces: [],
              proposals: [],
              createdAt: "2026-10-26T12:00:01Z",
            },
          ],
        };
      case "assistant_delete_conversation":
        return null;
      case "assistant_status":
        return {
          configured: false,
          model: "deepseek-chat",
          baseUrl: "https://api.deepseek.com",
          auditPath: "/tmp/sr-test/assistant-audit.jsonl",
        };
      case "assistant_local_report":
        return {
          instanceId: "abc123def456",
          instanceName: "1.20.4 实例",
          versionId: "1.20.4",
          loader: "Fabric 0.16.10",
          memoryMb: 4096,
          javaPath: null,
          mods: ["sodium.jar"],
          crash: {
            fileName: "crash-2026-10-26.txt",
            size: 2048,
            lineCount: 120,
            description: "Rendering overlay",
            minecraftVersion: "1.20.4",
            javaVersion: "21.0.9",
            loader: null,
            exceptions: [
              {
                kind: "java.lang.NoSuchMethodError",
                message: "Renderer.m_12345()V",
                line: 7,
              },
            ],
            causedBy: [],
            frames: [
              { line: 8, text: "at com.example.mod.Renderer.render(Renderer.java:42)" },
            ],
            suspectedMods: [
              { name: "examplemod", reason: "模组名称出现在异常或堆栈行", evidenceLine: 8 },
            ],
            modList: ["examplemod"],
            excerpt: "---- Minecraft Crash Report ----",
          },
          log: null,
          facts: ["崩溃报告共 120 行", "mods/ 目录中有 1 个文件"],
          limits: ["本地解析只引用原始行"],
        };
      case "assistant_chat": {
        const text = String(args?.message ?? "");
        if (text.includes("搜索")) {
          return {
            content: "找到 Sodium，建议安装这个版本。",
            toolTraces: [{ name: "prepare_mod_install", summary: "已生成模组安装计划", ok: true }],
            proposals: [
              {
                kind: "mod_install",
                id: "modplan1",
                title: "模组安装计划",
                summary: "1 个文件（含必需依赖）",
                plan: {
                  id: "modplan1",
                  instanceId: "abc123def456",
                  instanceName: "1.20.4 实例",
                  createdAt: "2026-10-26T12:00:00Z",
                  rootProject: "AANobbMI",
                  gameVersion: "1.20.4",
                  loader: "fabric",
                  entries: [
                    {
                      projectId: "AANobbMI",
                      title: "Sodium",
                      versionId: "ver1",
                      versionNumber: "1.0",
                      fileName: "sodium.jar",
                      url: "https://cdn.modrinth.com/x.jar",
                      size: 1024,
                      sha1: "abc123",
                      requiredBy: [],
                    },
                  ],
                  warnings: [],
                  notes: ["下载先写入临时目录并校验 SHA-1"],
                },
              },
            ],
            model: "deepseek-chat",
          };
        }
        return {
          content: "**结论**：根据 L7 的异常，问题来自 examplemod。\n\n- 建议先提高内存\n- 再禁用该模组",
          toolTraces: [{ name: "get_instance_diagnostics", summary: "已读取实例诊断", ok: true }],
          proposals: [
            {
              kind: "repair",
              id: "plan123",
              title: "修复计划",
              summary: "1 个低风险操作",
              plan: {
                id: "plan123",
                instanceId: "abc123def456",
                instanceName: "1.20.4 实例",
                createdAt: "2026-10-26T12:00:00Z",
                actions: [
                  {
                    kind: "set_memory",
                    description: "将实例最大内存从 4096 MB 提高到 6144 MB",
                    target: "abc123def456",
                    from: "4096",
                    to: "6144",
                    evidence: ["crash-2026-10-26.txt:L7"],
                    risk: "低：只修改实例 instance.json",
                  },
                ],
                notes: ["执行前会备份 instance.json"],
                rollback: "使用回滚令牌恢复",
              },
            },
          ],
          model: "deepseek-chat",
          conversationId: "conv-new",
          title: "测试对话",
        };
      }
      case "assistant_repair_plan":
        return {
          id: "plan123",
          instanceId: "abc123def456",
          instanceName: "1.20.4 实例",
          createdAt: "2026-10-26T12:00:00Z",
          actions: [
            {
              kind: "set_memory",
              description: "将实例最大内存从 4096 MB 提高到 6144 MB",
              target: "abc123def456",
              from: "4096",
              to: "6144",
              evidence: ["crash-2026-10-26.txt:L7"],
              risk: "低：只修改实例 instance.json",
            },
          ],
          notes: ["执行前会备份 instance.json"],
          rollback: "使用回滚令牌恢复",
        };
      case "assistant_apply_repair":
        return {
          token: "repair-abc",
          backupDir: "/tmp/sr-test/backups/assistant/repair-abc",
          applied: ["内存已设为 6144 MB"],
          warnings: [],
        };
      case "assistant_rollback_repair":
        return null;
      case "assistant_mod_plan":
        return null;
      case "assistant_apply_mod_plan":
        return {
          token: "mod-abc",
          backupDir: "/tmp/sr-test/backups/assistant/mod-abc",
          installed: ["sodium.jar"],
          warnings: [],
        };
      case "assistant_rollback_mod_install":
        return null;
      case "delete_crash_report":
        return null;
      default:
        return null;
    }
  });
});

describe("SR Launcher UI", () => {
  it("loads the dashboard from the backend without mock data", async () => {
    render(<App />);
    expect(await screen.findByText("生存世界", {}, { timeout: 3000 })).toBeInTheDocument();
    expect(screen.getByText("SRL")).toBeInTheDocument();
    expect(screen.getByText(/Minecraft 1\.20\.4/)).toBeInTheDocument();
  });

  it("launches through the real IPC command and reports the pid", async () => {
    render(<App />);
    const button = await screen.findByRole("button", { name: /启动游戏/ }, { timeout: 3000 });
    fireEvent.click(button);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("launch_instance", {
        instanceId: "abc123def456",
      });
    });
    expect(await screen.findByText("游戏进程已启动")).toBeInTheDocument();
    expect(screen.getByText("PID 4242")).toBeInTheDocument();
  });

  it("shows an honest empty state on the tasks page", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "下载任务" }));
    expect(await screen.findByText("没有下载任务")).toBeInTheDocument();
    expect(screen.getByText(/所有进度均为真实数据/)).toBeInTheDocument();
  });

  it("shows the 52FRP connection guidance when no token is configured", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "Minecraft 联机" });
    fireEvent.click(screen.getByRole("button", { name: "Minecraft 联机" }));
    expect(await screen.findByText("先连接 52FRP")).toBeInTheDocument();
  });

  it("shows the multiplayer create form after the account is connected", async () => {
    multiplayerConfigured = true;
    multiplayerLoggedIn = true;
    render(<App />);
    await screen.findByRole("button", { name: "Minecraft 联机" });
    fireEvent.click(screen.getByRole("button", { name: "Minecraft 联机" }));
    expect(await screen.findByRole("button", { name: "创建房间" })).toBeInTheDocument();
    expect(screen.getByText("Java 版")).toBeInTheDocument();
    expect(screen.getAllByText("max").length).toBeGreaterThan(0);
    fireEvent.mouseEnter(screen.getByLabelText(/说明：节点是/));
    expect(await screen.findByRole("tooltip")).toHaveTextContent("节点是 52FRP");
    fireEvent.click(screen.getByRole("button", { name: "帮助" }));
    expect(await screen.findByText(/先准备 52FRP/)).toBeInTheDocument();
    expect(screen.getByText("自定义局域网联机（Lan Server Properties）")).toBeInTheDocument();
  });

  it("offers loader downloads inside the resources page", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    fireEvent.click(await screen.findByRole("tab", { name: "加载器" }));
    expect(await screen.findByText("Minecraft 版本")).toBeInTheDocument();
    for (const name of ["Fabric", "Quilt", "Forge", "NeoForge", "Iris", "OptiFine"]) {
      expect(screen.getAllByRole("button", { name }).length).toBeGreaterThan(0);
    }
    expect(screen.getByRole("button", { name: "安装到实例" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下载到本地" })).toBeInTheDocument();
  });

  it("offers instance import and export controls", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "游戏实例" }));
    expect(await screen.findByRole("button", { name: "导入实例" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "导出实例" }));
    expect(await screen.findByText("导出文件路径")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    fireEvent.click(screen.getByRole("button", { name: "管理模组" }));
    expect(await screen.findByText("没有模组文件")).toBeInTheDocument();
  });

  it("browses installed versions with quick config and delete actions", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "版本管理" }));
    fireEvent.click(await screen.findByRole("button", { name: /已安装/ }));
    expect(await screen.findByText("1.20.4")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建实例" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "删除 1.20.4" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开 1.20.4 文件夹" })).toBeInTheDocument();
  });

  it("searches community resources and opens the install dialog", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    expect(await screen.findByText("Sodium")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: /Sodium/ })[0]);
    expect(await screen.findByText("可用版本")).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "安装" }));
    expect(await screen.findByText("安装到实例")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "开始" })).toBeInTheDocument();
  });

  it("searches CurseForge maps and opens the install dialog", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    fireEvent.click(await screen.findByRole("tab", { name: "地图" }));
    expect(await screen.findByText("Oneblock Test")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: /Oneblock Test/ })[0]);
    expect(await screen.findByText("可用版本")).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "安装" }));
    expect(await screen.findByText("安装到实例")).toBeInTheDocument();
    expect(await screen.findByText(/地图会解压到实例 saves/)).toBeInTheDocument();
  });

  it("searches mods on CurseForge after switching source", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    await screen.findByText("Sodium");
    fireEvent.click(screen.getByRole("radio", { name: "CurseForge" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "search_curseforge",
        expect.objectContaining({ kind: "mod" }),
      ),
    );
  });

  it("installs a Java runtime in one click", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "Java 环境" }));
    expect(await screen.findByText("下载并安装 Java")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下载并安装" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("install_java_runtime", { major: 21 }),
    );
  });

  it("opens local modpack import", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    fireEvent.click(await screen.findByRole("button", { name: "导入本地整合包" }));
    expect(await screen.findByText("开始导入")).toBeInTheDocument();
  });

  it("opens the map and material managers per instance", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "游戏实例" }));

    fireEvent.click(await screen.findByRole("button", { name: "地图" }));
    expect(await screen.findByRole("button", { name: "打开存档文件夹" })).toBeInTheDocument();
    expect(screen.getAllByText("test").length).toBeGreaterThan(0);
    fireEvent.click(screen.getAllByRole("button", { name: "关闭" })[0]);

    fireEvent.click(await screen.findByRole("button", { name: "材质" }));
    expect(await screen.findByText("test-pack.zip")).toBeInTheDocument();
    expect(screen.getByText("皮肤自动管理")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "选择 .zip 材质包" })).toBeInTheDocument();
  });

  it("opens the shader manager with real import controls", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "游戏实例" }));
    fireEvent.click(await screen.findByRole("button", { name: "光影管理" }));
    expect(await screen.findByText("原版客户端无法加载光影")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "选择 .zip 光影包" })).toBeInTheDocument();
    expect(screen.getByText("拖入 .zip 光影包到这里")).toBeInTheDocument();
  });

  it("shows download versions separated by release type", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "版本管理" }));
    expect((await screen.findAllByText("1.20.4")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("正式版").length).toBeGreaterThan(0);
    expect(screen.getByRole("tab", { name: /快照/ })).toBeInTheDocument();
  });

  it("downloads a selected loader version to a local directory", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    fireEvent.click(await screen.findByRole("tab", { name: "加载器" }));
    const download = await screen.findByRole("button", { name: "下载到本地" });
    await waitFor(() => expect(download).toBeEnabled());
    fireEvent.click(download);
    expect(await screen.findByText("下载目录")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "开始下载" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "download_loader_resource",
        expect.objectContaining({ loader: "fabric", gameVersion: "1.21.1" }),
      ),
    );
  });

  it("opens the combined version + loader install dialog", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "版本管理" }));
    const installButtons = await screen.findAllByRole("button", { name: "安装" });
    fireEvent.click(installButtons[0]);
    expect(await screen.findByText(/安装 Minecraft 1\.20\./)).toBeInTheDocument();
    expect(screen.getAllByText("模组加载器").length).toBeGreaterThan(0);
    expect(screen.getByText("光影插件")).toBeInTheDocument();
  });

  it("lets the combined installer pick an Iris version", async () => {
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_fabric_loaders") {
        return [{ version: "0.16.10", stable: true }];
      }
      return base!(cmd, args);
    });
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "版本管理" }));
    const installButtons = await screen.findAllByRole("button", { name: "安装" });
    fireEvent.click(installButtons[0]);
    expect(await screen.findByText(/安装 Minecraft 1\.20\./)).toBeInTheDocument();

    const loaderSelect = screen
      .getByText("模组加载器")
      .closest(".field")?.querySelector("select") as HTMLSelectElement;
    fireEvent.change(loaderSelect, { target: { value: "fabric" } });
    await screen.findByText("0.16.10");

    const shaderSelect = screen
      .getByText("光影插件")
      .closest(".field")?.querySelector("select") as HTMLSelectElement;
    fireEvent.change(shaderSelect, { target: { value: "iris" } });
    expect(await screen.findByText("Iris 版本")).toBeInTheDocument();
    expect(await screen.findByText("1.8.0 (fabric)")).toBeInTheDocument();
  });

  it("lets the combined installer pick an OptiFine build", async () => {
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_forge_versions") {
        return ["52.0.16"];
      }
      return base!(cmd, args);
    });
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "版本管理" }));
    const installButtons = await screen.findAllByRole("button", { name: "安装" });
    fireEvent.click(installButtons[0]);
    expect(await screen.findByText(/安装 Minecraft 1\.20\./)).toBeInTheDocument();

    const loaderSelect = screen
      .getByText("模组加载器")
      .closest(".field")?.querySelector("select") as HTMLSelectElement;
    fireEvent.change(loaderSelect, { target: { value: "forge" } });
    await screen.findByText("52.0.16");

    const shaderSelect = screen
      .getByText("光影插件")
      .closest(".field")?.querySelector("select") as HTMLSelectElement;
    fireEvent.change(shaderSelect, { target: { value: "optifine" } });
    expect(await screen.findByText("OptiFine 版本")).toBeInTheDocument();
    expect(await screen.findByText("HD_U J1")).toBeInTheDocument();
  });

  it("deletes and cleans logs from the diagnostics page", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "诊断中心" }));
    expect(await screen.findByText("a.log")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "删除 a.log" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("delete_log", {
        path: "/tmp/sr-test/logs/a.log",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "清理旧日志" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("cleanup_logs", undefined));
  });

  it("restores and deletes saved assistant conversations", async () => {
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "assistant_conversations") {
        return [
          {
            id: "conv1",
            title: "历史对话",
            instanceId: "abc123def456",
            updatedAt: "2026-10-26T12:00:00Z",
            messageCount: 2,
          },
        ];
      }
      if (cmd === "assistant_conversation") {
        return {
          id: "conv1",
          title: "历史对话",
          instanceId: "abc123def456",
          createdAt: "2026-10-26T12:00:00Z",
          updatedAt: "2026-10-26T12:00:00Z",
          messages: [
            {
              role: "user",
              content: "之前的问题",
              traces: [],
              proposals: [],
              createdAt: "2026-10-26T12:00:00Z",
            },
            {
              role: "assistant",
              content: "**历史** 回答",
              traces: [],
              proposals: [],
              createdAt: "2026-10-26T12:00:01Z",
            },
          ],
        };
      }
      if (cmd === "assistant_delete_conversation") return null;
      return base?.(cmd, args);
    });
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "鸦鸭" }));
    expect(await screen.findByText("历史对话")).toBeInTheDocument();
    expect(await screen.findByText("历史")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "删除对话 历史对话" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("assistant_delete_conversation", {
        id: "conv1",
      }),
    );
  });

  it("shows a configuration notice when the assistant has no key", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "鸦鸭" }));
    expect(await screen.findByText("配置 DeepSeek API key 后即可对话")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /分析当前实例最近一次崩溃/ })).toBeDisabled();
  });

  it("runs the assistant chat and confirms a repair proposal", async () => {
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "assistant_status") {
        return {
          configured: true,
          model: "deepseek-chat",
          baseUrl: "https://api.deepseek.com",
          auditPath: "/tmp/sr-test/assistant-audit.jsonl",
        };
      }
      return base?.(cmd, args);
    });
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "鸦鸭" }));
    const input = await screen.findByPlaceholderText(/描述问题/);
    fireEvent.change(input, { target: { value: "帮我看看这次崩溃" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText(/问题来自 examplemod/)).toBeInTheDocument();
    expect(screen.getByText("结论").tagName).toBe("STRONG");
    expect(screen.getByText("建议先提高内存").tagName).toBe("LI");
    expect(await screen.findByText(/提高到 6144 MB/)).toBeInTheDocument();
    const confirm = screen.getByRole("checkbox");
    const execute = screen.getByRole("button", { name: "执行修复" });
    expect(execute).toBeDisabled();
    fireEvent.click(confirm);
    expect(execute).toBeEnabled();
    fireEvent.click(execute);
    expect(await screen.findByText(/已备份到/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "回滚本次操作" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("assistant_rollback_repair", {
        instanceId: "abc123def456",
        token: "repair-abc",
      }),
    );
  });

  it("installs a proposed mod only after confirmation", async () => {
    const base = invokeMock.getMockImplementation();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "assistant_status") {
        return {
          configured: true,
          model: "deepseek-chat",
          baseUrl: "https://api.deepseek.com",
          auditPath: "/tmp/sr-test/assistant-audit.jsonl",
        };
      }
      return base?.(cmd, args);
    });
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "鸦鸭" }));
    const input = await screen.findByPlaceholderText(/描述问题/);
    fireEvent.change(input, { target: { value: "搜索性能模组" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText(/找到 Sodium/)).toBeInTheDocument();
    expect(await screen.findByText("Sodium")).toBeInTheDocument();
    const confirm = screen.getByRole("checkbox");
    const install = screen.getByRole("button", { name: "安装模组" });
    expect(install).toBeDisabled();
    fireEvent.click(confirm);
    fireEvent.click(install);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("assistant_apply_mod_plan", {
        planId: "modplan1",
      }),
    );
    expect(await screen.findByText(/已安装：sodium.jar/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "回滚本次操作" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("assistant_rollback_mod_install", {
        instanceId: "abc123def456",
        token: "mod-abc",
      }),
    );
  });

  it("shows the crash report section on diagnostics", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "诊断中心" }));
    expect(await screen.findByText("崩溃报告")).toBeInTheDocument();
    expect(await screen.findByText("没有崩溃报告")).toBeInTheDocument();
  });

  it("keeps focus in the account name input while typing", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "账号管理" }));
    fireEvent.click(await screen.findByRole("button", { name: "添加离线档案" }));
    const input = screen.getByLabelText("用户名");
    fireEvent.change(input, { target: { value: "S" } });
    await waitFor(() => expect(input).toHaveFocus());
    fireEvent.change(input, { target: { value: "St" } });
    await waitFor(() => expect(input).toHaveFocus());
    expect(input).toHaveValue("St");
  });

  it("selects a built-in skin and creates the account with its model", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "账号管理" }));
    fireEvent.click(await screen.findByRole("button", { name: "添加离线档案" }));
    expect(await screen.findByText("内置默认")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "纤细" }));
    const alex = await screen.findByRole("button", { name: /Alex/ });
    fireEvent.click(alex);
    fireEvent.change(screen.getByLabelText("用户名"), { target: { value: "SkinUser" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("add_offline_account", {
        username: "SkinUser",
        skinId: "builtin-slim-alex",
        skinModel: "slim",
      });
    });
  });

  it("configures Microsoft client id and enables premium login", async () => {
    render(<App />);
    await screen.findByText("生存世界", {}, { timeout: 3000 });
    fireEvent.click(screen.getByRole("button", { name: "账号管理" }));
    expect(await screen.findByText("离线档案")).toBeInTheDocument();
    expect(screen.getByText(/不是已验证的正版账号/)).toBeInTheDocument();
    expect(await screen.findByText("Azure 应用 client id")).toBeInTheDocument();
    const input = screen.getByPlaceholderText("00000000-0000-0000-0000-000000000000");
    fireEvent.change(input, {
      target: { value: "11111111-1111-4111-8111-111111111111" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存并启用" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({
          settings: expect.objectContaining({
            microsoftClientId: "11111111-1111-4111-8111-111111111111",
          }),
        }),
      ),
    );
    expect(
      await screen.findByRole("button", { name: "使用 Microsoft 登录" }),
    ).toBeEnabled();
  });
});
