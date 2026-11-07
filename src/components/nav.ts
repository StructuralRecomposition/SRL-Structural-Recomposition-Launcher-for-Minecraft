import type { IconName } from "../components/Icon";
import type { PageId } from "../state/AppContext";

export interface NavItem {
  id: PageId;
  label: string;
  icon: IconName;
  subtitle: string;
}

export const NAV: NavItem[] = [
  { id: "home", label: "首页", icon: "home", subtitle: "当前实例与快速启动" },
  { id: "instances", label: "游戏实例", icon: "box", subtitle: "隔离的游戏目录与配置" },
  { id: "versions", label: "版本管理", icon: "layers", subtitle: "官方版本清单与安装" },
  { id: "resources", label: "资源", icon: "search", subtitle: "Modrinth 模组、整合包、光影与材质" },
  { id: "multiplayer", label: "Minecraft 联机", icon: "globe", subtitle: "创建房间、邀请好友、管理隧道" },
  { id: "java", label: "Java 环境", icon: "coffee", subtitle: "发现、检测与兼容性" },
  { id: "tasks", label: "下载任务", icon: "download", subtitle: "真实进度、速度与取消" },
  { id: "assistant", label: "鸦鸭", icon: "cpu", subtitle: "崩溃诊断与模组助手" },
  { id: "accounts", label: "账号管理", icon: "user", subtitle: "离线档案与授权状态" },
  { id: "diagnostics", label: "诊断中心", icon: "activity", subtitle: "环境检查与日志" },
  { id: "settings", label: "设置", icon: "settings", subtitle: "目录、网络与外观" },
];

export function navItem(id: PageId): NavItem {
  return NAV.find((n) => n.id === id) ?? NAV[0];
}
