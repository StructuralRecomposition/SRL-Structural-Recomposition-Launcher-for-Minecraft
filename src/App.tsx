import { Component, type ErrorInfo, type ReactNode } from "react";
import { AppProvider, useApp } from "./state/AppContext";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { ToastHost } from "./components/Toast";
import { isTauri } from "./lib/api";
import { Button, Card } from "./components/ui";
import { Icon } from "./components/Icon";
import logo from "./assets/logo.png";
import { HomePage } from "./pages/Home";
import { InstancesPage } from "./pages/Instances";
import { VersionsPage } from "./pages/Versions";
import { ResourcesPage } from "./pages/Resources";
import { TasksPage } from "./pages/Tasks";
import { JavaPage } from "./pages/Java";
import { AccountsPage } from "./pages/Accounts";
import { AssistantPage } from "./pages/Assistant";
import { DiagnosticsPage } from "./pages/Diagnostics";
import { SettingsPage } from "./pages/Settings";

const PAGES = {
  home: HomePage,
  instances: InstancesPage,
  versions: VersionsPage,
  resources: ResourcesPage,
  tasks: TasksPage,
  java: JavaPage,
  assistant: AssistantPage,
  accounts: AccountsPage,
  diagnostics: DiagnosticsPage,
  settings: SettingsPage,
};

function Shell() {
  const { page, loading, error, bootstrap, reload, toasts, dismissToast } = useApp();
  const Page = PAGES[page];

  if (loading) {
    return (
      <div className="boot-screen">
        <img className="boot-logo" src={logo} alt="SRL" />
        <p>正在读取本地配置…</p>
        <div className="boot-bar" aria-hidden="true" />
      </div>
    );
  }

  if (error || !bootstrap) {
    return (
      <div className="boot-screen">
        <div className="boot-error">
          <Icon name="alert" size={28} />
          <h1>启动器初始化失败</h1>
          <p>{error?.message ?? "无法读取应用状态"}</p>
          {error?.detail ? <code>{error.detail}</code> : null}
          <Button variant="primary" onClick={() => void reload()}>
            重试
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <Sidebar />
      <div className="app-main">
        <TopBar />
        <main className="content">
          <Page />
        </main>
      </div>
      <ToastHost items={toasts} onDismiss={dismissToast} />
    </div>
  );
}

function NotDesktop() {
  return (
    <div className="boot-screen">
      <Card className="not-desktop">
        <img className="boot-logo" src={logo} alt="SRL" />
        <h1>SRL 需要在桌面应用中运行</h1>
        <p>
          当前页面没有连接 Tauri 后端，因此不会显示任何模拟数据。请从项目根目录启动桌面开发环境：
        </p>
        <code>npm run dev:desktop</code>
        <p className="muted small">
          或先构建：<code>npm run build</code>，再执行 <code>npm run build:desktop</code>。
        </p>
      </Card>
    </div>
  );
}

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("UI error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="boot-screen">
          <Card className="not-desktop">
            <h1>界面渲染出错</h1>
            <p>{this.state.error.message}</p>
            <Button variant="primary" onClick={() => window.location.reload()}>
              重新加载
            </Button>
          </Card>
        </div>
      );
    }
    return this.props.children;
  }
}

export default function App() {
  if (!isTauri()) return <NotDesktop />;
  return (
    <ErrorBoundary>
      <AppProvider>
        <Shell />
      </AppProvider>
    </ErrorBoundary>
  );
}
