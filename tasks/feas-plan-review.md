# 对抗式评审：feas-minimal-plan.md（t4）

> 评审对象：`rustdesk-app/tasks/feas-minimal-plan.md`（t3 产出，235 行）
> 输入审计：`rustdesk-liusir/tasks/feas-server-audit.md`（t1）、`rustdesk-app/tasks/feas-client-audit.md`（t2）
> 评审方式：**只读代码复核**，未改任何代码、未执行构建。行号口径：服务端 = `rustdesk-liusir/apps/server/src/rendezvous_server.rs`（HEAD `a635a9b`）；客户端 = "master 口径"，即上游标准客户端代码。
> **master 口径可信度已验证**：`rustdesk-app` 的 `master` 与 `upstream/master` 在 `src/client.rs`、`src/rendezvous_mediator.rs` 上 blob 完全相同；`src/common.rs` 仅在 2496 行之后多出 107 行定制（`apply_compile_time_preset_client`）。方案引用的 `common.rs` 行号全部 ≤1357，位于差异区之前，**对上游官方代码同样有效**。

---

## 1. 总体结论

**结论：需修改后执行（needs_revision）。核心方向批准，修正项集中在"文档/规格层"，不需要重构方案。**

- 方案的**主干 M1+M2+M3 经逐行复核全部成立**，且是**不可再压缩的最小集**（三个缺陷分别在三个独立组装点，任缺一个则 UDP 路径断链，见 §5）。
- 但存在 **3 个 major**：①M3/M4 的关键前提（"同一 addr 键覆盖只剩 1 条记录"）与代码不符，由此推出的"M3 必须与 M4 一起上"不成立；②M6 会覆盖 `lan_overridden`/`ALWAYS_USE_RELAY` 强制写入的 `nat_type=SYMMETRIC`，违反方案自己的回归红线 1；③M1 把未鉴权 UDP 源自报的 `udp_port` 变成可信输入，`§5.2` 的暴露面结论不完整。
- 另有 8 个 minor（场景矩阵超时数字、M5 TTL 理由、多 server 观测、PunchPool 32 容量门、IPv6/WebRTC 默认关、根因③机制措辞、≥90% 无出处、埋点行号尾部偏差）。
- 修正全部是"改文档 + 加 1-2 行守卫条件"级别，**不影响 M1/M2/M3 的落地顺序与发布方式**。修正清单见 §7。

**给 captain 的执行建议**：按 §7 的 10 项修正更新方案文档后即可开工；M6 在补上 `!lan_overridden` 排除条件前**不要实现**。

---

## 2. 事实核查：方案断言逐条对照（全部复核通过项）

以下为方案中被引用且**与代码一致**的关键断言（这些是方案的可信地基）：

| # | 方案断言 | 代码证据 | 判定 |
|---|---|---|---|
| V1 | af0c356 已补 UDP `TestNatRequest` 应答 | `rendezvous_server.rs:521-535`（`port: addr.port()`，`serial > tar.serial` 时附 `ConfigUpdate`）；客户端 `client.rs:5480-5549`（`test_udp_uat`，`:5545` 写 `udp_port`） | ✅ |
| V2 | M1 前提：`PunchHole` 组装处 `udp_port` 恒 0 | `:1155-1161`，`..Default::default()`；受控端 `rendezvous_mediator.rs:1095` `punch_udp = !relay_v4 && ph.udp_port > 0` | ✅ |
| V3 | M2 前提：`PunchHoleResponse` 从不置 `is_udp` | `:946-952` 无 `set_is_udp`；客户端硬门 `client.rs:1030-1036`（`udp.0.take()` 无条件 + `udp_nat_port>0 && ph.is_udp && s.is_some()`） | ✅ |
| V4 | M2 "socket_addr 无需改动" | UDP 到达时 `:947 socket_addr = AddrMangle::encode(addr)`，`addr` 即受控端打洞 socket 观察地址 ⇒ 控制器 `client.rs:1033 s.connect(peer_addr)` 目标正确 | ✅（方案论证正确） |
| V5 | M3 前提：UDP 到达时响应用 UDP 发给 A 的 TCP 端口 | `:957-961`；`addr_a = AddrMangle::decode(&phs.socket_addr)`（`:925`）与 `tcp_punch` 键同构（均经 `try_into_v4`，`libs/hbb_common/src/lib.rs:117-128`） | ✅ |
| V6 | `send_to_tcp` 是一次性消费 | `:1193-1199`（`tcp_punch.lock().remove(...)` 后 `send_to_sink`） | ✅ |
| V7 | 协议零变更 | `protos/rendezvous.proto`：`PunchHole.udp_port=4`(:37)、`PunchHoleResponse.is_udp=9`(:112)、`PunchHoleRequest.udp_port=7`(:27)、`relay_servers=11/13`(:44/:118)、`ConfigUpdate`(:121) | ✅ |
| V8 | M2 与 nat_type 不冲突 | `PunchHoleResponse` 的 `nat_type=5` 与 `is_local=6` 同在 oneof（`:106-108`），`is_udp=9` 独立字段 ⇒ `set_is_udp` 与 `:953-955 set_nat_type` 互不干扰 | ✅（方案未提及，正向补证） |
| V9 | 只有 `handle_hole_sent` 需要改（无第 4 个组装点） | `handle_local_addr`（`:965-1009`）结构同构，但 `LocalAddr` 只由受控端 `handle_intranet_` 经 **TCP** 发出（`rendezvous_mediator.rs:831-847` `socket.send_raw`）⇒ 永不带 `socket`，不在 M2/M3 射程内 | ✅（对最小性是利好） |
| V10 | §6.2 判据 3 可用 | `:929-934` `"{} punch hole response to {:?} from {:?}"`，`socket.is_none()→"TCP"` 否则 `"UDP"` | ✅ |
| V11 | relay 时序：direct 失败后才请求 relay | `client.rs:1550-1563`；无 WebRTC 时 `relay_fallback_delay_ms` 不参与竞速（`:1533 select_ok(direct_futures)`） | ✅ ⇒ "打洞失败不延长 relay 时延"成立 |
| V12 | §6.4 "relay_geo 有 7 个 43e1049 相关单测" | `apps/server/src/relay_geo.rs` 共 55 个 `#[test]`，其中 `candidates_*` 恰 7 个（`:2004-2070`） | ✅ |
| V13 | 开 UDP 打洞就会并发 2 个 `PunchHoleRequest` | `client.rs:558-563` 守卫（force_relay / `udp.0.is_none() && !webrtc` / `!tcp_punch_allowed()` 时只发一个）；`tcp_punch_allowed()` 默认 true（`option2bool` 对 `enable-*` 空值返回 true，`libs/hbb_common/src/config.rs:2924-2926`） | ✅ 且是**主流路径**，非边缘场景 |
| V14 | CGNAT 双源端口误判机理 | `common.rs:738-789`（server1=21116、server2=21115，`:780-785` 端口比较）；结果粘滞（`:695-735` 一次即 break） | ✅ |
| V15 | `relay_v4` 两个 OR 项一个可改一个不可改 | `rendezvous_mediator.rs:1091-1094`（`ph.nat_type` 转发值 + `Config::get_nat_type()` 本地值） | ✅ |
| V16 | §6.2 "punch_stats/punch_req 是定制端埋点" | `rustdesk-app/src/client.rs:1667-1683`、`src/rendezvous_mediator.rs:1097-1112`（均为 feat/udp-punch 的 US-005 归因行） | ✅ |

---

## 3. Finding 列表

### F1 [major] M3/M4 的"同一 addr 键覆盖、只剩 1 条记录"与代码不符；"M3 必须与 M4 一起上"不成立

- **问题**：方案 §3.3 风险段与 §7.2 断言"两个请求按同一 addr 键插入，后一个覆盖前一个，`tcp_punch` 里只剩 1 条记录；若 M3 在第一个响应上消费掉它，第二个请求的响应将无处可送。**因此 M3 必须与 M4 一起上**"。
- **证据**：`client.rs:824` `let mut socket = connect_tcp(&*rendezvous_server, CONNECT_TIMEOUT).await;` —— **每次 `_start_inner` 都新建一条 TCP 连接**；两个 `_start_inner`（`:541-554` preferred 带 udp、`:569-583` fallback 传 `(None,None)`）因此来自**不同源端口**；服务端 `:610 tcp_punch.insert(try_into_v4(addr), sink)` 写入的是**两条不同键**，不存在覆盖，也不存在"只剩 1 条"。
- **影响**：若实现者相信该前提，会去做不必要的去重/保留第一个 sink 的改动（违反最小性），并把验证精力错配到"2 请求覆盖"上。
- **要求的最小修正**：把 §3.3 风险段与 §7.2 该条改为"两请求来自不同 TCP 连接（`client.rs:824`），`tcp_punch` 为两条独立记录，互不覆盖、互不消费"；**M4 从改动项降级为验证项**（§6.3 的"2 请求场景"行保留作为实测项）；删除"M3 必须与 M4 一起上"的耦合要求。
- **附带（真正的残余风险，建议一并写入 §7.2）**：3 次 punch attempt 中只有第 1 次会 insert sink（`:610` 的 `sink.take()` 语义；`client.rs:985` 后续 attempt 复用同一 socket 但服务端 sink 已空、不会重新 insert）⇒ 若受控端响应晚于第 1 个 attempt 窗口（>3s），M3 的 `tcp_punch` 已无记录 → 退回 UDP → 与今天一样被 NAT 丢弃。**这是既有行为、不是回归**，但属于已知限制。

### F2 [major] M6 会覆盖 `lan_overridden`/`ALWAYS_USE_RELAY` 强制的 `nat_type=SYMMETRIC`，违反自家回归红线 1

- **问题**：M6 在 `:1157 nat_type: ph.nat_type` 处把转发值改写为 `ASYMMETRIC`，但 `:1105-1112` 的 `lan_overridden`（`ALWAYS_USE_RELAY || (peer_is_lan ^ is_lan)`）已经在 `:1111` 把 `ph.nat_type` 改写为 `SYMMETRIC`（注释即 `// will force relay`）。M6 的判据"控制器自报 SYMMETRIC + 观测表命中"无法区分"客户端自报的 SYMMETRIC"和"服务端强制写入的 SYMMETRIC"——无论取改写前还是改写后的值，都会把强制中继场景改成尝试 UDP punch。方案 §4 矩阵"强制中继 → relay（不变），置信度 高"与 §5.4 红线 1 因此**在 M6 开启时不成立**。
- **证据**：`:1105-1112`、`:1111`、`:1157`；受控端 `rendezvous_mediator.rs:1091-1095`（`relay_v4` 第一项被中和后 `punch_udp` 变真）。
- **要求的最小修正**：M6 条件显式加 `!lan_overridden`（MUST_LOGIN/licence 拒绝路径在 `:1029-1081` 已提前 return，天然不受影响）；§5.4 红线 1 增加一条 "`NAT_TYPE_RELAX=Y` + `ALWAYS_USE_RELAY=Y`/跨网段" 的改动前后对照记录；§4 矩阵"强制中继"行标注 "M6 开启时依赖本修正"。

### F3 [major] 场景矩阵的回落时延数字与代码不符；目标场景的真实等待上限（18s）完全缺失

- **问题**：§4 矩阵写 cone–symmetric "约 3s 后回落"、symmetric–cone "可能先耗 3s UDP"；且全文未提 cone–cone 场景 UDP 失败后 TCP punch 仍可等 18s。
- **证据**（`client.rs:1431-1460`）：
  - `is_local || peer_nat_type == SYMMETRIC ⇒ connect_timeout = MIN(1000ms)`；
  - `peer_nat_type == ASYMMETRIC && my_nat_type == ASYMMETRIC ⇒ CONNECT_TIMEOUT(18s)`，`direct_failures>0 ⇒ punch_time_used*6`；
  - `my_nat_type == SYMMETRIC ⇒ MIN`；`connect_timeout==0 ⇒ punch_time_used*(3|6)`。
  推论：
  - **cone–symmetric**（受控端真对称）：受控端 `relay_v4` 成立 → 不发 `PunchHoleSent` → 控制器 `is_udp` 门不过 → `udp.0` 被吞掉（`:1030-1036`）→ **根本不建 UDP future** → 回落由 TCP punch 的 MIN=1s 决定，不是 3s。
  - **symmetric–cone**：控制器 `peer_nat_type=ASYMMETRIC`、`my_nat_type=SYMMETRIC` ⇒ TCP punch 只等 1s；但若 UDP future 已建立，`punch_udp` 仍跑到自己的 `MAX_TIME=3s`（`common.rs:3039`，超时 `bail!` 在 `:3057`）⇒ UDP 分支最长 3s+KCP(1s)。矩阵数字不全。
  - **cone–cone（目标场景）**：UDP 失败后 TCP punch 仍等 `CONNECT_TIMEOUT=18s`（首次）或 `punch_time_used*6`（有失败史）⇒ 最坏 18s 才 relay。**与今天逐位一致（今天同一分支同一超时），不构成回归**，但矩阵必须写出来，否则 §6.3 "3s 内回落、不劣于现状" 的通过条件会拿错基准。
- **要求的最小修正**：三行数字按上述口径改写；§6.3 增加"记录控制器 `peer address: ..., timeout: {}` 日志行（`client.rs:1461`）以实测 `connect_timeout`"，用实测值而非估计值判定"不劣于现状"。

### F4 [major] M1 把未鉴权 UDP 源自报的 `udp_port` 当可信输入；§5.2 暴露面结论不完整

- **问题**：§5.2 声称"M1/M2/M3 都不新增对外发送的 UDP 包……不改变 hbbs 对第三方的暴露面"。该结论只覆盖 hbbs 自身，漏了"**hbbs 指挥受控端向第三方指定地址发包**"这条新面：`udp_port` 一旦被回填，恶意源可以让任一已注册受控端向任意 `addr:port` 执行 `punch_udp`（`common.rs:3035-3094`，3s、约 20+ 个探测包）——一个源地址被冒充、目标任选的小型 UDP 反射面。
- **证据**：`:499-510`（UDP `PunchHoleRequest` arm 只查 `pm.is_in_memory(&ph.id)`；**无 `check_ip_blocker`**——该函数仅在 RegisterPk `:779` 调用一次；`MUST_LOGIN=N`（OSS 默认）时无任何鉴权，`MUST_LOGIN=Y` 时 token 校验在共享的 `:1029-1081` 内、只验证"是登录用户"不验证"是否有权连该 peer"）→ `:1242-1257` → `:1155-1161`。
- **要求的最小修正**（二选一，1-2 行）：①`udp_port` 只在请求经 **TCP** 到达时回填（标准客户端只走 TCP，`client.rs:953-985`），UDP arm 保持 0；②要求 UDP 源 IP 与该 peer 已注册的 rendezvous 地址一致。同时 §5.2 补一句"UDP arm 无 IP blocker，M1 不得把其 `udp_port` 当可信输入"。

### F5 [minor] M5 的 TTL 理由与代码不符；§6.2 判据 1 阈值偏松

- **证据**：`client.rs:896-916`（`udp_nat_wait = rtt < TCP_RTT_PLAUSIBLE_MIN ? UDP_NAT_TEST_GRACE(400ms, :663) : rtt/2`）；UDP `TestNatRequest` 与 TCP connect 并发发出。⇒ TestNatRequest→PunchHoleRequest 间隔 ≈ TCP connect 时间 + ≤1s，**与"3 次 punch attempt 的总时长"无关**（attempts 发生在 PunchHoleRequest 之后）。
- **要求的最小修正**：§3.5 TTL 理由改为"覆盖 TCP connect + `udp_nat_wait` + 一次 PunchHole 往返（量级 1-2s），30s 已足够宽"；§6.2 判据 1 阈值改为"TTL 内且与 PunchHoleRequest 源 IP 相同"。

### F6 [minor] M5/M6 的观测键只想到 IP，多 rendezvous server 场景会失效

- **证据**：`client.rs:828-837`（首个 server TCP 连接失败时改连 `servers` 列表）；UDP TestNatRequest 只发给第一个 server ⇒ UDP 观测落在 hbbs#1、PunchHoleRequest 可能到达 hbbs#2 ⇒ hbbs#2 的 M5 表无记录 ⇒ M6 门永不命中、§6.2 判据 1 失真。**M1-M3 不受影响**（`udp_port` 是 A 的打洞 socket 公网映射端口，与哪台 server 观测无关；`socket_addr` 用 A 在该 server 上的 TCP 观察地址，自洽）。
- **要求的最小修正**：§5.3 或 §3.5 加一条"M5 表是进程内单点观测；多 rendezvous server 时 M6 判据可能不命中，M1-M3 不受影响"。

### F7 [minor] `PunchPool` 容量门（32）未进场景矩阵与验收

- **证据**：`rendezvous_mediator.rs:116` `UDP_PUNCHES = PunchPool::new(32)`、`:119` `TCP_PUNCHES = 32`；`punch_udp_hole` 在无槽位时直接 return（`:1215-1223`）、**不发 `PunchHoleSent`** ⇒ 控制器收不到响应、超时回落。⇒ 并发 UDP punch >32 时第 33 个起静默失败；"cone–cone ≥90%"隐含"并发不超 32"。
- **要求的最小修正**：§4 或 §7.2 加一条已知限制；§6.3 加一项 >32 并发的观察（服务端无日志，只能靠 hbbr 连接数反推——这也是 M5 补不上的观测盲区，应写明）。

### F8 [minor] IPv6 punch 与 WebRTC 对标准客户端默认关闭，矩阵/不做项未点明

- **证据**：`common.rs:1327-1339` + `:1341-1357`（`enable-ipv6-punch`/`enable-webrtc` 与 `enable-udp-punch` 同走 `get_local_option` 的 "N" 默认）；`config.rs:2924-2926`。⇒ 标准客户端对自建 server 的可用直达路径**只有 TCP punch 与 UDP punch**；`relay_fallback_delay_ms` 在无 WebRTC 时也不参与（`:1533`）。
- **要求的最小修正**：§4/§7.1 加一段说明。这既是"UDP 是唯一新增直达选项"的理由，也是"relay 兜底未被削弱"的论据。

### F9 [minor] 根因③ 的机制描述不精确

- **证据**：`client.rs:1467-1474`（`allow_tcp_punch` 只 push `connect_tcp_local`，A 侧**不 listen**）；`rendezvous_mediator.rs:1532-1546`（B 侧 `punch_tcp_until_connected` 既 `new_listener` 又重复 punch，靠同时打开）。⇒ "两端同时 connect 后谁都不 accept" 不准确。
- **要求的最小修正**：§2 表格③ 改为"A 只 connect、B listen+重复 punch，依赖同时打开；跨对称 NAT 时 SYN 无法穿越"。（结论"服务端不可行"不受影响。）

### F10 [minor] "cone–cone ≥90%" 是估计值无出处；af0c356 的实证应注明出处

- **证据**：`rustdesk-liusir` git log 显示 `af0c356` 是**本地 commit**（作者 逐梦ing，2026-11-05），message 记录对官方公共 `rs-ny.rustdesk.com` 的 UDP TestNat 应答实证 6/6，并注明 OSS 1.1.16/master 源码均无该 arm。⇒ §1"官方 Pro 服务端本来就应答 UDP TestNat"**有实证支撑**；但 "≥90%" 仍是估计。
- **要求的最小修正**：把 ≥90% 标注为"待 §6 实测的目标值/估计"，并在 §1 注明 af0c356 commit message 的实证出处。

### F11 [minor] 埋点行号引用尾部偏差

- **证据**：§6.2 引 `client.rs:1666-1679`；实际 log 宏为 `rustdesk-app/src/client.rs:1667-1683`。`punch_req:` 引 `rendezvous_mediator.rs:1097-1112` ✅ 精确。
- **要求的最小修正**：行号改为 `1667-1683`。

---

## 4. 遗漏扫描：三层根因之外的失败路径

| 候选第 4 条路径 | 核查结果 |
|---|---|
| `handle_local_addr` 是否有同构缺陷（第 4 个组装点） | **无**。`LocalAddr` 只由受控端 `handle_intranet_` 经 TCP 发出（`rendezvous_mediator.rs:831-847`），服务端 `:965-1009` 永不带 `socket` ⇒ 方案只改 `handle_hole_sent` 是完备的（利好最小性） |
| IPv6 | 标准客户端对自建 server 默认关（F8），死路径 |
| WebRTC 兜底 | 同上，默认关 ⇒ UDP 失败后只剩 TCP punch + relay |
| hbbr relay 分配时序 | relay 请求在 direct 失败之后（`client.rs:1550-1563`）；无 WebRTC 时 `relay_fallback_delay` 不参与竞速（`:1533`）⇒ **无新增时延**，方案判断正确 |
| key/token 交换窗口 | **发现一个方案未收的窄化条件**：`legacy_secure` 时控制器跳过 UDP NAT 等待（`client.rs:896`）⇒ `udp_port` 可能为 0 ⇒ M1 对该子类无效。建议 §4 加一行（minor，并入 F8 修正） |
| relay-fallback-delay 与 punch 竞速 | 不适用（无 WebRTC） |
| 多 ID 服务器场景 | 见 F6（M5/M6 观测失效，M1-M3 不受影响） |
| 并发容量门 | 见 F7（PunchPool 32） |
| 双侧 CGNAT / 一端 Symmetric | 矩阵已覆盖；时延数字需按 F3 修正 |

**结论：三层根因之外没有第 4 个服务端结构性缺口**；上述均为默认关闭的死路径、容量限制或观测盲区，不需要新的改动项。

---

## 5. 最小性审查（明确判断）

**判断：方案的核心改动集 M1+M2+M3 是最小不可压缩集，方案的最小性主张成立；但 M4 应删除、M5 可裁剪。**

1. **不存在"一个 match arm + 一个决策条件"的更短路径**。三个缺陷分别位于三个独立组装点，且互相不可替代：
   - 只有 M1（`:1155-1161`）→ 受控端会 UDP punch，但控制器 `udp.0` 被 `:1030` 吞掉、永不建 UDP future（V3）；
   - 只有 M2（`:946-952`）→ 控制器保留 socket，但 `ph.udp_port` 恒 0 ⇒ 受控端 `punch_udp` 为假、根本不发 `PunchHoleSent`；
   - 只有 M3（`:957-961`）→ 两者都可能就绪，但响应用 UDP 发给 A 的 TCP 端口被 NAT 静默丢弃。
   三者必须同时在场，缺一即断链。✅ 方案的最小性论证正确。
2. **M4 应删除**（F1）：其存在的前提是错的；保留为 §6.3 的实测项即可。修正后改动集 = **M1 + M2 + M3 + M5**（M6 门控）。
3. **M5 可裁剪为 M5-lite**：若 M6 推迟发布，M5 可先只做"UDP TestNatRequest arm 内一行 debug 日志 + 复用 `:929-934` 既有日志行"，把只读查询命令留到 M6 立项时再加。方案当前的 M5（静态表 + TTL + 容量上限 + 命令通道）是为 M6 的门控判据服务的，属于合理但不紧迫。
4. **必要性/可回退性/开关**：
   - M1/M2/M3：必要（见 1）；可回退 = 重新部署旧二进制 + hbbs 重启（方案已说明）；**无独立开关**——建议增加一个环境变量总闸（如 `UDP_PUNCH_FORWARD=off`，仿 `:107-111` 静态风格），使灰度期可原位回退而不用回滚二进制。这是建议而非要求（S-1）。
   - M5：必要（标准客户端无埋点，§6 的服务端证据唯一来源）；纯可观测、无行为影响。
   - M6：必要性质疑（收益不确定，方案自己也说"默认关、不绑定发布"）；有开关 ✅；但必须先修 F2。
5. **协议面**：全部复用既有字段、无 submodule 变更 ✅（V7/V8），与 43e1049 的兼容策略一致。

---

## 6. 安全审查

| 面 | 结论 |
|---|---|
| UDP 反射/放大 | hbbs 自身不新增外发 UDP 包（TestNatResponse ~8B vs 请求 ~11B）✅。**但** M1 之后 hbbs 会指挥受控端向攻击者选定地址发约 20+ 个探测包/次（3s），源被冒充、目标任选 → F4，需加 TCP-only 回填守卫 |
| 未注册来源 | UDP `PunchHoleRequest` arm（`:499-510`）无 IP blocker、无注册校验；`MUST_LOGIN=Y` 时只验 token 有效性、不验授权关系。今天因 `udp_port=0` 后果有限，M1 放大了后果 → F4 |
| source 校验 | `phs.socket_addr` 是受控端回传的 A 的观察地址（`:925`），M3 用它查 `tcp_punch`——键与插入时同构（`try_into_v4`）✅；但 `AddrMangle` 的 v4 编码带时间盐（`lib.rs:131-152`）、v6 为原样 16B+port（`:154-160`），原生 IPv6 客户端（非 v4-mapped）时 `try_into_v4` 不转换、`tcp_punch` 键为 v6，回程仍自洽 ✅ |
| relay 兜底是否被削弱 | **未被削弱**：relay 请求仍在 direct 全败之后（`client.rs:1550-1563`）；UDP future 失败不拖累 `select_ok` 里的 TCP punch（`:1533`）；最坏等待与今天同一超时（见 F3）。§5.4 红线 4（TCP 到达仍走 `send_to_tcp`、不置 `is_udp`）经 `:957-961` + M3 只改 `socket.is_some()` 分支得到保证 ✅ |

---

## 7. 验收性审查与修正清单

**验收性结论：方案能够用标准客户端客观判定直连，判据链完整**（客户端 UI 直连/中继标识 + `client.rs:1047 "Hole Punched"` / `:1053 "relay requested from peer"` + hbbs `:929-934` UDP 行 + `:1149-1154` Punch hole request 行 + 受控端抓包 + hbbr 连接数反推）。两点需补：
- "直连率 = 1 - relay_session/total" 的**分母必须限定**为"UDP 打洞已开 + 非 LAN + 非强制中继"的会话，否则被大量从未尝试直连的会话稀释，指标失真；
- 所有时延类判据必须"改动前/改动后"两次记录（§6.4 已有此要求，但需覆盖 F3 的三个场景）。

### 修正清单（按优先级，全部为文档/小守卫级）

| # | 对应 finding | 动作 |
|---|---|---|
| C1 | F4 | `udp_port` 仅在 TCP 到达时回填（或校验 UDP 源 IP 与注册地址一致）；§5.2 补"UDP arm 无 IP blocker" |
| C2 | F2 | M6 条件加 `!lan_overridden`；§5.4 红线 1 增加 M6 开启时的对照项；M6 未修前不实现 |
| C3 | F1 | 改正 §3.3/§7.2 的"同键覆盖"论述；M4 降级为验证项；补记"attempt 2/3 无 sink 可回"的既有限制 |
| C4 | F3 | 场景矩阵三行时延数字改为 MIN=1s / 3s+KCP / 18s 口径；§6.3 增加 `connect_timeout` 实测日志行 |
| C5 | F10 | "≥90%" 标注为估计/目标值；注明 af0c356 commit message 的实证出处 |
| C6 | F5 | 修正 M5 TTL 理由与 §6.2 判据 1 阈值 |
| C7 | F6 | 补"多 rendezvous server 时 M5/M6 观测可能失效，M1-M3 不受影响" |
| C8 | F7 | 补 PunchPool(32) 容量门为已知限制 + §6.3 观察项 |
| C9 | F8 | 补"IPv6/WebRTC 默认关 ⇒ UDP 是唯一新增直达选项、relay 兜底未削弱"；补 `legacy_secure` 窄化条件 |
| C10 | F9/F11 | 根因③ 机制措辞；埋点行号 1667-1683 |
| S1 | （建议） | 为 M1-M3 增加 `UDP_PUNCH_FORWARD=off` 总闸，支持灰度期原位回退 |

---

## 8. 一页结论

- **可执行性**：需修改后执行。M1+M2+M3 主干**批准**，是最小不可压缩集，协议零变更、回归红线 1/4/5/7 经复核可保证；M4 删除、M6 补守卫后再评估、M5 可先做 lite 版。
- **是否需推倒重来**：不需要。10 项修正全部是文档级 + 2 处 1-2 行守卫条件。
- **最大剩余不确定性**（方案已诚实标注，评审认可）：CGNAT 误判类只能部分缓解；标准客户端的 UDP 打洞开关是人工的，这是天花板。
