# RustDesk 裁剪配置手册

本文档说明如何通过独立 JSON 配置文件和 GitHub Actions workflow 的编译期环境变量裁剪 RustDesk 定制版功能。目标是尽量减少源码改动，后续升级上游源码时只保留一处通用配置注入逻辑，裁剪策略主要维护在 preset JSON 文件中。

## 配置入口

推荐把裁剪配置维护在独立 JSON 文件中，例如：

```text
.github/presets/easylan.json
```

JSON 文件结构：

```json
{
  "hard_settings": {
    "conn-type": "incoming"
  },
  "override_settings": {
    "custom-rendezvous-server": "rd.easylan.site"
  },
  "builtin_settings": {
    "hide-network-settings": "Y"
  }
}
```

workflow 只指定 preset 文件路径，并在 checkout 后读取 JSON，写入编译期环境变量：

```yaml
env:
  RUSTDESK_PRESET_FILE: ".github/presets/easylan.json"

steps:
  - uses: actions/checkout@v4

  - name: Load RustDesk preset config
    shell: pwsh
    run: |
      $preset = Get-Content "${{ env.RUSTDESK_PRESET_FILE }}" -Raw | ConvertFrom-Json
      $settings = @{
        RUSTDESK_PRESET_HARD_SETTINGS = $preset.hard_settings
        RUSTDESK_PRESET_OVERRIDE_SETTINGS = $preset.override_settings
        RUSTDESK_PRESET_BUILTIN_SETTINGS = $preset.builtin_settings
      }
      foreach ($item in $settings.GetEnumerator()) {
        $json = $item.Value | ConvertTo-Json -Compress -Depth 20
        Add-Content -Path $env:GITHUB_ENV -Value "$($item.Key)<<EOF"
        Add-Content -Path $env:GITHUB_ENV -Value $json
        Add-Content -Path $env:GITHUB_ENV -Value "EOF"
      }
```

三类配置含义：

| 配置变量 | 用途 |
| --- | --- |
| JSON 字段 | 编译期环境变量 | 用途 |
| --- | --- | --- |
| `hard_settings` | `RUSTDESK_PRESET_HARD_SETTINGS` | 强制产品形态和硬限制，通常不可被用户修改 |
| `override_settings` | `RUSTDESK_PRESET_OVERRIDE_SETTINGS` | 固定普通设置，UI 通常会识别为 fixed |
| `builtin_settings` | `RUSTDESK_PRESET_BUILTIN_SETTINGS` | 隐藏入口、内置行为、品牌化选项 |

注意事项：

- preset JSON 里的 key 和 value 都建议写成字符串。
- `enable-*` 通常用 `N` 表示禁用。
- `allow-*` 通常用 `Y` 表示允许，空值或 `N` 表示不允许。
- `hide-*` / `disable-*` 通常用 `Y` 表示生效。
- 不要把私钥、证书密码写入 workflow。
- 统一固定密码风险很高，建议按设备生成唯一密码。

## 推荐配置：被控小窗

适用于只作为被控端运行的小窗口客户端。

```yaml
RUSTDESK_PRESET_HARD_SETTINGS: >-
  {
    "conn-type": "incoming",
    "password": "可选：你的永久密码",
    "disable-settings": "Y",
    "disable-account": "Y",
    "disable-ab": "Y",
    "disable-installation": "Y"
  }

RUSTDESK_PRESET_OVERRIDE_SETTINGS: >-
  {
    "custom-rendezvous-server": "rd.easylan.site",
    "relay-server": "rd.easylan.site",
    "api-server": "",
    "key": "你的key",
    "approve-mode": "password",
    "verification-method": "use-permanent-password",
    "enable-check-update": "N",
    "allow-auto-update": "N",
    "enable-lan-discovery": "N"
  }

RUSTDESK_PRESET_BUILTIN_SETTINGS: >-
  {
    "hide-network-settings": "Y",
    "hide-server-settings": "Y",
    "hide-proxy-settings": "Y",
    "hide-websocket-settings": "Y",
    "hide-security-settings": "Y",
    "disable-change-permanent-password": "Y",
    "hide-remote-printer-settings": "Y",
    "hide-help-cards": "Y",
    "hide-powered-by-me": "Y"
  }
```

## 产品形态

放在 `RUSTDESK_PRESET_HARD_SETTINGS`。

```json
{
  "conn-type": "incoming"
}
```

| 配置项 | 值 | 说明 |
| --- | --- | --- |
| `conn-type` | `incoming` | 仅被控模式，小窗口 |
| `conn-type` | `outgoing` | 仅控制端模式 |
| `disable-settings` | `Y` | 隐藏设置入口 |
| `disable-account` | `Y` | 禁用账号/登录 |
| `disable-ab` | `Y` | 禁用地址簿 |
| `disable-installation` | `Y` | 禁用安装入口 |
| `disable-tcp-listen` | `Y` | 禁用 TCP 监听 |
| `password` | 自定义字符串 | 预置永久密码 |

## 永久密码

永久密码放在 `RUSTDESK_PRESET_HARD_SETTINGS`：

```json
{
  "password": "你的永久密码"
}
```

建议同时固定认证方式，放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`：

```json
{
  "approve-mode": "password",
  "verification-method": "use-permanent-password"
}
```

禁止用户修改永久密码，放在 `RUSTDESK_PRESET_BUILTIN_SETTINGS`：

```json
{
  "disable-change-permanent-password": "Y"
}
```

相关配置说明：

| 配置项 | 推荐位置 | 说明 |
| --- | --- | --- |
| `password` | `HARD_SETTINGS` | 预置永久密码 |
| `approve-mode` | `OVERRIDE_SETTINGS` | 会话批准方式；`password` 表示密码验证 |
| `verification-method` | `OVERRIDE_SETTINGS` | 密码类型；`use-permanent-password` 表示只使用永久密码 |
| `disable-change-permanent-password` | `BUILTIN_SETTINGS` | 禁止用户修改永久密码 |

`verification-method` 可选值：

| 值 | 说明 |
| --- | --- |
| `use-temporary-password` | 只使用一次性临时密码 |
| `use-permanent-password` | 只使用永久密码 |
| 空值或其他值 | 同时允许临时密码和永久密码 |

`approve-mode` 可选值：

| 值 | 说明 |
| --- | --- |
| `password` | 通过密码验证接受连接 |
| `click` | 通过本机点击确认接受连接 |
| 空值或其他值 | 密码和点击确认都可用，具体行为受密码是否存在影响 |

安全建议：

- 不建议所有设备使用同一个固定密码。
- 如果必须使用固定密码，建议只在受控内网环境使用，并限制服务器访问范围。
- 更安全的方案是每台设备生成唯一密码，或由 API 按设备下发密码。

## 服务器固定

放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "custom-rendezvous-server": "rd.easylan.site",
  "relay-server": "rd.easylan.site",
  "api-server": "",
  "key": "你的key"
}
```

| 配置项 | 说明 |
| --- | --- |
| `custom-rendezvous-server` | ID / hbbs 服务器 |
| `relay-server` | Relay / hbbr 服务器 |
| `api-server` | API 地址；留空时按 ID 服务器推导 |
| `key` | 服务器公钥 |

## 更新裁剪

放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "enable-check-update": "N",
  "allow-auto-update": "N"
}
```

| 配置项 | 说明 |
| --- | --- |
| `enable-check-update` | 禁用启动检查更新 |
| `allow-auto-update` | 禁用自动更新 |

## 网络裁剪

隐藏网络相关 UI，放在 `RUSTDESK_PRESET_BUILTIN_SETTINGS`。

```json
{
  "hide-network-settings": "Y",
  "hide-server-settings": "Y",
  "hide-proxy-settings": "Y",
  "hide-websocket-settings": "Y"
}
```

固定网络行为，放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "enable-lan-discovery": "N",
  "allow-websocket": "N",
  "disable-udp": "N",
  "allow-insecure-tls-fallback": "N"
}
```

| 配置项 | 说明 |
| --- | --- |
| `enable-lan-discovery` | 是否启用局域网发现 |
| `allow-websocket` | 是否允许 WebSocket |
| `disable-udp` | 是否禁用 UDP |
| `allow-insecure-tls-fallback` | 是否允许不安全 TLS fallback |

## 安全设置裁剪

隐藏安全设置，放在 `RUSTDESK_PRESET_BUILTIN_SETTINGS`。

```json
{
  "hide-security-settings": "Y",
  "disable-change-permanent-password": "Y",
  "disable-change-id": "Y",
  "disable-unlock-pin": "Y"
}
```

固定安全行为，放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "allow-numeric-one-time-password": "N",
  "allow-remote-config-modification": "N",
  "enable-trusted-devices": "N",
  "temporary-password-length": "10"
}
```

| 配置项 | 说明 |
| --- | --- |
| `hide-security-settings` | 隐藏安全设置页 |
| `disable-change-permanent-password` | 禁止修改永久密码 |
| `disable-change-id` | 禁止修改 ID |
| `disable-unlock-pin` | 禁止修改/解锁 PIN |
| `allow-numeric-one-time-password` | 是否允许纯数字一次性密码 |
| `allow-remote-config-modification` | 是否允许远程修改配置 |
| `enable-trusted-devices` | 是否启用可信设备 |
| `temporary-password-length` | 临时密码长度 |

## 能力开关

放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "enable-keyboard": "Y",
  "enable-clipboard": "Y",
  "enable-file-transfer": "N",
  "enable-camera": "N",
  "enable-terminal": "N",
  "enable-audio": "N",
  "enable-tunnel": "N",
  "enable-remote-printer": "N",
  "enable-record-session": "N",
  "enable-block-input": "N"
}
```

| 配置项 | 说明 |
| --- | --- |
| `enable-keyboard` | 允许键盘控制 |
| `enable-clipboard` | 允许剪贴板 |
| `enable-file-transfer` | 允许文件传输 |
| `enable-camera` | 允许摄像头查看 |
| `enable-terminal` | 允许终端 |
| `enable-audio` | 允许音频 |
| `enable-tunnel` | 允许端口转发/隧道 |
| `enable-remote-printer` | 允许远程打印 |
| `enable-record-session` | 允许录制 |
| `enable-block-input` | 允许阻止本地输入 |

## 显示和性能

放在 `RUSTDESK_PRESET_OVERRIDE_SETTINGS`。

```json
{
  "enable-hwcodec": "N",
  "enable-directx-capture": "Y",
  "allow-remove-wallpaper": "Y",
  "allow-always-software-render": "N"
}
```

| 配置项 | 说明 |
| --- | --- |
| `enable-hwcodec` | 是否启用硬件编解码 |
| `enable-directx-capture` | 是否启用 DirectX 捕获 |
| `allow-remove-wallpaper` | 远控时是否允许移除壁纸 |
| `allow-always-software-render` | 是否允许始终软件渲染 |

## 界面隐藏

放在 `RUSTDESK_PRESET_BUILTIN_SETTINGS`。

```json
{
  "hide-help-cards": "Y",
  "hide-powered-by-me": "Y",
  "hide-username-on-card": "Y",
  "hide-remote-printer-settings": "Y",
  "remove-preset-password-warning": "Y"
}
```

| 配置项 | 说明 |
| --- | --- |
| `hide-help-cards` | 隐藏帮助卡片 |
| `hide-powered-by-me` | 隐藏 powered by me |
| `hide-username-on-card` | 隐藏卡片用户名 |
| `hide-remote-printer-settings` | 隐藏远程打印设置 |
| `remove-preset-password-warning` | 隐藏预置密码警告 |

## 地址簿和面板

禁用地址簿建议优先使用 `disable-ab`：

```yaml
RUSTDESK_PRESET_HARD_SETTINGS: >-
  {
    "disable-ab": "Y"
  }
```

禁用局域网发现：

```yaml
RUSTDESK_PRESET_OVERRIDE_SETTINGS: >-
  {
    "enable-lan-discovery": "N"
  }
```

可选面板项：

```json
{
  "disable-group-panel": "Y",
  "disable-discovery-panel": "Y"
}
```

这两个更偏本地 UI 状态，建议优先使用 `disable-ab` 和 `enable-lan-discovery=N`。

## 控制端 Viewer 示例

适用于只控制其他设备、不接受被控连接的客户端。

```yaml
RUSTDESK_PRESET_HARD_SETTINGS: >-
  {
    "conn-type": "outgoing",
    "disable-installation": "Y"
  }

RUSTDESK_PRESET_OVERRIDE_SETTINGS: >-
  {
    "custom-rendezvous-server": "rd.easylan.site",
    "relay-server": "rd.easylan.site",
    "api-server": "",
    "key": "你的key",
    "enable-check-update": "N",
    "allow-auto-update": "N"
  }

RUSTDESK_PRESET_BUILTIN_SETTINGS: >-
  {
    "hide-network-settings": "Y",
    "hide-server-settings": "Y",
    "hide-proxy-settings": "Y",
    "hide-websocket-settings": "Y"
  }
```

## Workflow 级别裁剪

以下裁剪不依赖三段 JSON，而是直接修改 workflow 构建步骤：

| 裁剪项 | 操作 |
| --- | --- |
| 只保留 Windows x64 | 只保留 Windows job |
| 去掉 MSI | 删除 `Build msi` 和对应上传 |
| 去掉便携 exe | 删除 `Build self-extracted executable` |
| 不上传解压目录 | 删除 `Upload unpacked artifact` |
| 不做签名 | 删除签名相关 step |
| 不打包远程打印驱动 | 删除 printer driver 下载/解压步骤 |
| 不打包虚拟显示驱动 | 删除 `usbmmidd_v2` 下载/解压步骤 |
| 不构建 topmost window | 删除 `build-RustDeskTempTopMostWindow` job 和下载 artifact step |
| 不启用硬编解码 | 去掉 build 命令里的 `--hwcodec` |
| 不启用 vram | 去掉 build 命令里的 `--vram` |
