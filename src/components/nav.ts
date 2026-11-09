import type { IconName } from "../components/Icon";
import type { PageId } from "../state/AppContext";
import { t } from "../lib/i18n";


export interface NavItem {
  id: PageId;
  label: string;
  icon: IconName;
  subtitle: string;
  tag?: string;
}

export const NAV: NavItem[] = [
  { id: "home", label: t("首页"), icon: "home", subtitle: t("当前实例与快速启动") },
  { id: "instances", label: t("游戏实例"), icon: "box", subtitle: t("隔离的游戏目录与配置") },
  { id: "versions", label: t("版本管理"), icon: "layers", subtitle: t("官方版本清单与安装") },
  { id: "resources", label: t("资源"), icon: "search", subtitle: t("Modrinth 模组、整合包、光影与材质") },
  { id: "multiplayer", label: t("Minecraft 联机"), icon: "globe", subtitle: t("创建房间、邀请好友、管理隧道") },
  { id: "java", label: t("Java 环境"), icon: "coffee", subtitle: t("发现、检测与兼容性") },
  { id: "tasks", label: t("下载任务"), icon: "download", subtitle: t("真实进度、速度与取消") },
  { id: "assistant", label: t("鸦鸭"), icon: "cpu", subtitle: t("崩溃诊断与模组助手"), tag: t("测试") },
  { id: "accounts", label: t("账号管理"), icon: "user", subtitle: t("离线档案与授权状态") },
  { id: "diagnostics", label: t("诊断中心"), icon: "activity", subtitle: t("环境检查与日志") },
  { id: "settings", label: t("设置"), icon: "settings", subtitle: t("目录、网络与外观") },
];

export function navItem(id: PageId): NavItem {
  return NAV.find((n) => n.id === id) ?? NAV[0];
}
