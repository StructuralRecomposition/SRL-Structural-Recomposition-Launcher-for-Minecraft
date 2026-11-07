import { Button } from "./ui";
import { Modal } from "./Modal";

const FRP52 = "https://www.52frp.com/user/#/user-center";
const TOKEN_PAGE = "https://www.52frp.com/user/#/user-center";

export function MultiplayerHelp({
  open,
  onClose,
  onOpenUrl,
}: {
  open: boolean;
  onClose: () => void;
  onOpenUrl: (url: string, label: string) => void;
}) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      wide
      title="Minecraft 联机教程"
      subtitle="照着做就行。遇到问题时，先看最后一节。"
      footer={
        <Button variant="primary" onClick={onClose}>
          知道了
        </Button>
      }
    >
      <div className="mp-help">
        <p className="mp-help__lead">
          一句话：你开一个 Minecraft 世界，SRL 用 52FRP 把它搬到公网，朋友输入一个地址就能进来。
          朋友不需要开穿透，直连即可。
        </p>

        <section className="mp-help__section">
          <h3>1. 先准备 52FRP</h3>
          <ol className="mp-help__steps">
            <li>
              <strong>注册并实名认证。</strong>
              打开 52FRP，注册账号，按页面提示完成实名认证。没有实名通常不能购买和创建隧道。
            </li>
            <li>
              <strong>买一个套餐。</strong>
              进入套餐商城，选一个离你近的节点。常见价格大约是 4.5 元/月，实际价格和库存以商城页面为准。
            </li>
            <li>
              <strong>创建 API 令牌。</strong>
              登录后点右上角头像 → API 访问 → 创建令牌。权限里务必勾上
              <b>“创建和修改隧道与客户端”</b>，然后创建并复制以 <code>hz_</code> 开头的令牌。
            </li>
            <li>
              <strong>填进 SRL。</strong>
              回到 SRL 的 Minecraft 联机页，把令牌粘贴到输入框，点“连接 52FRP”。
            </li>
          </ol>
          <div className="mp-help__actions">
            <Button size="sm" onClick={() => onOpenUrl(FRP52, "52FRP 个人中心")}>
              打开 52FRP
            </Button>
            <Button size="sm" variant="ghost" onClick={() => onOpenUrl(TOKEN_PAGE, "52FRP 令牌页面")}>
              打开令牌页面
            </Button>
          </div>
          <p className="mp-help__tip">
            令牌相当于密码。SRL 只把它保存在系统凭据库，不会写进日志或配置文件，也不要发给别人。
          </p>
        </section>

        <section className="mp-help__section">
          <h3>2. 房主：开世界，再创建房间</h3>
          <ol className="mp-help__steps">
            <li>
              <strong>先把 Minecraft 开起来。</strong>
              进入你的世界，按 <code>Esc</code> → <b>对局域网开放</b>。
              普通 Java 版会随机给一个端口；装了离线联机模组的话，可以固定成 <code>25565</code> 或 <code>59888</code>。
            </li>
            <li>
              <strong>在 SRL 里填本机端口。</strong>
              选游戏实例和 Java 版，本地 IP 保持 <code>127.0.0.1</code>，本地端口填 Minecraft 实际显示的端口。
              不确定就点“自动检测”。
            </li>
            <li>
              <strong>创建房间。</strong>
              节点选“自动选择”即可。等状态变成“联机在线”，复制上面的公网地址。
            </li>
            <li>
              <strong>保持三个东西都开着。</strong>
              Minecraft 世界、SRL、FRPC。关掉任何一个，朋友都会断线。
            </li>
          </ol>
        </section>

        <section className="mp-help__section">
          <h3>3. 朋友：只做这一步</h3>
          <p className="mp-help__plain">
            朋友打开同版本的 Minecraft，进入 <b>多人游戏</b> → <b>直接连接</b>，把房主复制的地址粘进去。
            地址长这样：<code>203.0.113.10:44688</code>（这是示例，请填你自己的地址）。
          </p>
          <ul className="mp-help__list">
            <li>朋友不需要开穿透，也不需要装 FRPC，直连即可。</li>
            <li>Java 版用 TCP；基岩版在“服务器 → 添加服务器”里填同一个地址和端口，走 UDP。</li>
            <li>版本、Forge 和模组要跟房主一致，否则会进不去或被踢。</li>
            <li>地址里冒号后面的端口是公网端口，不是房主的本地端口。</li>
          </ul>
        </section>

        <section className="mp-help__section">
          <h3>4. 没有正版账号，怎么离线联机</h3>
          <p className="mp-help__plain">
            两个人用离线账号也能联机，但要让 Minecraft 关闭正版验证。双方都要安装
            <b>“自定义局域网联机”</b>模组（版本和游戏/Forge 版本要一致）：
          </p>
          <ol className="mp-help__steps">
            <li>
              复制下面的网址到浏览器打开，下载和游戏版本、Forge 版本匹配的文件：
              <code className="mp-help__url">https://www.mcmod.cn/class/2754.html</code>
            </li>
            <li>房主和朋友都要装，版本必须一致。</li>
            <li>进入世界，打开“对局域网开放”，把端口固定成 <code>25565</code> 或 <code>59888</code>，关闭“在线模式”。</li>
            <li>回到 SRL，用这个固定端口创建房间，把公网地址发给朋友。</li>
          </ol>
          <p className="mp-help__tip">
            也可以开一个 Forge 专用服务端，在 <code>server.properties</code> 里设置
            <code>online-mode=false</code> 和 <code>server-port=59888</code>。关闭正版验证后谁都能进，最好再加白名单。
          </p>
        </section>

        <section className="mp-help__section">
          <h3>5. 常见问题</h3>
          <dl className="mp-help__faq">
            <div>
              <dt>朋友显示“连接超时”</dt>
              <dd>先看 SRL 是不是“联机在线”，再确认朋友填的是公网地址和公网端口。穿透没通时通常是超时或拒绝连接。</dd>
            </div>
            <div>
              <dt>朋友显示“无效会话”</dt>
              <dd>这不是穿透问题，是 Minecraft 正版验证没过。正版账号重新登录；离线账号则按上一节关闭在线模式。</dd>
            </div>
            <div>
              <dt>本地端口和公网端口有什么区别？</dt>
              <dd>本地端口是 Minecraft 在你电脑上开的门，公网端口是 52FRP 给朋友开的门。朋友只填公网端口。</dd>
            </div>
            <div>
              <dt>为什么每次都要开 SRL？</dt>
              <dd>SRL 负责启动 FRPC 并维持隧道。SRL 关了，公网地址就失效了。</dd>
            </div>
          </dl>
        </section>
      </div>
    </Modal>
  );
}
