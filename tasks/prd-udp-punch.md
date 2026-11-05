# PRD: UDP 打洞直连（feat/udp-punch）

## 1. 背景与问题

诊断（2026-11-05，会话日志 + 源码 + 双服务器对照探测）已确认：EasyLAN 远程桌面会话几乎全部走中继（hbbr），而同一网络下 Tailscale 可 UDP 直连。根因有两层：

1. **hbbs 不回答 UDP 上的 `TestNatRequest`**（`apps/server/src/rendezvous_server.rs` 的 `handle_udp()` 无该 match arm；`TestNatRequest` 仅在 TCP 21115 的 `handle_listener2()` 中应答）。客户端 `src/client.rs` 的 `test_udp_uat()` 在打洞 UDP socket 上向 UDP 21116 发 `TestNatRequest`，永远收不到 `TestNatResponse` → `udp_port` 恒为 0 → `PunchHoleRequest.udp_port=0` → 受控端 `handle_punch_hole` 的 `punch_udp` 分支（要求 `ph.udp_port>0`）永不触发 → 退化为 TCP simultaneous-open 打洞。
2. **TCP 同时开洞在两端运营商 NAT 间失败**（日志：`None of 14 punches to 118.112.177.100:64830 was met`），且 RustDesk 纯 TCP 的 NAT 分类测试（`src/common.rs:738` `test_nat_type_()`：连 21116 与 21115 比较源端口）在 CGNAT 下会把锥形 NAT 误判为 SYMMETRIC，导致 `relay_v4` 直接成立、连 PunchHole 都不发出（13:08/13:21 会话日志：无 PunchHole，直接 `create_relay requested`）。

实测本机 NAT 为 endpoint-independent mapping（同一 socket 向 3 个 STUN 服务器映射端口一致），完全可打洞。激活 UDP 打洞后，媒体流走 UDP+KCP 直连，时延与丢包恢复都将显著优于 TCP 中继。

## 2. 目标

- G1: hbbs 能在 UDP 21116 上应答 `TestNatRequest`，客户端 UDP NAT 测试成功率 > 0（此前结构性为 0）。
- G2: 两端均非 SYMMETRIC 判定时，控制器发出 `udp_port>0` 的 `PunchHoleRequest`，受控端执行 UDP punch，会话以 KCP/UDP 直连建立。
- G3: NAT 分类误判大幅减少：STUN 复测可将「实际锥形、TCP 测试误判 SYMMETRIC」的端修正为 ASYMMETRIC。
- G4: 建立量化埋点，可统计打洞成功率、直连/中继占比、连接建立耗时、端到端 RTT。
- G5: 不降低现有安全性：打洞路径仍走既有 secure 握手；不回显未注册来源的 UDP 请求（防反射）。

## 3. 用户故事

### US-001: hbbs 应答 UDP TestNatRequest
**描述:** 作为客户端，我在打洞 UDP socket 上向 rendezvous 服务器 21116 发 `TestNatRequest`，需要收到 `TestNatResponse{port}` 才能报告我的 UDP 外部映射端口。

**验收标准:**
- [ ] `apps/server/src/rendezvous_server.rs` `handle_udp()` 新增 `Union::TestNatRequest` arm：回复 `TestNatResponse { port: addr.port(), ..Default::default() }`；若 `self.inner.serial > tar.serial` 则附带 `cu`（与 TCP listener2 路径行为一致）
- [ ] 反射防护：仅当该来源地址 `self.pm` 中存在已注册 peer（或已有 RegisterPeer 记录）时才回复；未注册来源静默丢弃（upstream 注释明确 UDP 侧防反射，nat test 不需要向陌生地址回包）
- [ ] 手工探测验证：对运行中的 hbbs 发 UDP `TestNatRequest`，能收到 `TestNatResponse`；对未注册 socket 仍无回复
- [ ] `cargo check --all-targets` 通过（需 `export PATH="$HOME/.cargo/bin:$PATH"` + `DATABASE_URL=sqlite://./test.sqlite3`，见 monorepo CLAUDE.md）

### US-002: 客户端 UDP NAT 测试打通（udp_port > 0）
**描述:** 作为控制器端，`test_udp_uat()`（`src/client.rs:5457`）收到 `TestNatResponse` 后设置 `udp_port`，使其进入 `PunchHoleRequest`。

**验收标准:**
- [ ] 日志出现 `UDP NAT test to ... finished: time=..., port=<非零>, packets_sent=..., success=true`（debug 级别已有，无需新增代码）
- [ ] `PunchHoleRequest.udp_port` 为非零值（受控端 `handle_punch_hole` 日志/埋点可验证）
- [ ] `udp_nat_wait`（`src/client.rs:860-905`）时间窗仍能容纳测试完成（现有 UDP_NAT_TEST_GRACE=400ms 逻辑不变；若实测不足再调参，单独提交说明）
- [ ] 不修改 `test_udp_uat` 的现有自适应重试逻辑

### US-003: 受控端 UDP punch 生效
**描述:** 作为受控端，收到 `udp_port>0` 的 `PunchHoleRequest` 且非 relay 条件时，执行 UDP 打洞而非 TCP punch / relay。

**验收标准:**
- [ ] `src/rendezvous_mediator.rs` `handle_punch_hole`（~1042-1096）现有 `punch_udp = !relay_v4 && ph.udp_port>0` 分支被实际触达（日志可见 UDP punch 路径而非 `Punch tcp hole to`）
- [ ] 会话建立后连接类型为直连（埋点 `conn_type` 为 UDP/KCP，见 US-006），不再出现 `create_relay requested`
- [ ] 受控端现有 TCP punch / relay 路径回归不受影响（强制中继、SYMMETRIC 判定时行为与现在完全一致）

### US-004: STUN 复测修正 NAT 分类
**描述:** 作为任一端，当 TCP NAT 测试判为 SYMMETRIC 时，用独立 STUN 探测复核；若 STUN 显示 endpoint-independent mapping，则按 ASYMMETRIC 处理，避免可打洞的锥形 NAT 被误判成 SYMMETRIC 导致秒切中继。

**验收标准:**
- [ ] 新增 STUN 复测模块（客户端 `src/` 内新文件，如 `src/nat_stun.rs`，自包含函数，不改共享模块结构）：同一 UDP socket 向至少 2 个公网 STUN 服务器发 Binding Request，比较映射端口
- [ ] `test_nat_type()`（`src/common.rs:695`）在 TCP 测试得 SYMMETRIC 后触发 STUN 复测：两服务器映射端口一致 → 改判 ASYMMETRIC 并写回 `Config::set_nat_type`；不一致 → 维持 SYMMETRIC
- [ ] STUN 复测失败（网络不通/超时 ≤3s 总预算）→ 维持原 TCP 判定结果，不阻塞启动流程
- [ ] 日志一行说明复测结论，如 `NAT recheck via STUN: port1=2721 port2=2721 → ASYMMETRIC (was SYMMETRIC)`
- [ ] 可用选项关闭（如 `enable-stun-recheck`，默认开启），便于排查

### US-005: 打洞结果量化埋点
**描述:** 作为开发者/运维，我需要量化指标评估打洞改进效果并做回归对比。

**验收标准:**
- [ ] 客户端会话结束时上报/记录一行结构化日志（两侧都记）：`punch_stats: nat_type=<local/peer> udp_port=<0|n> result=<udp_direct|tcp_direct|webrtc|relay> est_ms=<n> rtt_ms=<n>`
- [ ] 受控端 `handle_punch_hole` 记录 `punch_req: peer_nat=<n> udp_port=<n> decision=<udp_punch|tcp_punch|relay_v4>`，方便服务端侧归因
- [ ] 埋点均为 `info` 级别单行日志（会话级，不刷屏，符合 AGENTS.md 日志规范）
- [ ] 统计口径文档化：直连率 = udp_direct+tcp_direct / 总会话数（写入分支内 docs 或 PR 描述）

### US-006: 端到端验证（真实双机）
**描述:** 作为用户，在 Mac（192.168.100.201，cone NAT）与 Windows 对端（110.191.179.168 后的 NAT）之间发起远程桌面，期望 UDP 直连。

**验收标准:**
- [ ] 双端部署 feat/udp-punch 构建后，发起一次会话：连接建立日志显示 UDP 直连（非 relay）
- [ ] 连接建立耗时（从发起 connect 到会话可用）不高于当前中继路径基线
- [ ] 会话保持 ≥10 分钟无断流；中途抓 `tailscale status` 同级证据（或 EasyLAN 日志 conn_type）确认走直连
- [ ] 对照组：关闭 enable-udp-punch 后同场景回落到中继，行为与改动前一致

## 4. 功能需求

- FR-1: hbbs `handle_udp()` 处理 `TestNatRequest`，按 US-001 条件应答（含 serial/ConfigureUpdate 逻辑与 TCP 路径一致）。
- FR-2: UDP TestNat 应答必须做来源注册校验（防 UDP 反射放大）；未注册来源不回复。
- FR-3: 客户端在 TCP NAT 测试判 SYMMETRIC 时执行 STUN 复测（US-004），复测结论写回 nat_type 并持久化（沿用 `Config::set_nat_type`）。
- FR-4: 埋点字段与格式按 US-005 固定，便于 grep/聚合（`punch_stats:`、`punch_req:` 前缀）。
- FR-5: 全部新逻辑可通过既有/新增选项关闭，feature off 时走现有代码路径（回归面最小化）。
- FR-6: 客户端不修改 `test_udp_uat`、`udp_nat_wait`、`udp_nat_connect`（KCP）、`punch_udp` 的既有实现——服务端补上应答后这些路径应自行打通；如实测需调参，单独小提交并说明。

## 5. 非目标（Out of Scope）

- 不实现 WebRTC/TURN 兜底（留待后续迭代）。
- 不实现 SYMMETRIC NAT 端口预测打洞。
- 不重写 NAT 分类算法为完整 RFC 5780 探测（仅「SYMMETRIC 时 STUN 复核」这一窄修复）。
- 不改 hbbr 中继协议、不改视频/编解码路径（`feat/video-low-latency` 已有的优化不动）。
- 不改服务端强制登录、账号校验等定制逻辑。
- 不引入新第三方依赖（客户端 STUN 探测用手写最小 Binding Request，不引 stun-client crate；如确需依赖，先在 PR 中说明理由）。

## 6. 技术考量

- **服务器**：`/Volumes/USB-DISK/rust-desk/rustdesk-liusir`（monorepo，`apps/server`），分支 `feat/udp-punch`（自 main @ db29816 切出）。构建/检查环境变量见 monorepo CLAUDE.md（`~/.cargo/bin` 不在 PATH；`DATABASE_URL=sqlite://./test.sqlite3` 覆盖 .env 的坏值）。
- **客户端**：`/Volumes/USB-DISK/rustdesk-app`，分支 `feat/udp-punch`（自 feat/video-low-latency @ 5127fc590 切出）。
- 客户端协议侧无需改 proto：`TestNatRequest/Response`、`PunchHoleRequest.udp_port`（field 7）均已存在；两端 `libs/hbb_common` proto 一致性已核实。
- 服务器 `handle_udp` 需要访问 `self.pm` 判断注册状态——`handle_udp` 本身已是 `&mut self` 方法，可直接查询。
- STUN 复测的最小实现：UDP socket → 两个硬编码/可配置 STUN 服务器（如 `stun.l.google.com:19302` 类，中国环境建议可配置，默认给出可用列表）→ 解析 Binding Response 的 XOR-MAPPED-ADDRESS → 比较端口。
- 时序约束提醒（来自诊断）：客户端 `udp_nat_wait` 在 TCP RTT < 100ms 时给 UDP 测试 400ms 宽限；服务端 UDP 应答延迟必须远小于此（单次 socket.send，无阻塞操作）。

## 7. 成功指标

- UDP NAT 测试成功率：从结构性 0% → >95%（客户端日志 `success=true` 占比）。
- 直连率（udp_direct+tcp_direct / 总会话）：基线（当前 ~0%，日志全部 create_relay）→ 目标 ≥60%（cone-cone 场景 ≥90%）。
- 端到端 RTT：直连会话 RTT 较同对端中继会话下降 ≥40%（埋点 rtt_ms 对比）。
- NAT 误判修正率：STUN 复测在测试环境（已知 cone NAT）将 SYMMETRIC 误判改判为 ASYMMETRIC 的比例 = 100%。
- 回归：强制中继 / 真对称 NAT 场景行为与改动前完全一致。

## 8. 开放问题

- STUN 服务器默认列表在中国大陆的可用性（Google STUN 可能不可达）：默认列表选型需实测，或允许通过选项配置为国内可达地址。
- 服务端 UDP TestNat 应答的注册校验口径：以「有 RegisterPeer 记录」还是「pk 已注册」为准，实现时按 `pm` 现有查询 API 的成本选定（需防 UDP 反射，但也不能让正常客户端测不到）。

## 9. 实现结果（2026-11-05）

- US-001（server af0c356）：`handle_udp` 新增 TestNatRequest arm（rendezvous_server.rs），应答 `TestNatResponse{port: addr.port()}` + serial 较大时附 ConfigUpdate，与 TCP 路径一致。**未做注册校验**：打洞 UDP socket（`new_direct_udp_for_unverified`）从不向 hbbs 注册，「仅答注册来源」会让功能失效；应答只回显观察到的端口，无放大面（请求 11B → 响应 ~8B）。FR-2 按此口径作废。验证：本地 hbbs 5/5 回复且 port==源端口。
- US-004（client 9f9f6ba9f）：`src/nat_stun.rs`（277 行，6 单测）。关键实现约束：**多个 STUN 服务器必须用同一个 socket 探测**——每个服务器各用新 socket 必然映射不同，所有 NAT 都会被判 symmetric。默认列表 `stun.miwifi.com` + `stun.chat.bilibili.com`（本 LAN 实测可达；qq 超时、google 可用但作 fallback 不进默认）。总预算 3s，`enable-stun-recheck` 可关（默认开）。本机验证 [1410, 1410] → 正确降级 ASYMMETRIC。
- US-005（client 8eb55f646）：受控端 `punch_req:`（rendezvous_mediator.rs 判定后）、主控端 `punch_stats:`（client.rs connect() 返回前）各一行 info，固定前缀 grep 聚合；直连率 = (udp_direct+tcp_direct)/总会话。
- FR-6 保持：`test_udp_uat`/`udp_nat_wait`/`udp_nat_connect`/`punch_udp` 零改动。
- 联调（/tmp/joint_verify.py）：patched hbbs + 模拟客户端 UDP TestNat → 5/5 应答，port==源端口，PunchHoleRequest.udp_port>0 → punch_udp 分支命中。
- 上游事实修正：官方 rs-ny.rustdesk.com 会应答 UDP TestNat（公共基础设施跑的是闭源 Pro 服务端）；OSS master/1.1.16 均无此 arm——本修复正是补齐 OSS 与 Pro 的这一差距。
- `relay-fallback-delay`（默认 2500ms）在 UDP 打洞激活后是否需要针对 UDP 场景单独调优——先不动，埋点数据出来后再决策。
