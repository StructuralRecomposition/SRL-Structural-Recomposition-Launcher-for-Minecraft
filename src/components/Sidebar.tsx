import { Icon } from "./Icon";
import { NAV } from "./nav";
import { useApp } from "../state/AppContext";
import logo from "../assets/logo.png";

export function Sidebar() {
  const { page, setPage, tasks, running, bootstrap, theme, setTheme } = useApp();
  const activeTasks = tasks.filter((t) =>
    ["queued", "running", "verifying", "retrying"].includes(t.status),
  ).length;
  const runningCount = running.size;

  return (
    <aside className="sidebar">
      <div className="brand">
        <img className="brand__logo" src={logo} alt="SRL" />
        <div className="brand__text">
          <strong>SRL</strong>
          <span>Structural Recomposition Launcher</span>
        </div>
      </div>

      <nav className="nav" aria-label="主导航">
        {NAV.map((item) => {
          const badge =
            item.id === "tasks" && activeTasks > 0
              ? activeTasks
              : item.id === "instances" && runningCount > 0
                ? runningCount
                : null;
          return (
            <button
              key={item.id}
              type="button"
              className={`nav__item ${page === item.id ? "nav__item--active" : ""}`}
              onClick={() => setPage(item.id)}
              aria-current={page === item.id ? "page" : undefined}
            >
              <Icon name={item.icon} size={18} />
              <span>{item.label}</span>
              {badge !== null ? <em className="nav__badge">{badge}</em> : null}
            </button>
          );
        })}
      </nav>

      <div className="sidebar__footer">
        <button
          type="button"
          className="theme-toggle"
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          aria-label="切换明暗主题"
        >
          <Icon name={theme === "dark" ? "moon" : "sun"} size={16} />
          <span>{theme === "dark" ? "深色主题" : "浅色主题"}</span>
        </button>
        <div className="sidebar__meta">
          <span>v{bootstrap?.appVersion ?? "0.1.0"}</span>
          <span>本地优先</span>
        </div>
      </div>
    </aside>
  );
}
