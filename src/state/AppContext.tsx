import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { getLanguage, normalizeLanguage, setLanguage } from "../lib/i18n";
import {
  api,
  ApiError,
  isTauri,
  onEvent,
  type Bootstrap,
  type LaunchEvent,
  type TaskView,
} from "../lib/api";
import type { ToastItem, ToastTone } from "../components/Toast";
import { t as tr } from "../lib/i18n";


export type PageId =
  | "home"
  | "instances"
  | "versions"
  | "resources"
  | "multiplayer"
  | "java"
  | "tasks"
  | "assistant"
  | "accounts"
  | "diagnostics"
  | "settings";

interface AppContextValue {
  bootstrap: Bootstrap | null;
  loading: boolean;
  error: ApiError | null;
  reload: () => Promise<void>;
  tasks: TaskView[];
  running: Set<string>;
  markRunning: (id: string, running: boolean) => void;
  launchEvents: LaunchEvent[];
  clearLaunchEvents: () => void;
  page: PageId;
  setPage: (p: PageId) => void;
  theme: "dark" | "light";
  setTheme: (t: "dark" | "light") => void;
  toasts: ToastItem[];
  pushToast: (message: string, tone?: ToastTone, detail?: string) => void;
  dismissToast: (id: string) => void;
  updateBootstrap: (patch: (b: Bootstrap) => Bootstrap) => void;
  refreshTasks: () => Promise<void>;
  setReloadPaused: (paused: boolean) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

/// UI log history is bounded; the backend log file remains the source of
/// truth. Flushing at 5 Hz keeps a chatty mod from triggering a React update
/// for every single stdout line.
const MAX_LAUNCH_EVENTS = 400;
const LAUNCH_FLUSH_MS = 200;

export function useApp(): AppContextValue {
  const value = useContext(AppContext);
  if (!value) throw new Error("useApp must be used inside AppProvider");
  return value;
}

function resolveTheme(theme: string): "dark" | "light" {
  if (theme === "light") return "light";
  if (theme === "system") {
    return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  return "dark";
}

export function AppProvider({ children }: { children: ReactNode }) {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<ApiError | null>(null);
  const [tasks, setTasks] = useState<TaskView[]>([]);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [launchEvents, setLaunchEvents] = useState<LaunchEvent[]>([]);
  const [page, setPage] = useState<PageId>("home");
  const [theme, setThemeState] = useState<"dark" | "light">("light");
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const bootstrapTimer = useRef<number | null>(null);
  const reloadPaused = useRef(false);
  const initialized = useRef(false);
  const launchBuffer = useRef<LaunchEvent[]>([]);
  const launchFlushTimer = useRef<number | null>(null);

  const pushToast = useCallback((message: string, tone: ToastTone = "info", detail?: string) => {
    const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    setToasts((prev) => [...prev.slice(-3), { id, message, detail, tone }]);
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 6000);
  }, []);

  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const updateBootstrap = useCallback((patch: (b: Bootstrap) => Bootstrap) => {
    setBootstrap((prev) => (prev ? patch(prev) : prev));
  }, []);

  const flushLaunchEvents = useCallback(() => {
    if (launchFlushTimer.current !== null) {
      window.clearTimeout(launchFlushTimer.current);
      launchFlushTimer.current = null;
    }
    const pending = launchBuffer.current;
    if (pending.length === 0) return;
    launchBuffer.current = [];
    setLaunchEvents((prev) => [...prev, ...pending].slice(-MAX_LAUNCH_EVENTS));
  }, []);

  const queueLaunchEvent = useCallback(
    (event: LaunchEvent) => {
      const buffer = launchBuffer.current;
      buffer.push(event);
      // A misbehaving mod must not grow this buffer without bound before the
      // next flush tick.
      if (buffer.length > MAX_LAUNCH_EVENTS * 4) {
        buffer.splice(0, buffer.length - MAX_LAUNCH_EVENTS);
      }
      if (launchFlushTimer.current === null) {
        launchFlushTimer.current = window.setTimeout(flushLaunchEvents, LAUNCH_FLUSH_MS);
      }
    },
    [flushLaunchEvents],
  );

  const reload = useCallback(async () => {
    if (!isTauri()) {
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const data = await api.bootstrap();
      setBootstrap(data);
      setThemeState(resolveTheme(data.settings.theme));
      const language = normalizeLanguage(data.settings.language);
      if (language !== getLanguage() && sessionStorage.getItem("srl.lang-reloaded") !== "1") {
        sessionStorage.setItem("srl.lang-reloaded", "1");
        setLanguage(language);
        window.location.reload();
        return;
      }
      setRunning(new Set(data.runningInstances));
      setError(null);
      const initialTasks = await api.listTasks();
      setTasks(initialTasks);
      // Java discovery runs in the background; refresh once if it was empty.
      if (data.java.length === 0) {
        window.setTimeout(async () => {
          try {
            const java = await api.listJava(false);
            if (java.length > 0) {
              updateBootstrap((b) => ({ ...b, java }));
            }
          } catch {
            /* discovery may still be running */
          }
        }, 1600);
      }
    } catch (e) {
      setError(e instanceof ApiError ? e : new ApiError("INTERNAL", String(e)));
    } finally {
      setLoading(false);
    }
  }, [updateBootstrap]);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;
    void reload();
  }, [reload]);

  const refreshTasks = useCallback(async () => {
    try {
      setTasks(await api.listTasks());
    } catch {
      /* non-fatal */
    }
  }, []);

  // Task progress stream.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onEvent<TaskView>("task://update", (task) => {
      setTasks((prev) => {
        const idx = prev.findIndex((t) => t.id === task.id);
        const next = idx >= 0 ? prev.map((t) => (t.id === task.id ? task : t)) : [task, ...prev];
        return next.sort((a, b) => b.startedAt.localeCompare(a.startedAt)).slice(0, 50);
      });
      if (task.status === "completed" && task.kind === "install" && !reloadPaused.current) {
        if (bootstrapTimer.current) window.clearTimeout(bootstrapTimer.current);
        bootstrapTimer.current = window.setTimeout(() => {
          void reload();
        }, 900);
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
      if (bootstrapTimer.current) window.clearTimeout(bootstrapTimer.current);
    };
  }, [reload]);

  // Launch log stream.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onEvent<LaunchEvent>("launch://event", (event) => {
      if (event.kind === "log") {
        queueLaunchEvent(event);
        return;
      }
      if (event.kind === "exit") {
        flushLaunchEvents();
        setRunning((prev) => {
          const next = new Set(prev);
          next.delete(event.instanceId);
          return next;
        });
        pushToast(
          event.code === 0 ? tr("游戏已退出") : tr("游戏进程已结束"),
          event.code === 0 ? "info" : "warning",
          event.message ?? undefined,
        );
        void reload();
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
      if (launchFlushTimer.current !== null) {
        window.clearTimeout(launchFlushTimer.current);
        launchFlushTimer.current = null;
      }
      launchBuffer.current = [];
    };
  }, [pushToast, reload, queueLaunchEvent, flushLaunchEvents]);


  const markRunning = useCallback((id: string, isRunning: boolean) => {
    setRunning((prev) => {
      const next = new Set(prev);
      if (isRunning) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  const clearLaunchEvents = useCallback(() => {
    launchBuffer.current = [];
    if (launchFlushTimer.current !== null) {
      window.clearTimeout(launchFlushTimer.current);
      launchFlushTimer.current = null;
    }
    setLaunchEvents([]);
  }, []);
  const setReloadPaused = useCallback((paused: boolean) => {
    reloadPaused.current = paused;
  }, []);

  const setTheme = useCallback(
    async (next: "dark" | "light") => {
      setThemeState(next);
      if (!bootstrap) return;
      const settings = { ...bootstrap.settings, theme: next };
      try {
        const saved = await api.updateSettings(settings);
        updateBootstrap((b) => ({ ...b, settings: saved }));
      } catch (e) {
        pushToast(tr("主题已应用，但未能保存"), "warning", e instanceof Error ? e.message : undefined);
      }
    },
    [bootstrap, pushToast, updateBootstrap],
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    if (bootstrap) {
      setThemeState(resolveTheme(bootstrap.settings.theme));
    }
  }, [bootstrap?.settings.theme]);

  const value = useMemo<AppContextValue>(
    () => ({
      bootstrap,
      loading,
      error,
      reload,
      tasks,
      running,
      markRunning,
      launchEvents,
      clearLaunchEvents,
      page,
      setPage,
      theme,
      setTheme,
      toasts,
      pushToast,
      dismissToast,
      updateBootstrap,
      refreshTasks,
      setReloadPaused,
    }),
    [
      bootstrap,
      loading,
      error,
      reload,
      tasks,
      running,
      markRunning,
      launchEvents,
      clearLaunchEvents,
      page,
      theme,
      setTheme,
      toasts,
      pushToast,
      dismissToast,
      updateBootstrap,
      refreshTasks,
      setReloadPaused,
    ],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
