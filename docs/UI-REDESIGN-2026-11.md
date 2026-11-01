# EasyLAN 客户端 UI 重构与产品化改造（2026-11）

本文记录本轮对桌面客户端的完整改造：ToDesk 风格的窗口导航与主页、设置页裁剪与紧凑化、
白标机制、macOS 授权向导，以及本地 Xcode 27 构建适配。基线：`master @ a519b49b0`
（上游 1.5.1 合并后）。

---

## 1. 窗口导航重构

### 1.1 窗口级侧栏（新增 `sidebar_widget.dart`）

- 178px 固定宽度，浅色 `#F0F3F8` / 深色 `#1F2027`，选中项蓝字 + 圆角浅底。
- 结构：28px 拖拽区（接管 macOS 红绿灯拖拽）→ 头像/昵称（未登录显示「登录」，
  点击拉起登录弹窗；已登录跳转设置-账户）→ 5 个 peer 导航项（**动态生成**自
  `PeerTabModel.visibleEnabledOrderedIndexs`，某项被禁用即自动消失）→ 分隔线 →
  设置/关于 → 底部 Logo + 版本号。
- **侧栏是主窗口唯一导航**：设置/关于/账户入口全部走
  `DesktopSettingPage.switch2page()`（打开或跳转 + 子页切换），修复了原先
  `onAddSetting` 只跳 tab 不切子页的停留 bug。

### 1.2 标签条移除（`tabbar_widget.dart`）

主窗口（`leading != null`）不再渲染顶部 28px 标签条——单标签时它是死空间，
双标签时与侧栏重复。tab 机制（状态/切换/多窗口注册）完整保留，仅视觉移除。
其他窗口（远程会话、文件传输等）无 `leading`，行为不变。

### 1.3 主页重设计（`desktop_home_page.dart`）

- **顶部工具栏**（一行 64px，兼任窗口拖拽把手）：`ID chip（点击复制）│
  一次性密码 chip（刷新/修改）│ [连接输入框][连接][⋮]`。
  连接表单从 `ConnectionPage` 抽取为独立组件 `RemoteIdInput`（保留
  RawAutocomplete 联想 / 更多连接方式菜单 / 回车提交）。
- **权限横幅**：浅蓝 info 条（图标 + 文案 + 配置 + 帮助），替代原粉色渐变大卡；
  帮助卡片整体走紧凑样式。
- **过滤 chips**：`[全部] [用户…]`，替代「可访问的设备」原左侧 150px 窄栏；
  地址簿的 200px 标签面板同样改为「地址簿下拉 + 标签 chips」单行工具条。
- **peer 卡片扁平化**：柔和色平台图标块 + ID/主机名 + 状态点，网格固定行高
  64（`childMainAxisExtent`），行距 6pt——修复 wrapping 网格松弛约束导致的
  巨大行间距。
- 设备 ID / 一次性密码大卡与 200px 左栏移除（incoming-only 精简窗口
  `buildLeftPane` 路径原样保留）。

### 1.4 首启授权向导（新增 `permission_wizard.dart`）

macOS 首次启动集中列出 屏幕录制 / 辅助功能 / 输入监控 三项权限的实时状态
（绿✓/橙⚠），每项「配置」直达系统设置；**授权后对应行自动变绿、全部授权后
弹窗自动关闭**（1s 轮询）。「跳过」以本地选项 `permission-wizard-dismissed`
记住选择。入口：`DesktopTabPage.initState` 延迟 3s，`maybeShowPermissionWizard()`。

---

## 2. 设置页紧凑化与裁剪

- `_Card` 标题 20px → 14px semibold，间距收敛——单屏可见卡片数翻倍。
- 主题：三个竖排 radio → **分段控件**（`SegmentedButton`）。
- 语言 / 音频输入设备下拉限宽 300、左对齐。
- 网络设置整组隐藏：预设 `builtin_settings` 的
  `hide-network-settings / hide-server-settings / hide-proxy-settings /
  hide-websocket-settings` 已由设置页识别。
- 错误文案：登录失败提示改用 `Login failed` 翻译键（不再输出英文技术细节）。

---

## 3. 预设裁剪（full-client.json）

`.github/presets/full-client.json` 是完整客户端的产品预设，CI 与本地共用：

```json
{
  "hard_settings": {},
  "override_settings": {
    "custom-rendezvous-server": "rd.wh.easylan.site",
    "relay-server": "rd.wh.easylan.site",
    "key": "<服务器公钥>",
    "enable-check-update": "N",
    "allow-auto-update": "N"
  },
  "builtin_settings": {
    "hide-network-settings": "Y",
    "hide-server-settings": "Y",
    "hide-proxy-settings": "Y",
    "hide-websocket-settings": "Y",
    "hide-remote-printer-settings": "Y",
    "hide-powered-by-me": "Y",
    "disable-change-id": "Y",
    "disable-unlock-pin": "Y"
  }
}
```

语义（详见 `rustdesk裁剪配置手册.md`）：`override_settings` = 显示但锁定；
`builtin_settings` = 整组隐藏。注入方式见 §5。

**注意**：`hide-help-cards` 不要开启——权限横幅属于帮助卡片体系，开启后
用户将看不到屏幕录制授权提示。

---

## 4. 白标（EasyLAN）

- **已完成（显示层）**：`AppInfo.xcconfig` `PRODUCT_NAME = EasyLAN` →
  应用包名 / Dock / 菜单栏 / 进程名；关于页标题、版权（© 2026 EasyLAN）、
  网站链接（https://rd.wh.easylan.site）。
- **已实现但停用（完整改名）**：`hard_settings` 支持 `"app-name"`，运行时改写
  `APP_NAME`（页脚、URI prefix、LaunchAgent 名等全部跟随），并在
  `src/common.rs::migrate_config_dir_for_preset()` 中一次性迁移配置目录
  （`com.carriez.RustDesk` → `com.carriez.EasyLAN`，仅当新目录不存在时复制）。
  **当前停用原因**：迁移时机（FFI 初始化）晚于窗口创建与首次配置读取，启用后
  会导致登录状态/窗口尺寸丢失。启用前需把迁移调用前移到 Rust `main` 最早处
  （`core_main::main` 开头，函数已就位，改动一行）并充分测试。
- 应用图标未替换——需要设计资产（.icns），非代码工作。

---

## 5. 本地构建（Xcode 27 适配）

`docs/BUILD-LOCAL-macos-arm64.md` 的补充，2026-11 起本机构建需要：

1. **Xcode 27.2**：需 `sudo xcodebuild -license accept`。
2. **部署目标 12.0**：Xcode 27 拒绝 < 12.0 的目标（Podfile platform、
   pbxproj、`MACOSX_DEPLOYMENT_TARGET` 均已调整）。
3. **`ENABLE_DEBUG_DYLIB = NO`**（Runner 三个配置）：Xcode 26+ 的 debug
   dylib 机制会把 Frameworks 阶段的库（含 Rust dylib）排除在链接之外。
4. **先编 Rust 再编 Flutter**：`flutter build` 不编译 Rust。Xcode 27 改为
   按名链接（`-weak-llibrustdesk` + LIBRARY_SEARCH_PATHS），需要
   `cargo build --lib --features "hwcodec,flutter,unix-file-copy-paste"`
   先产出 `target/debug/liblibrustdesk.dylib`（release 产物同理）。
5. **预设注入**：`local_build_env.sh`（本地、不入库）从
   `.github/presets/full-client.json` 读取并导出 `RUSTDESK_PRESET_*`
   三个环境变量；通用加载脚本 `.github/presets/env.sh` 可被任何构建环境复用：
   `. .github/presets/env.sh .github/presets/full-client.json`。
   `build.rs` 监听这些变量，变更后自动重编。
6. `local_build_env.sh` 与 `特性/`（任务材料）为本地文件，已加入 `.gitignore`。

---

## 6. 已知限制

- `cargo test --workspace` 在 macOS 上会因 `scrap` 的 example 链接失败
  （缺 `_BackingScaleFactor`，该符号由主程序 `src/platform/macos.mm` 提供）
  ——上游固有，CI 从不在 macOS 跑 `--workspace`；本地验证用
  `cargo test --lib` + `-p clipboard`。
- 会话内工具栏（`remote_toolbar.dart`）功能完整但视觉仍是上游风格，本轮仅
  统一了菜单圆角；完整对齐需要在真实会话中逐项走查，列入后续迭代。
- flutter/web 无 JS 源码（维护者私有），Web 目标本机不可构建。
- 每次重建 ad-hoc 二进制后，TCC 会视为新应用：麦克风弹窗、屏幕录制需重新
  授权（正式签名发布后消失）。
