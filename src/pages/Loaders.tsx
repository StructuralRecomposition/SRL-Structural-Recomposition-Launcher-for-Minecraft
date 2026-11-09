import { useApp } from "../state/AppContext";
import { api } from "../lib/api";
import { Badge, Button } from "../components/ui";
import { Icon } from "../components/Icon";
import { LoaderInstallerCard } from "../components/LoaderInstallerCard";
import { LoaderRecommendationCard } from "../components/LoaderRecommendationCard";
import { t } from "../lib/i18n";




export function LoadersPage() {
  const { setPage } = useApp();

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>{t("模组加载器")}</h2>
          <p className="muted">{t("Fabric / Quilt 使用官方元数据；Forge / NeoForge 在确认后运行官方安装器。")}</p>
        </div>
        <Badge tone="success">{t("Fabric / Quilt / Forge / NeoForge 已接入")}</Badge>
      </div>

      <LoaderRecommendationCard />

      <LoaderInstallerCard
        kind="fabric"
        title="Fabric"
        description={t("轻量、快速更新的模组加载器")}
        metaHost="meta.fabricmc.net · bmclapi2.bangbang93.com"
        note={t("已通过真实 1.21.1 + Fabric 0.19.5 安装验证（shared jar、71 个库、完整性检查）。默认优先 BMCLAPI，失败自动回退官方源。")}
        stableKnown
        installedPrefix="fabric-loader-"
        listLoaders={api.listFabricLoaders}
        install={api.installFabric}
      />

      <LoaderInstallerCard
        kind="quilt"
        title="Quilt"
        description={t("Fabric 生态的衍生加载器，使用独立的官方 meta")}
        metaHost="meta.quiltmc.org"
        stableKnown={false}
        verified
        note={t("已通过真实 1.21.1 + Quilt 0.20.0-beta.9 安装验证（77 库、shared jar、完整性检查）。Quilt 官方源时好时坏，安装可能较慢；构件已配置 BMCLAPI/USTC 镜像回退。")}
        installedPrefix="quilt-loader-"
        listLoaders={api.listQuiltLoaders}
        install={api.installQuilt}
      />

      <div className="notice">
        <Icon name="shield" size={17} />
        <div>
          <strong>{t("加载器安装边界")}</strong>
          <p>
            {t("Fabric / Quilt 使用官方 meta；Forge / NeoForge 使用官方安装器（点击安装时会明确确认）。 所有加载器安装后都会生成可启动的版本，并自动压平继承关系。")}</p>
        </div>
        <Button size="sm" onClick={() => setPage("versions")}>
          {t("管理版本")}</Button>
      </div>
    </div>
  );
}
