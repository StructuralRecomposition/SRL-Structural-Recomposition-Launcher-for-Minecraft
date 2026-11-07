import { Icon } from "./Icon";
import { useApp } from "../state/AppContext";
import { navItem } from "./nav";
import { IconButton } from "./ui";

export function TopBar() {
  const { page, setPage, tasks, running, bootstrap, theme, setTheme } = useApp();
  const item = navItem(page);
  const activeTasks = tasks.filter((t) =>
    ["queued", "running", "verifying", "retrying"].includes(t.status),
  ).length;

  return (
    <header className="topbar">
      <div className="topbar__title" key={page}>
        <h1>{item.label}</h1>
        <p>{item.subtitle}</p>
      </div>
      <div className="topbar__actions">
        {running.size > 0 ? (
          <button className="chip chip--live" onClick={() => setPage("instances")}>
            <span className="pulse-dot" aria-hidden="true" />
            {running.size} 个实例运行中
          </button>
        ) : null}
        {activeTasks > 0 ? (
          <button className="chip" onClick={() => setPage("tasks")}>
            <Icon name="download" size={14} />
            {activeTasks} 个任务
          </button>
        ) : null}
        <div className="topbar__divider" />
        <IconButton
          icon={theme === "dark" ? "sun" : "moon"}
          label={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
        />
        <div className="topbar__env" title={`${bootstrap?.platform ?? ""}`}>
          {bootstrap?.platform ?? "desktop"}
        </div>
      </div>
    </header>
  );
}
