import { Icon } from "./Icon";
import { NAV } from "./nav";
import { useApp } from "../state/AppContext";
import logo from "../assets/logo.png";

const GROUPS: { label: string; ids: string[] }[] = [
  { label: "游戏", ids: ["home", "instances", "versions", "resources"] },
  { label: "工具", ids: ["java", "tasks", "assistant"] },
  { label: "系统", ids: ["accounts", "diagnostics", "settings"] },
];

export function Sidebar() {
  const { page, setPage, tasks, running, bootstrap, theme, setTheme } = useApp();
  const activeTasks = tasks.filter((t) =>
    ["queued", "running", "verifying", "retrying"].includes(t.status),
  ).length;
  const runningCount = running.size;

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand__mark" aria-hidden="true">
          <img className="brand__logo" src={logo} alt="" />
        </span>
        <div className="brand__text">
          <strong>SRL</strong>
          <span>Structural Recomposition</span>
        </div>
      </div>

      <nav className="nav" aria-label="主导航">
        {GROUPS.map((group) => {
          const items = group.ids
            .map((id) => NAV.find((n) => n.id === id))
            .filter((item): item is (typeof NAV)[number] => Boolean(item));
          if (items.length === 0) return null;
          return (
            <div className="nav-group" key={group.label}>
              <span className="nav-group__label">{group.label}</span>
              {items.map((item) => {
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
                    <Icon name={item.icon} size={17} />
                    <span>{item.label}</span>
                    {badge !== null ? <em className="nav__badge">{badge}</em> : null}
                  </button>
                );
              })}
            </div>
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
          <span>v{bootstrap?.appVersion ?? "0.1.1"}</span>
          <span>本地优先</span>
        </div>
      </div>
    </aside>
  );
}
