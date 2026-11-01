# EasyLAN RustDesk 本地编译指南（macOS arm64）

本仓库 = 上游 rustdesk **v1.5.1** + EasyLAN 预设定制（`git describe --tags` → `1.5.1-6-ga6a020a9d`）。
本文记录在 macOS arm64 上跑通 **Sciter 原生客户端**（`cargo build`，非 Flutter）的实测步骤。

## 一、已验证可用的构建链路

以下结果均在 8 核 / 16GB / macOS arm64 上实测通过：

| 命令 | 结果 |
| --- | --- |
| `cargo check -p rustdesk` | ✅ 45s |
| `cargo build -p rustdesk` | ✅ debug 二进制 162 MB |
| `cargo build --release -p rustdesk` | ✅ 7m46s，产物 27 MB |
| `cargo check -p rustdesk --features hwcodec` | ✅ 1m33s |
| `cargo test -p rustdesk --lib` | ✅ 216 passed / 0 failed |

产物：`target/release/rustdesk`，`--version` 输出 `1.5.1`。

## 二、三个前置条件（不满足会报错）

### 1. vcpkg + C++ 依赖（关键）

**症状**：不设 `VCPKG_ROOT` 时构建失败：

```
scrap v0.5.0 build.rs:89 panicked:
Could not find package in /opt/homebrew/Cellar/libyuv.
```

**原因**：`libs/scrap/build.rs:78-110` 的 `link_homebrew_m1()` 在 macOS aarch64 上只认
`/opt/homebrew/Cellar/<name>`；而 **Homebrew 没有 libyuv 这个 formula**（`brew install libyuv`
会报 `No available formula`），所以这条路走不通，必须用 vcpkg。

**解决**：vcpkg 必须钉在 `vcpkg.json` 的 baseline commit 上（与 CI 的 `VCPKG_COMMIT_ID` 一致）：

```bash
git clone --depth 1 https://github.com/microsoft/vcpkg.git /Volumes/USB-DISK/vcpkg-tmp
cd /Volumes/USB-DISK/vcpkg-tmp
git fetch --depth 1 origin 9e593bb18ea69cc5095e012465dcd675a822ed0d
git checkout 9e593bb18ea69cc5095e012465dcd675a822ed0d
./bootstrap-vcpkg.sh -disableMetrics
```

安装依赖（**必须在仓库根目录执行**，否则 vcpkg 找不到 `vcpkg.json` manifest）：

```bash
cd /Volumes/USB-DISK/rustdesk-app
export VCPKG_ROOT=/Volumes/USB-DISK/vcpkg-tmp
"$VCPKG_ROOT/vcpkg" install --x-install-root="$VCPKG_ROOT/installed" --triplet arm64-osx
```

约 3.5 分钟装好：aom 3.14.1、ffmpeg 7.1.1、libjpeg-turbo 3.2.0、libvpx 1.15.2、libyuv 1857、opus 1.5.2。

> ⚠️ `--x-install-root` 只能出现一次，否则报
> `error: the option 'install-root' was specified multiple times`。

### 2. nasm

`res/vcpkg/aom/portfile.cmake:2` 要求 nasm：

```bash
brew install nasm
```

### 3. Sciter 运行时库

Sciter 版客户端启动时需要 `libsciter.dylib`，否则 panic：

```
error: 'libsciter.dylib' was not found neither in PATH nor near the current executable.
```

注意：它是**运行时**依赖，`cargo build` 不会失败，只有 `./target/release/rustdesk --help`
这类真正启动 UI 的命令才会崩。下载放到仓库根目录（该文件已在 `.gitignore:57`）：

```bash
curl -L -o libsciter.dylib \
  https://raw.githubusercontent.com/c-smile/sciter-sdk/master/bin.osx/libsciter.dylib
```

（可选）内联 UI 资源：`python3 res/inline-sciter.py`

## 三、构建命令

```bash
cd /Volumes/USB-DISK/rustdesk-app
export PATH="$HOME/.cargo/bin:$PATH"
export VCPKG_ROOT=/Volumes/USB-DISK/vcpkg-tmp

cargo build -p rustdesk            # debug
cargo build --release -p rustdesk  # release（LTO + strip，约 5-8 分钟）
cargo test  -p rustdesk --lib      # 单元测试
```

## 四、预设注入（EasyLAN 定制功能）

预设定制通过 `option_env!` 在**编译期**注入，见 `src/common.rs:2521 apply_compile_time_preset_client()`：
把 `RUSTDESK_PRESET_HARD_SETTINGS` / `_OVERRIDE_` / `_BUILTIN_` 解析进
`config::{HARD,OVERWRITE,BUILTIN}_SETTINGS`，外加 4 个旧式覆盖键
（`RUSTDESK_PRESET_RENDEZVOUS_SERVER` / `_RELAY_SERVER` / `_API_SERVER` / `_KEY`）。

验证方式：把值编码进二进制后 `strings` 就能看到。

```bash
export RUSTDESK_PRESET_HARD_SETTINGS='{"hide-powered-by-me":"Y","disable-account":"Y"}'
export RUSTDESK_PRESET_RENDEZVOUS_SERVER="test.example.com"
cargo build --release -p rustdesk
strings target/release/rustdesk | grep test.example.com   # 命中即注入成功
```

`build.rs` 顶部有对应的 7 条 `cargo:rerun-if-env-changed=`，改了环境变量会自动重新编译。

> ⚠️ 这些环境变量是**编译期**生效的：换一个值必须重新 `cargo build`，
> 清缓存不是必需的（`rerun-if-env-changed` 会触发重编）。

## 五、未跑通的部分：Flutter 版（含 GUI 现代化界面）

`cargo build` 只产出 Sciter 版二进制。要出 Flutter 版（`build.py --flutter`）还缺三样，本机均不具备：

1. **Flutter 3.24.5 SDK** —— 未安装（`flutter`/`dart` 都不在 PATH）。
2. **完整 Xcode** —— 只有 Command Line Tools（`xcode-select -p` →
   `/Library/Developer/CommandLineTools`），`flutter build macos` 需要 `xcodebuild`。
   系统盘 `/` 仅剩 9.5 Gi，装不下 Xcode（约 40 GB）。
3. **bridge 生成文件** —— `src/bridge_generated.rs`、`flutter/lib/generated_bridge.dart`、
   `flutter/macos/Runner/bridge_generated.h` 都被 gitignore，需由 CI
   `.github/workflows/bridge.yml` 产出。**`AGENTS.md` 明确禁止本地跑
   `flutter_rust_bridge_codegen`**（版本钉死 1.80.1 + `--features uuid`，本地难复现）。

结论：Flutter 版建议继续交给 CI 的 `build-easylan-desktop.yml` 矩阵构建，本地只做 Rust 侧验证。

## 六、其他已知事项

- **bridge 文件缺失不影响桌面端构建**：`src/lib.rs:39-41` 只在
  `any(target_os=android, target_os=ios, feature="flutter")` 时才编译 `bridge_generated`。
- **类型报错先怀疑脏工作区**：曾遇到 `src/ui.rs:39` 的 `E0308 expected &mut [String],
  found &mut CoreMainAction`，实际是 `src/main.rs` 处于合并前的旧版本。用
  `git diff 1.5.1 -- <file>` 确认后再调试。
- **Flutter UI 改造已提交**：窗口侧栏、主页重设计、设置裁剪、白标与 Xcode 27
  适配的完整说明见 `docs/UI-REDESIGN-2026-11.md`。
- **预设注入**：`source local_build_env.sh` 会经 `.github/presets/env.sh`
  导出 `RUSTDESK_PRESET_*`，把 `full-client.json` 的默认值与 UI 裁剪烘进
  每次本地构建（改动 `src/` 或预设后需先 `cargo build --lib` 再 `flutter build`）。
