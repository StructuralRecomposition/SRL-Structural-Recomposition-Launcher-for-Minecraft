import { Icon } from "./Icon";
import { useApp } from "../state/AppContext";
import { navItem } from "./nav";
import { IconButton } from "./ui";
import { t as tr } from "../lib/i18n";


export function TopBar() {
  const { page, setPage, tasks, running, theme, setTheme } = useApp();
  const item = navItem(page);
  const activeTasks = tasks.filter((t) =>
    ["queued", "running", "verifying", "retrying"].includes(t.status),
  ).length;

  return (
    <header className="topbar">
      <div className="topbar__crumbs" title={item.subtitle}>
        <span className="topbar__app">SRL</span>
        <Icon name="chevron" size={11} className="topbar__chevron" />
        <span className="topbar__page">{item.label}</span>
      </div>
      <div className="topbar__actions">
        {running.size > 0 ? (
          <button type="button" className="chip chip--live" onClick={() => setPage("instances")}>
            <span className="pulse-dot" aria-hidden="true" />
            {running.size} {tr("个实例运行中")}
          </button>
        ) : null}
        {activeTasks > 0 ? (
          <button type="button" className="chip" onClick={() => setPage("tasks")}>
            <Icon name="download" size={13} />
            {activeTasks} {tr("个任务")}
          </button>
        ) : null}
        <div className="topbar__divider" />
        <IconButton
          icon={theme === "dark" ? "sun" : "moon"}
          label={theme === "dark" ? tr("切换到浅色主题") : tr("切换到深色主题")}
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
        />
      </div>
    </header>
  );
}
