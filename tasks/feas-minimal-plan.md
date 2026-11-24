# 最小修改方案：标准客户端打洞直连的可行性综合（t3）

> 输入：`rustdesk-liusir/tasks/feas-server-audit.md`（t1，服务端审计）、`rustdesk-app/tasks/feas-client-audit.md`（t2，标准客户端审计）、两份 `tasks/prd-udp-punch.md`。
> 本文为**方案文档，不含代码改动**；所有引用行号可在上述两份审计报告中找到，或来自本文复核过的源码（已注明文件:行）。
> 复核基线：rustdesk-liusir `a635a9b`（server）、rustdesk-app `0454d61b4`（client，feat/udp-punch）；"master 口径"指上游标准客户端代码（导出件 `/tmp/master_ref/`）。
> 修订记录：t3 初版 → t4 对抗式评审（`rustdesk-app/tasks/feas-plan-review.md`，needs_revision：3 major + 8 minor）→ **t8 按评审 §7 修正清单逐项修订**（M4 降级为验证项、M6 加 `!lan_overridden`、M1 加 TCP-only 回填、场景矩阵时延数字更正、M5 TTL/多 server 说明、`≥90%` 标注为估计值、埋点行号更正），并新增 §7.3「本期实现口径」对齐 M1-M3 代码状态 → **t9 按 t7 实现评审（`feas-impl-review-t5-round1.md`，needs_revision：0 blocker / 0 major / 5 minor）修复**（F-R1 注释改函数名/代码片段定位、F-R4 `udp_port` 赋值收为 rustfmt 单行、F-R2/F-R3 重写 §5.2 与 §4 的安全/机理口径并在 §7.2 增补 4 条已知限制/权衡、F-R5 仅记录不改）。

---

## 1. 可行性总结论（一段话）

**在「客户端为未修改的官方构建」这一硬约束下，纯服务端改动无法让 UDP 打洞默认生效，但可以让「已经手动打开 UDP 打洞选项的标准客户端」在锥形-NAT 场景下完成 UDP 直连；对 CGNAT 误判为 SYMMETRIC 的一端，纯服务端只能部分缓解，不能根治。**

具体说：官方标准客户端打洞 UDP socket 的前置开关 `enable-udp-punch` 在指向自建 rendezvous server 时默认返回 `"N"`（t2：`src/common.rs:1352-1368` + `is_public:1311-1322`），而服务端没有任何协议杠杆能改写这个本地开关（t1：`ConfigUpdate` 只带 `{serial, rendezvous_servers}`，标准客户端根本不解析它），因此「出厂默认设置的标准客户端」在任何服务端方案下都只会走 TCP punch → 失败 → relay。这个开关是服务端不可逾越的墙，只能由用户在客户端界面上手动打开，或改客户端默认值（t1 的 S5，服务端不可为）。

一旦用户手动打开 UDP 打洞，链路上还剩 4 个服务端结构性缺口（t2 的 G3/G4/G5/G6），全部可在不改协议、不改客户端、不改 proto 字段号的前提下补齐；补齐后 cone-cone 场景预期 UDP 直连率 ≥90%（**此为待 §6 实测确认的目标值/估计，尚无实测出处**，评审 F10；与官方公共 rs-ny 服务端的能力持平这一论断有实证出处：af0c356 的 commit message 记录了本地对官方 `rs-ny.rustdesk.com` 的 UDP TestNat 应答实测 6/6，且确认 OSS 1.1.16 与 master 均无该 arm）。cone-symmetric 场景 UDP 打洞在原理上不可行（对称 NAT 无稳定映射），应回落到既有 TCP punch / relay，行为与今天一致；symmetric-symmetric 无解。CGNAT 误判场景是本方案收益最大也最不确定的一类：受控端若被判 SYMMETRIC 会**立刻**中继（`rendezvous_mediator` 的 `relay_v4` 同时看 `ph.nat_type` 与本地 `Config::get_nat_type()`，t2 的 C8），服务端只能改写转发值、改不了客户端本地值，所以只能「部分缓解」，且这一类的根治只能落在客户端 STUN 复测（PRD US-004，已在 feat/udp-punch 落地，不属于本服务端方案）。

一句话排序：**先交付 C3/C4/C5/C6（纯服务端、协议零变更、标准客户端立即可用），再评估 C7（有开关、可回退、预期收益不确定），C8 与 C9 明确不做。**

---

## 2. 三层根因逐项对账（对照 PRD 诊断）

| # | PRD 诊断的根因 | 审计证据（行号） | 标准客户端侧的真实门 | 判定 |
|---|---|---|---|---|
| ① | hbbs 不应答 UDP `TestNatRequest` → `udp_port=0` | 服务端 `rendezvous_server.rs:517-535` 已由 af0c356 补上 UDP arm；t2 G2 确认 af0c356 是必要条件 | G3 `PunchHoleRequest.udp_port`（`client.rs:960` 发出）被服务端 `PunchHole` 组装丢弃（`rendezvous_server.rs:1155-1161`，`..Default::default()` 使 `udp_port` 恒 0）；受控端 `rendezvous_mediator.rs:1145-1148` 只在 `ph.udp_port>0` 时 punch UDP | **纯服务端可修复**（转发 `ph.udp_port`，t2 C3） |
| ② | TCP NAT 测试在 CGNAT 下把锥形误判 SYMMETRIC → 直连 relay | t2：`common.rs:738-789` 双源端口比较；`common.rs:695-735` 一次分类即 break、结果粘滞；t1：服务端无法触发重测 | 受控端 `relay_v4 = ph.nat_type==SYMMETRIC \|\| Config::get_nat_type()==SYMMETRIC \|\| ...`（t2 复核 `rendezvous_mediator.rs:1091-1094`），两个 OR 项一个是服务端可改的转发值、一个是纯本地值；控制器侧 `my_nat_type` 同样是本地值 | **部分可缓解**（只改得了转发值那一项；本地那一项服务端零控制权） |
| ③ | TCP 同时开洞在双侧 NAT 间失败 | t1 S8；t2 §3.3：TCP punch 依赖「同时打开」 | 标准客户端 TCP punch 路径本身没有改善余地。机制更正（评审 F9）：A 侧的 `allow_tcp_punch` 只 `push connect_tcp_local`、**A 不 listen**（client.rs:1467-1474），B 侧 `punch_tcp_until_connected` 既 `new_listener` 又重复 punch（rendezvous_mediator.rs:1532-1546），靠两端同时打开打通；跨对称 NAT 时 SYN 无法穿越 ⇒ 失败。**不是「两端同时 connect 后谁都不 accept」** | **不可行（服务端侧）**，属于已知 NAT 限制；缓解靠 UDP 路径替代（即 ①） |

补充两个 t2 新发现、PRD 未覆盖的服务端缺口（它们与 ①②③ 同层，都是「服务端组装消息时漏字段」）：

- ②′ `PunchHoleResponse.is_udp` 双方都不写（`libs/hbb_common` proto 里 `bool is_udp = 9` 仍在，t2 §2），而 `client.rs:1030-1036` 是 `udp.0.take()` 无条件执行 + `is_udp` 硬门：不置 `true` 则控制器的 UDP socket 被永久吞掉、根本不建 UDP future（t2 G4）。
- ③′ 受控端经 UDP 发 `PunchHoleSent` 时，`rendezvous_server.rs:918-963` 把 `PunchHoleResponse` 用 UDP 发给 A 的 **TCP** 源端口（`phs.socket_addr` 回传的是 A 的 TCP 观察地址）→ A 的 NAT 没有这个 UDP 映射 → 静默丢弃，控制器 3 次尝试全超时（t2 G6）。修法：优先用 `tcp_punch` map 里保留的 A 的 TCP sink 回送。

这三项（①/②′/③′）+ 客户端会并发发 2 个 `PunchHoleRequest`（t2：`client.rs:558-562`，服务端需容忍重复）构成本方案的服务端改动全集。

> **M4 定位更正（评审 F1）**：「服务端需容忍重复」这一项经复核**不需要改动**：两个请求来自两条独立 TCP 连接（client.rs:824），`tcp_punch` 是两条独立键、互不覆盖（见 §3.4）。因此服务端改动全集实为 **M1+M2+M3 三项**，M4 降级为 §6.3 验证项。

---

## 3. 最小修改方案（核心交付物）

排序原则：**纯服务端、协议零变更、既有路径改动最小者优先**；每项都可独立回退；可配置开关兜底。改动全部集中在 `rustdesk-liusir/apps/server/src/rendezvous_server.rs`（+ 极少量 `libs/hbb_common` 侧无需改动，见 §3.9）。

### 3.1 M1：`PunchHole.udp_port` 回填控制器 UDP 映射端口（最高优先）

- **改动位置**：`apps/server/src/rendezvous_server.rs` `handle_punch_hole_request()` 内组装 `PunchHole` 的位置（现为 `:1155-1161`）。
- **改动内容**：给 `PunchHole` 增加 `udp_port: ph.udp_port as _`（`ph` 是 `PunchHoleRequest`，其 `udp_port` 由标准客户端 `client.rs:960` 发自报值）。一行。
- **安全约束（评审 F4 / 评审 C1，硬约束，已并入改动内容）**：**`udp_port` 只能在 TCP 到达时回填**——UDP `PunchHoleRequest` arm（`:499-510`）没有 `check_ip_blocker`（该函数全仓仅在 `RegisterPk` 路径 `:779` 调用一次），`MUST_LOGIN=N`（OSS 默认）时也无任何鉴权，`MUST_LOGIN=Y` 时 `:1029-1081` 只验 token 有效性不验授权关系。若放任未鉴权 UDP 源自报 `udp_port`，攻击者可指挥任一已注册受控端向任意 `addr:port` 发 UDP 探测（客户端 `punch_udp`，common.rs:3035-3094，3s、约 20+ 包）。实现上是在 `handle_udp_punch_hole_request` 入口把 `ph.udp_port` 清零（1-2 行），标准客户端只在 TCP 发 `PunchHoleRequest`（client.rs:953-985），目标能力不受损。另加一条双保险：`lan_overridden` 时 `udp_port` 也必须是 0，不允许「强制中继却带 udp_port」的组合（与 §5.1 一致）。
- **影响的客户端行为**：受控端 `rendezvous_mediator.rs:1145-1148` 的 `ph.udp_port > 0` 分支首次可达 → 执行 UDP punch（`punch_udp_hole` → `udp_nat_listen` → KCP accept）；控制器侧 `client.rs:1031` 的 `udp_nat_port > 0` 条件同时成立（但还需 M2 才真正建 UDP future）。
- **风险**：低。`ph.udp_port` 是客户端自报值，最坏情况是自报错误端口 → 受控端向错误端口打洞、超时后仍走 TCP/relay，不改变失败语义；无新增状态、无新增分支以外的路径改动。
- **预期收益**：高。这是 UDP 路径的总闸门，此前恒为 0（结构性 0% → 可达）。
- **部署方式**：hbbs 热更新/重启即生效，无需客户端配合。

### 3.2 M2：`PunchHoleResponse.is_udp` 置位（UDP 传输时）

- **改动位置**：`rendezvous_server.rs` `handle_hole_sent()` 内组装 `PunchHoleResponse` 的位置（现为 `:946-952`）。
- **改动内容**：当 `socket.is_some()`（即受控端的 `PunchHoleSent` 从 UDP 到达）时 `p.set_is_udp(true)`。约两行；`socket_addr` 无需改动——UDP 到达时 `addr` 就是受控端打洞 socket 的观察地址（t2 C4 的第二半部分自动满足）。
- **影响的客户端行为**：控制器 `client.rs:1030-1036` 的 `if udp_nat_port > 0 && ph.is_udp && s.is_some()` 首次成立 → `s.connect(peer_addr)` 后保留 UDP socket，随后建立 UDP/KCP future。
- **风险**：低。仅在该消息本来就该走 UDP 的路径上置位；TCP 路径的 `PunchHoleResponse` 不置位（保持原语义，控制器仍走 TCP punch）。
- **预期收益**：高。没有它，M1/M3 都白做——控制器会把 UDP socket 永久吞掉。
- **部署方式**：hbbs 重启即生效。

### 3.3 M3：`PunchHoleResponse` 回程优先走 A 的 TCP sink

- **改动位置**：`rendezvous_server.rs` `handle_hole_sent()` 末尾的发送分支（现为 `:957-961`：`if let Some(socket) = socket { socket.send(...) } else { self.send_to_tcp(...) }`）。
- **改动内容**：把两个分支的顺序倒过来——先试 `self.tcp_punch` 里的 A 的 TCP sink（`send_to_tcp`），取不到再退回 UDP `socket.send`。约 3 行，无新状态（复用既有 map）。
- **影响的客户端行为**：控制器 3 次 `punch_attempts`（`client.rs:977-987`）内能真正收到 `PunchHoleResponse`，而不是 UDP 包被 NAT 静默丢弃后全部超时。
- **风险**：中。`send_to_tcp` 是 `remove()` 语义（一次性消费），响应必须落在受控端响应到达时仍存在的记录上。更正（评审 F1 / 评审 C3）：标准客户端连发的 2 个 `PunchHoleRequest`（t2：`client.rs:558-562`，一个带 UDP 一个纯 TCP）**不会**按同一 addr 键互相覆盖——客户端每次 `_start_inner` 都 `connect_tcp` 新建一条 TCP 连接（client.rs:824），两个请求来自不同源端口 ⇒ `tcp_punch` 是**两条独立键**，互不影响。因此 M3 **不需要**与 M4 一起上即可正确路由第 1 次 attempt 的响应。
- **M3 的真实残余限制（评审 F1，属既有限制、非回归）**：3 次 attempt 中**只有第 1 次会 insert sink**（rendezvous_server.rs:610 `sink.take()`；客户端 client.rs:985 起后续 attempt 复用同一 socket，服务端不会为该 socket 重新 insert）。所以受控端的响应必须落在第 1 个 attempt 的窗口内 M3 才能命中；若端到端 >3s，M3 查不到记录 ⇒ 退回今天的 UDP 回程 ⇒ 与改动前一样被丢弃。这不是新增失败模式，只是 M3 的生效边界，须在 §6.3 一并实测。
- **预期收益**：高。这是 UDP 路径的最后一跳；不修则 M1/M2/M4 全部无效。
- **部署方式**：hbbs 重启即生效。

### 3.4 M4：容忍重复 `PunchHoleRequest`（**降级为验证项，非改动项**）

> 评审 F1（评审 C3）后降级：前述「同键覆盖只剩 1 条」的前提不成立（client.rs:824 每次 attempt 新建连接 ⇒ 两条独立键），因此本项**不需要与 M3 配套上线**；其价值收敛为 §6.3 的一个实测确认项。实现口径上**本期不改代码**，是否保留 `tcp_punch` 的覆盖语义由实测结论决定。

- **改动位置**：`rendezvous_server.rs` `handle_tcp_punch_hole_request()` / `handle_udp_punch_hole_request()` 的调用点（`:610-618` 附近，`tcp_punch.insert(try_into_v4(addr), sink)` 处）。
- **改动内容**：不新增去重表，仅确认 `insert` 的覆盖语义不会让 A 的第二个请求把第一个的 sink 挤掉后无处回送——最小做法是**不改**，但需在验证中实测 2 请求场景；若实测第二个请求无 sink 可回，则退化为「按 addr 保留第一个、忽略后续重复插入」。此项结论依赖实测，见 §6。
- **影响的客户端行为**：无（不改变客户端可见语义，只保证服务端不因重复请求而丢响应）。
- **风险**：低（若最终判定为「无需改动」，则本项降级为验证项而非改动项）。
- **预期收益**：中（保证性，非增益）。
- **部署方式**：随 hbbs 重启。

### 3.5 M5：观测沉淀 + 审计日志（`punch_obs` 静态表）

- **改动位置**：`rendezvous_server.rs` 模块级新增 `static UDP_NAT_OBS`（仿 `:107-111` 的 `ALWAYS_USE_RELAY`/`RELAY_CANDIDATES_MAX` 静态风格）+ 在 `:521-535` 的 UDP `TestNatRequest` arm 里 `insert` + 在 `handle_punch_hole_request` 里 `get` + 在命令通道（`:1340-1348` 的帮助列表附近）加一条只读查询命令。
- **改动内容**：记录「UDP 源地址 → Instant」的映射，TTL 约 30s。TTL 依据（评审 F5 更正）：客户端的 `udp_nat_wait = rtt < TCP_RTT_PLAUSIBLE_MIN ? UDP_NAT_TEST_GRACE(400ms) : rtt/2`（client.rs:896-916），且 `TestNatRequest` 与 TCP connect **并发**发出（同函数内），所以「TestNatRequest 到达 → PunchHoleRequest 到达」的间隔约为「一次 TCP connect 耗时 + `udp_nat_wait`」，量级 1-2s，**与 3 次 punch attempt 的总时长无关**。30s 覆盖这个间隔再加一次 `PunchHole` 往返已足够宽。
- **影响的客户端行为**：无。纯可观测性。
- **风险**：低。唯一新增状态；须按 AGENTS.md/CLAUDE.md 的日志规范做**限流**（一次性 map 写入不是热点，但查询命令不要进热路径），并给出清理（TTL + 惰性清理，避免无界增长）。
- **预期收益**：中。它是 §6 验证与 M6 判定的唯一服务端侧证据来源；也是「标准客户端没有 `punch_stats:` 埋点」这一缺口的替代品。
- **部署方式**：hbbs 重启即生效。
- **观测键的多 rendezvous server 局限（评审 F6）**：客户端首个 rendezvous server TCP 连接失败时会改连 `servers` 列表里的下一个（client.rs:828-837），而 `TestNatRequest` 只发往**第一个** server ⇒ 观测落在 hbbs#1 的 `UDP_NAT_OBS` 里，而 `PunchHoleRequest` 可能到达 hbbs#2 ⇒ **M6 的门在多 server 场景永不命中**，且 §6.2 判据 1 在多 server 部署下失真。单 server 部署（武汉/成都各自单 hbbs）不受影响。**M1-M3 不受此影响**：`udp_port` 是 A 的打洞 socket 的公网映射，与哪台 server 应答无关；`socket_addr` 用的是 A 在该 server 上的 TCP 观察地址，自洽。

### 3.6 M6：条件性放宽 `nat_type`（可选，默认关）

- **改动位置**：`rendezvous_server.rs:1155-1161` 组装 `PunchHole` 处（`nat_type: ph.nat_type` 原样透传，t1 S2）。
- **改动内容**：新增下划线环境变量 `NAT_TYPE_RELAX=Y`（默认 `N`，走原路径）。开启后，当控制器自报 `SYMMETRIC` **且** M5 观测表显示该控制器刚刚成功完成 UDP NAT 测试（即它的 UDP socket 与 hbbs 通）**且 `!lan_overridden`** 时，把下发给受控端的 `nat_type` 改写为 `ASYMMETRIC`。
- **必须带 `!lan_overridden` 守卫（评审 F2 / 评审 C2，硬约束）**：`lan_overridden = ALWAYS_USE_RELAY || (peer_is_lan ^ is_lan)` 成立时，`:1111` 已**强制**把 `ph.nat_type` 写成 `SYMMETRIC` 以走 relay。若不设 `!lan_overridden` 守卫，M6 会把这份服务端强制的 `SYMMETRIC` 改写成 `ASYMMETRIC`，直接违反回归红线 1 与 §4 矩阵的「强制中继 → relay 不变」。`!lan_overridden` 天然排除 `ALWAYS_USE_RELAY=Y` 与跨网段 LAN 两种情形；MUST_LOGIN/licence 拒绝路径（`:1029-1081`）在组装 `PunchHole` 之前已 `return`，同样不受影响。
- **影响的客户端行为**：受控端 `relay_v4` 的第一项（`ph.nat_type==SYMMETRIC`）被中和 → 不再秒切 relay，改为尝试 UDP punch；控制器侧 `connect_timeout` 的 `my_nat_type`（本地值）**不受影响**（见风险）。
- **风险**：中，且收益不确定：
  - 它只改得了「转发值」这一半。`relay_v4` 的另一半是受控端本地 `Config::get_nat_type()`（t2 C8），若受控端自己误判，本项无效。
  - 控制器侧 `connect_timeout` 由**本地** `my_nat_type` 决定（`client.rs` 的 `else if peer_nat_type == ASYMMETRIC { match my_nat_type { SYMMETRIC => MIN, ... } }`，master `client.rs:1439-1470` 复核）：误判端若在控制器侧，放宽后它只肯等 ~1s，UDP punch（`punch_udp` 的退避最长约 3s）很可能来不及 → 仍然 relay，用户可见的是连接慢几秒。
  - 「成功完成 UDP NAT 测试」并不等于锥形 NAT（对称 NAT 也能向 hbbs 发 UDP），所以该判据只是启发式。
  - 多 rendezvous server 部署下 M5 的观测与 `PunchHoleRequest` 可能不在同一台 hbbs 上，门永不命中（见 §3.5）⇒ 多 server 场景应保持 `NAT_TYPE_RELAX=N`，或先把 M5 做成跨 server 共享状态。
- **预期收益**：高/中（在「控制器误判 + 受控端未误判」这一类上确定有效；其余子类不确定）。
- **部署方式**：hbbs 重启 + 环境变量；默认关闭，**不与 M1/M2/M3 绑定发布**。
- **兜底**：关掉即回到今天的逐位一致行为。

### 3.7 汇总表（改动位置 × 内容 × 影响 × 风险 × 收益）

| 项 | 位置（文件:函数） | 内容 | 影响的标准客户端行为 | 风险 | 收益 | 生效方式 |
|---|---|---|---|---|---|---|
| M1 | `rendezvous_server.rs:handle_punch_hole_request`（`PunchHole` 组装，`:1155-1161`） | `udp_port: ph.udp_port`（**仅 TCP 到达；UDP arm 入口清零；`lan_overridden` 时为 0**，见 §3.1） | 受控端进 UDP punch 分支 | 低 | 高 | hbbs 重启 |
| M2 | `rendezvous_server.rs:handle_hole_sent`（`PunchHoleResponse` 组装，`:946-952`） | UDP 到达时 `set_is_udp(true)` | 控制器保留 UDP socket 并建 UDP future | 低 | 高 | hbbs 重启 |
| M3 | `rendezvous_server.rs:handle_hole_sent`（发送分支，`:957-961`） | 优先 `tcp_punch` TCP sink，退回 UDP | 控制器能收到 `PunchHoleResponse` | 中 | 高 | hbbs 重启 |
| M4 | `rendezvous_server.rs:handle_*_punch_hole_request`（`:610-618`） | **本期不改代码**，降级为 §6.3 实测确认项（两键不覆盖，见 §3.4） | 无可见变化 | 低 | 中 | — |
| M5 | `rendezvous_server.rs` 模块级 + `:521-535` + 命令通道 | 观测表 + 只读查询命令 | 无 | 低 | 中 | hbbs 重启 |
| M6 | `rendezvous_server.rs:1155-1161` | `NAT_TYPE_RELAX` 条件下改写转发 `nat_type`（**条件含 `!lan_overridden`**，见 §3.6） | 受控端不秒切 relay | 中 | 高/中 | hbbs 重启 + env，默认关 |
| M7 | `rustdesk-app/src/common.rs` `get_udp_punch_enabled`（t2：`1352-1368`） | 自建 server 也默认返回 `"Y"` | 出厂即可 UDP 打洞 | 高（改所有自建用户默认行为） | 最高 | 需客户端升级 |
| M8 | `rustdesk-app/src/common.rs` `test_nat_type_`（t2：`738-789`） | CGNAT 误判修正 | 减少 SYMMETRIC 误判 | 中 | 中 | 需客户端升级 |

### 3.8 哪些需要客户端/协议配合，为什么无法避免

- **M7（客户端默认开 UDP 打洞）无法用服务端替代**：`get_udp_punch_enabled` 是纯本地配置读取（t2：`common.rs:1352-1368`，`is_public:1311-1322` 只看 host 名是否 `*.rustdesk.com`），`ConfigUpdate` 只带 `{serial, rendezvous_servers}` 且标准客户端不解析它（t1：全仓无 `configure_update` 命中）。**没有任何已存在的协议字段能驱动 LocalConfig 写入。** 若要服务端驱动，只能新增协议消息类型 → 那就不是「标准客户端」了，直接违背任务目标。
- **M8（CGNAT 误判根治）无法用服务端替代**：误判发生在客户端本地 `test_nat_type_`（t2：`common.rs:695-735` 一次即 break、结果粘滞），两端判据都是本地值（t2 C8）。M6 只是绕过转发值，绕不过本地值。
- **两者若要进标准客户端**，只能以上游可合入的形式：PRD US-004 的 `src/nat_stun.rs`（feat/udp-punch 已实现，277 行 + 6 单测，无新依赖，`enable-stun-recheck` 可关）就是按这个口径写的；M7 则是一个纯粹的默认值/策略变更（把 `is_public` 的判断从「公网 host」改为「所有 host」），diff 极小但会改变所有自建部署的默认行为，需上游单独讨论。**本服务端方案不依赖它们。**

### 3.9 协议面

M1/M2/M3/M4/M6 全部只使用**已存在的 proto 字段**（`PunchHole.udp_port=4`、`PunchHoleResponse.is_udp=9`、`PunchHoleRequest.udp_port=7`，见 `libs/hbb_common/protos/rendezvous.proto` 复核）。不新增字段、不改字段号、不动 `hbb_common` submodule → 与 43e1049 的「老端忽略未知字段、空列表零开销」兼容策略一致，标准老客户端不会因本方案出现解析问题。

---

## 4. 场景矩阵

「现状」= 只带 af0c356 的当前 hbbs + 手动打开 UDP 打洞的标准客户端；「方案后」= M1+M2+M3（+M4/M5），M6 单独标注。

| 场景 | 标准客户端现状 | 方案后预期路径 | 置信度 | 说明 |
|---|---|---|---|---|
| cone–cone | TCP punch 常成功；UDP 从不尝试（`udp_port=0`） | **udp_direct** | 高 | M1/M2/M3 齐备后 UDP 路径全通；这是 PRD 目标场景（G2），官方 Pro 服务端同能力。**UDP 失败后 TCP punch 不会再跑（评审 F-R3 更正）**：受控端在 `ph.udp_port > 0` 时提前 return（master `rendezvous_mediator.rs:1145-1150`），且 `punch_tcp` 要求 `ph.udp_port <= 0`（:1096）⇒ 受控端不再 `connect_tcp_local`（:1173）；只有控制器侧仍在后台 `connect_tcp_local`（master `client.rs:1468`）⇒ TCP 同时打开无法完成，只能等 `connect_timeout` 超时回落 relay。回落时延结论不变：控制器 `my=ASYMMETRIC && peer=ASYMMETRIC` 时 `connect_timeout = CONNECT_TIMEOUT = 18s`（首次，client.rs:1431-1460），`direct_failures>0` 时退化为 `punch_time_used * 6` ⇒ **最坏 18s 才回落 relay**，与今天同一分支同一超时，不构成回归；仅机理表述由「TCP punch 仍会跑」改为「等超时」。切换的行为边界与残留权衡见 §7.2 |
| cone–symmetric（控制器锥、受控端真对称） | TCP punch 失败 → relay | **relay**（先试 UDP，回落时延由 TCP punch 的 `MIN` 决定） | 高 | 对称 NAT 无稳定映射，UDP punch 原理上不通。回落时延的更正（评审 F3）：此场景**受控端 `relay_v4` 成立**（`ph.nat_type==SYMMETRIC` 且受控端自己的本地判定也是 SYMMETRIC）⇒ 它**不发 `PunchHoleSent`** ⇒ 控制器的 `is_udp` 门（client.rs:1030-1036）不过、`udp.0` 被吞掉 ⇒ **控制器根本不建 UDP future**，不存在「跑 3s UDP」⇒ 回落只由 TCP punch 的 `MIN`（1000ms，client.rs:1431-1460）决定，**不是 3s** |
| symmetric–cone（控制器真对称、受控端锥） | TCP punch 失败 → relay | **relay**（TCP 分支只等 ~1s；UDP future 若已建立则最长 3s） | 高 | 控制器 `peer=ASYMMETRIC && my=SYMMETRIC ⇒ MIN(1000ms)`（client.rs:1431-1460）；但 UDP future 若已建立，`punch_udp` 仍会跑到 `MAX_TIME=3s`（common.rs:3050，bail 在 :3062）⇒ UDP 分支最长 3s + KCP 约 1s。回落路径与今天一致 |
| symmetric–symmetric | 秒切 relay（受控端 `relay_v4` 成立） | **relay**（不变） | 高 | 行为必须与改动前一致 |
| CGNAT 误判（两端锥形，被 TCP 测试判 SYMMETRIC） | **立即 relay**（`relay_v4` 成立，连 `PunchHole` 都不发，PRD 日志即此） | **部分缓解**：控制器误判 + 受控端未误判 → 可 udp_direct；受控端误判 → 仍 relay；两端都误判 → 仍 relay | 中 | 纯服务端只改转发值（M6 默认关）；根治靠客户端 M8/US-004。**窄化条件（评审 F8）**：控制器侧 `legacy_secure` 时会跳过 UDP NAT 等待（client.rs:896 `udp.1.as_ref().filter(|_| !legacy_secure)`）⇒ `udp_port` 可能为 0 ⇒ M1 对该子类无效 |
| CGNAT 误判 + M6 开启 | 立即 relay | 控制器误判子类可 udp_direct；受控端误判子类不变 | 低-中 | 判据是启发式；且控制器本地 `my_nat_type` 会把等待压到 ~1s；多 server 部署下门可能永不命中（§3.5） |
| 强制中继（`ALWAYS_USE_RELAY` / `force_relay` / LAN 跨网） | relay | **relay**（不变） | 高 | `:1105-1111` 的 `lan_overridden` 已强制 `ph.nat_type = SYMMETRIC`，本方案不改该分支；**M6 开启时该不变性依赖 §3.6 的 `!lan_overridden` 守卫（评审 F2）** |
| 同内网（`same_intranet`） | `FetchLocalAddr` → 本地直连 | 不变 | 高 | `:1136-1147` 分支不走 `PunchHole`，M1 天然不影响 |

**已知限制（评审 F7，进矩阵与验收）**：客户端的 UDP punch 走 `PunchPool`，容量 **32**（`rendezvous_mediator.rs:116` `UDP_PUNCHES = PunchPool::new(32)`；`:119` `TCP_PUNCHES` 同为 32）。拿不到槽位时 `punch_udp_hole` 直接 `return`（`:1215-1223`）、**不发 `PunchHoleSent`** ⇒ 控制器收不到响应、超时回落 relay。因此并发 UDP punch > 32 时第 33 个起静默失败；「cone–cone 高直连率」隐含「并发不超 32」。服务端无日志可观测，只能靠 hbbr 连接数反推（见 §6.3）。

预期直连率口径（供 §6 统计）：cone–cone **≥90% UDP 直连为目标值/估计，尚无实测出处**（评审 F10），须由 §6 实测后回填；CGNAT 误判子类在 M6 关闭时接近 0、开启后取决于两端哪一侧误判；总体（打开 UDP 打洞的用户群）预期从当前 ~0%（TCP punch 跨运营商基本失败）提升到「锥形-NAT 对占多数」的水平，但**无法给出一个适用于全部用户的总数字**，因为客户端开关是人工的。§6 的直连率分母必须限定为「UDP 打洞已开 + 非 LAN + 非强制中继」的会话，否则会被从未尝试直连的会话稀释（评审 §7）。

---

## 5. 风险与回归面

### 5.1 对既有 relay/geo 选路的影响（43e1049、af0c356）

- **43e1049（就近候选 relay 列表）**：M1-M6 不触碰 `pick_candidates`/`note_client_relay`/`RELAY_CANDIDATES_MAX` 的任何代码路径。`PunchHole` 组装处（`:1155-1161`）与 `relay_candidates` 的注入（`:1115-1126`）是相邻但独立的字段赋值；`lan_overridden` 时 `relay_candidates` 已被置空（`:1115-1116` 的 `&& !lan_overridden`），M1 在该场景下同样应保持为空/不注入，**实现时必须把 `udp_port` 的赋值放在 `lan_overridden` 判断之外但不与之耦合**，避免出现「强制中继却带 udp_port」的组合。
- **af0c356（UDP TestNatRequest 应答）**：M5 只在其 arm 内加一次 map 写入，不改应答内容、不改 serial/ConfigUpdate 逻辑。af0c356 的「未做注册校验」口径（PRD §9：打洞 socket 从不注册，且应答只回显端口、无放大面）保持不变；M5 的 map 也**不是**注册校验，不能反过来被当成安全边界。
- **geo 地域选路 / 武汉成都双地域**：本方案不改 geo。唯一间接影响：UDP 直连成功率上升 → 经过 hbbr 的流量下降 → `note_client_relay` 的样本变少、就近选路的收益面变小（这是**期望结果**，不是回归）。部署上无新配置项（除 M6 的 `NAT_TYPE_RELAX`）。

### 5.2 安全面

- **UDP 反射/放大（实现后口径，评审 F-R2a 更正）**：af0c356 的应答是「回显观察到的端口」，请求 ~11B → 响应 ~8B，无放大；hbbs 自身对外发送的 UDP 包数量与放大比在 M1/M2/M3 前后**完全不变**（M1/M2/M3 只改变既有响应包的**传输通道与字段内容**）。但 M3 使原先被 A 的 NAT 静默丢弃的 UDP 应答改走 TCP 真正送达 ⇒ 控制器首次通过 `udp_nat_port > 0 && ph.is_udp && s.is_some()`（master `client.rs:1031`）建起 UDP future 并向 `p.socket_addr` 打洞（`common.rs` `punch_udp`，3s、约 20+ 个探测包）。这些探测包**是客户端既有行为、非服务端发送**，但「从不到达」变「到达」确实改变了实际触达量：**方案的真实代价是客户端侧多出有界的 UDP 触达（约 20+ 包/3s/会话），不是服务端放大面变大**。M5 的表按「UDP 源地址」索引，任何人 UDP 打一针都能插一条——因此它必须是**有界**的（TTL + 容量上限 + 惰性清理），否则是一个可被 UDP 洪水撑爆内存的 DoS 面。这是 M5 唯一需要认真对待的安全约束。
- **未注册来源（实现后口径，评审 F-R2b 更正）**：原断言「不存在『未注册来源直接触发回包』的新路径」**不成立**——其论据（「链路起点是 A 的已认证 PunchHoleRequest」）把意图当成了代码不变量。真实情况：UDP `PunchHoleSent` arm（`rendezvous_server.rs` 的 `handle_udp` 内，`Union::PunchHoleSent => ... handle_hole_sent(.., Some(socket))`）**无鉴权、无注册校验、无速率限制**，`phs.socket_addr` 完全由报文自身决定。任意 UDP 源伪造 `phs.socket_addr = A 的公网 TCP 地址`，即可让 hbbs 向 A 的 `tcp_punch` TCP sink 投递一次 `is_udp=true` 的应答，且该应答的 `socket_addr` 指向攻击者自选地址 ⇒ A 的客户端会向该地址发约 20+ 个探测包/3s（**有界反射**），并可能因 `udp.0.take()`（master `client.rs:1030`）被提前消费而**抢占**合法 UDP punch（拒予直连）。**前置条件**：攻击者知道 A 的公网 IP:端口，且落在 A 的 `tcp_punch` 项存活窗口内（连接关闭时才清理）。**定性**：不是新增放大源（请求数十字节、响应一次性、客户端探测有界），且「让 A 向某地址打洞」的原语在改动前已可经 TCP `PunchHoleSent` 达成（攻击者自建 TCP 连接、`socket_addr` 指向自己）；M3 的新增量仅是「可用伪造 UDP 源指向第三方」。已知限制与可选硬化见 §7.2。
- **伪造 `ph.udp_port`**：恶意客户端可以自报任意端口。后果是受控端向错误端口打洞（浪费 3s 带宽，量级等同于它本来就会做的 TCP punch），不获得任何信息增益，也不改变 hbbs 对第三方的暴露面。M6 若开启，会把「自报 SYMMETRIC + 观测表命中」作为改写 nat_type 的判据，此时伪造者可以让自己被判成 ASYMMETRIC —— 这只会让自己多试一次打洞，不影响他人。
- **M1 不得把 UDP 到达的 `udp_port` 当可信输入（评审 F4 / 评审 C1，硬约束）**：UDP `PunchHoleRequest` arm（`:499-510`）**不调用** `check_ip_blocker`（该函数全仓仅在 `RegisterPk` 路径 `:779` 调用一次），而 `MUST_LOGIN=N`（OSS 默认）下根本没有鉴权、`MUST_LOGIN=Y` 时 `:1029-1081` 也只校验 token 有效性而不校验「该来源是否有权代表这个受控端」。若允许未鉴权 UDP 源自报 `udp_port` 并回填，攻击者可指挥**任一已注册受控端**向任意 `addr:port` 发送 UDP 探测（客户端 `punch_udp`，common.rs:3035-3094，持续 3s、约 20+ 个探测包）——一个虽小但真实的 UDP 反射/骚扰面。**实现口径：`udp_port` 仅在 TCP 到达时回填（`handle_udp_punch_hole_request` 保持填 0）**，因为标准客户端只在 TCP 上发 `PunchHoleRequest`（client.rs:953-985）；这 1-2 行即消除该面，且不损失任何目标能力。备选方案是校验 UDP 源 IP 与注册地址一致，但那是「新增鉴权判断」，比前者重。

### 5.3 多地域部署（武汉/成都生产）

- 无新增单点：改动全在 hbbs 信令进程，与 hbbr 无关；两台 hbbs 独立生效，不需要同步新状态（M5 的表是进程内、可丢弃的）。
- **`MUST_LOGIN=Y` 的多 hbbs 部署本就不可行**（CLAUDE.md：账号过期校验读本地只读 SQLite），与本方案无关，但意味着不能指望「改一台验证、另一台灰度」之外的跨节点协同。

### 5.4 必须与改动前完全一致的行为（回归红线）

1. `ALWAYS_USE_RELAY=Y` / `force_relay` / LAN 跨网（`lan_overridden`）→ 必须仍强制 relay，且 `PunchHole` 不得携带会诱导 UDP punch 的字段。**逐项对照（评审 F2 要求）**：改动前后各跑一次 `NAT_TYPE_RELAX=Y + ALWAYS_USE_RELAY=Y`（或跨网段）组合，确认下发的 `PunchHole.nat_type` 仍为 `SYMMETRIC`、仍走 relay——该不变性由 §3.6 的 `!lan_overridden` 守卫保证，是 M6 的前置条件。
2. 受控端自报或本地判定 SYMMETRIC → 仍走 `relay_v4` 秒切中继（M6 关闭时）。
3. `same_intranet` → 仍走 `FetchLocalAddr`。
4. TCP `PunchHoleResponse`（受控端从 TCP 发 `PunchHoleSent`）→ 仍走 `send_to_tcp`，`is_udp` 不置位。
5. `RELAY_CANDIDATES=0`（默认）→ 不下发 `relay_servers`，与 43e1049 前逐位一致。
6. af0c356 的 UDP `TestNatResponse` 内容不变（`port` + 条件 `cu`）。
7. 客户端 UDP 打洞关闭时 → 整个 UDP 路径不被触达（`udp.0` 为 `None`，服务端改动全部是死代码）。

---

## 6. 验证方案（用标准客户端验收）

### 6.1 环境与构建

- 客户端：**官方未修改构建**（RustDesk 官方 release，不要用 feat/udp-punch 的自建客户端）。PRD 基线 1.1.16 一代结构上没有 UDP 打洞（t2：UDP punch 首发 1.4.0/7792ac148），所以验收客户端必须 ≥1.4.0；否则「UDP 打洞从未存在」会被误判成「方案无效」。若必须用 1.1.16，则只能验收「不劣化」而非「增益」。
- 服务端：本仓库 hbbs（`apps/server`，`cargo build --release`，`~/.cargo/bin` 在 PATH、`DATABASE_URL=sqlite://./test.sqlite3`）。
- 客户端侧唯一人工动作：在设置里打开 UDP 打洞开关（标准客户端的等价选项）。**这一动作必须写进验收记录**，否则结论不可解释。

### 6.2 判据（标准客户端没有埋点，用什么替代）

- **`punch_stats:` / `punch_req:` 不可用**：这两行是 feat/udp-punch 定制客户端的埋点（`rustdesk-app/src/client.rs:1667-1683`、`src/rendezvous_mediator.rs:1097-1112`，行号经评审 F11 复核），官方构建不会打印。§6 的口径沿用 PRD US-005 只适用于定制端。
- **替代判据（hbbs 侧，全部来自 M5 观测表与既有日志）**：
  1. `handle_udp` 的 UDP `TestNatRequest` 是否被应答 → 客户端 `udp_port>0` 的**服务端证据**。判据阈值（评审 F5）：M5 表条目**在 TTL 内、且其源 IP 与随后到达的 `PunchHoleRequest` 的 TCP 源 IP 相同**（双条件，避免同 NAT 后其他主机误命中）。
  2. `handle_punch_hole_request` 日志 `Punch hole ... request from ...` 中 `PunchHole` 是否携带非零 `udp_port`（M1 生效判据；加一行 debug 或从 M5 表比对）。
  3. `handle_hole_sent` 日志 `UDP punch hole response to ...`（现为 `:929-934` 的 `"{} punch hole response to {:?} from {:?}"`）→ 说明受控端确实从 UDP 回了 `PunchHoleSent`（M1 在受控端生效的判据）。
  4. `PunchHoleResponse` 走了哪条通道（M3 判据）：`send_to_tcp` 命中即 TCP sink 送达；若退回 UDP 需记录。
- **替代判据（客户端侧，标准客户端可观察）**：
  - 官方客户端连接成功后的连接类型显示（RustDesk UI 的「直连/中继」标识）——这是最接近用户真实体感的判据。
  - 客户端日志：标准客户端仍有 `Hole Punched ...`（`client.rs:1047`）与 `None of N punches to ... was met`、`relay requested from peer` 等既有行；UDP 直连成功时连接类型为 UDP/KCP。
  - **`connect_timeout` 实测（评审 F3 要求，判定「不劣于现状」的硬判据）**：记录标准客户端 `client.rs:1461` 的 `log::info!("peer address: {}, timeout: {}", peer, connect_timeout);` 这一行，用实测的 `connect_timeout` 值（而非本文的推断）来核对 §4 矩阵里的分支归属；所有时延类判据必须**改动前/改动后各记录一次**（评审 §7）。
- **替代判据（网络侧，最硬）**：在受控端抓包，看 21116/打洞端口上是否出现来自控制器公网地址的 UDP 包并建立 KCP 会话；同时对比 hbbr 中继流量（relay 端口 21117 的连接数与字节数）作为「直连率」的反向指标。
- **直连率统计口径（标准客户端版）**：`直连率 = (hbbr 上无对应会话的连接数) / 总会话数`，等价于 `1 - relay_session/total`，其中 relay_session 由 hbbr 侧连接日志统计（每次 `create_relay` 对应一条 hbbr 连接）。这比 PRD 的 `punch_stats` 口径粗（拿不到 `udp_direct` vs `tcp_direct` 的细分），但**不依赖定制客户端**；细分可通过抓包补。**分母必须限定为「UDP 打洞已开 + 非 LAN + 非强制中继」的会话**（评审 §7），否则被从未尝试直连的会话稀释。

### 6.3 双机场景与抓包要点

| 场景 | 部署 | 抓包/日志判据 | 通过条件 |
|---|---|---|---|
| cone–cone | 两端不同运营商家宽，均开 UDP 打洞 | 受控端 tcpdump `udp port <A 的打洞端口>`；hbbs 日志 | KCP over UDP 建连；UI 显示直连；hbbr 无新连接 |
| cone–symmetric | 一端置于对称 NAT（或云主机公网 IP + 严格防火墙） | 同上 + 客户端 `client.rs:1461` 的 `timeout: {}` 实测 | 回落时延由 TCP punch `MIN`（1000ms）决定（§4 更正：受控端 `relay_v4` 成立、不发 `PunchHoleSent`，控制器不建 UDP future），**用实测 timeout 值确认不劣于现状**，而非假定某个秒数 |
| 强制中继 | `ALWAYS_USE_RELAY=Y` | hbbs 命令通道 `aur Y` 后重连 | 与改动前逐位一致（§5.4 红线 1；含 `NAT_TYPE_RELAX=Y` 组合，若 M6 上线） |
| 2 请求场景（M4 验证项） | 标准客户端开 UDP 打洞（会并发 2 个 `PunchHoleRequest`） | hbbs 日志两条 request、两条 response | 控制器仍能收到 `PunchHoleResponse`；预期结论是「两个请求是两条独立 `tcp_punch` 键、互不覆盖，无需改动」（§3.4） |
| >32 并发 UDP punch（评审 F7） | 同时发起 >32 个 UDP 打洞会话 | 服务端**无日志**，只能靠 hbbr 连接数/回落会话反推 | 第 33 个起是否静默失败（`punch_udp_hole` 无槽位直接 return、不发 `PunchHoleSent`）；若失败率与容量门吻合，记录为已知限制 |
| 关闭 UDP 打洞 | 同 cone–cone 两端，开关关闭 | 抓包无打洞 UDP | 与今天完全一致 |

### 6.4 回归验证

- §5.4 的 7 条红线逐条跑一遍；每条都要有「改动前/改动后」两次记录。
- `cargo check --all-targets`（`apps/server`，环境变量见 CLAUDE.md）+ 现有 `cargo test --lib`（`relay_geo` 有 7 个 43e1049 相关单测，必须仍绿）。
- 灰度顺序：先 M1+M2+M3（M4/M5 为验证项/可观测项），M6 只在有 M5 数据支撑后再单独开。
- **建议的灰度保险（评审 §5 的 S-1，建议非要求）**：为 M1-M3 增加一个总闸环境变量 `UDP_PUNCH_FORWARD=off`（默认 on，即现行行为），灰度期一旦发现异常可原位关断回退，无需回滚二进制。成本约 1-2 行；若不接受新增开关，则以 `git revert` 本文件改动为回退路径。
- **验收记录必须注明（t9，因上一项未实现）**：实现方未采纳 `UDP_PUNCH_FORWARD=off` 总闸，回退路径就是 `git revert` ⇒ **灰度期若需关断 M1-M3，必须重启/回滚二进制，不存在原位关断手段**。这是实现方主动选择的备选口径，非偏离，但验收结论里必须写明，否则运维侧会误以为可以热关断。另需注明 §6.3 的客户端侧人工动作（手动打开 UDP 打洞开关）。

---

---

## 7. 明确的不做项（非目标）与遗留风险

### 7.1 不做项

1. **不让服务端改写客户端本地开关/本地 NAT 判定**（M7/M8）：协议上无路可走，且违背「标准客户端」前提。
2. **不做单侧 NAT 判定**（t1 S7）：受控端打洞 socket 从不向 hbbs 发 UDP，hbbs 只有单观测点，协议无下发探测指令的字段。
3. **不动 hbbr / 不延迟 relay**（t1 S8）：relay 请求发生在 direct 失败之后（`client.rs:1550-1574`），延迟 relay 只会拖长失败路径、不增加成功率。
4. **不新增协议字段/消息类型**：M1-M6 全部复用既有字段；新增字段会让标准老客户端走到「忽略未知字段」路径，收益为零、风险不为零。
5. **不引入新第三方依赖**（服务端无此必要；客户端 STUN 复测按 PRD §5 用手写 Binding Request，已在 feat/udp-punch 实现）。
6. **不改 `test_udp_uat` / `udp_nat_wait` / `punch_udp` / `udp_nat_connect` 的既有实现**（PRD FR-6）：这些是客户端路径，本方案不碰。
7. **不做 WebRTC/TURN 兜底、不做端口预测打洞、不改 hbbr 中继协议**（PRD §5）。**注意标准客户端对自建 server 的默认能力面（评审 F8）**：`get_udp_punch_enabled`/`get_ipv6_punch_enabled`/`get_webrtc_enabled` 均经 `get_local_option` 的 `"N"` 默认分支（common.rs:1330/1338/1346/1352）+ `config.rs:2924-2926` `option2bool`（`enable-` 前缀 `value != "N"`）⇒ 自建 server 场景下**只有 TCP punch 与 UDP punch 两条直达路径可用**，IPv6 punch 与 WebRTC 默认关闭；`relay_fallback_delay_ms` 在无 WebRTC 时不参与回落计时。本方案只覆盖 TCP/UDP punch 这两条。
8. **不把 M5 观测表当安全边界**（注册校验/防反射仍按 af0c356 口径，见 §5.2）。

### 7.2 遗留风险

- **客户端开关是人工的**：即使 M1-M3 全部生效，未手动打开 UDP 打洞的标准客户端仍然 100% 中继。这是本方案的天花板，也是唯一需要上游配合的点（M7）。
- **CGNAT 误判无法根治**：受控端本地误判时服务端零杠杆；控制器本地误判时，即使 M6 开启，`connect_timeout` 会被压到 ~1s（master `client.rs:1439-1470` 复核），UDP punch 可能来不及。要根治只能客户端 STUN 复测（PRD US-004，已实现但不在本服务端方案内）。
- **M6 是启发式**：「成功完成 UDP NAT 测试」≠ 锥形 NAT；开启后可能把少量真对称场景从「秒切 relay」变成「3s 后 relay」，用户可见为变慢。默认关闭 + 可一键回退是唯一合理的发布方式。
- **M3 的生效边界（评审 F1 更正）**：不是「M3 与 M4 耦合未实测」，而是「3 次 attempt 只有第 1 次 insert sink（`:610`；client.rs:985 起复用同一 socket 不重新 insert）」⇒ 受控端响应若晚于第 1 个 attempt 窗口（端到端 >3s），M3 查不到记录、退回 UDP 回程被丢弃。这是**既有限制、非新增回归**，须按 §6.3 实测确认其发生频率。
- **M4 的结论待实测确认**：两请求不覆盖是代码阅读结论（client.rs:824 每次 `_start_inner` 新建连接 ⇒ 两条独立键），需 §6.3 的 2 请求场景实测闭环；若实测推翻，再把 M4 从「无需改动」升级为「保留第一个 sink」的真实改动。
- **M5 表的内存面**：TTL + 容量上限必须实现，否则是可被 UDP 洪水利用的 DoS 面（§5.2）。
- **验收客户端版本门槛**：官方 <1.4.0 构建没有 UDP 打洞代码路径，任何在其上做的「直连率」测试都会得到 0，与方案有效性无关。
- **M3 引入的「伪造 UDP PunchHoleSent 指向第三方」已知限制（评审 F-R2b，本期不硬化）**：UDP `PunchHoleSent` arm 无鉴权/无注册校验/无速率限制，`phs.socket_addr` 由报文自身决定。**前置条件**：攻击者知道 A 的公网 TCP IP:端口，且落在 A 的 `tcp_punch` 项存活窗口内（A 的请求连接关闭时才清理）。**影响面**：(1) 有界反射——A 的客户端向攻击者自选地址发约 20+ 个探测包/3s；(2) 抢占——A 的 `udp.0` 被提前 `take()` 消费，合法 UDP punch 被拒予直连。**定性**：非新增放大源（改动前已可经 TCP `PunchHoleSent` 用同一原语让 A 向自己打洞），M3 仅新增「可用伪造 UDP 源指向第三方」这一增量。**可选硬化（另案评估，本期条件不含）**：校验 UDP `PunchHoleSent` 源 IP 与 `phs.id` 对应 peer 的注册 IP 一致；须注意 NAT 重绑定与多公网 IP 出口可能误杀合法流量，落地前需单独评估误杀率。
- **受控端 `punch_tcp → punch_udp` 切换的行为边界与残留权衡（评审 F-R3）**：切换条件是 `!relay_v4 && ph.udp_port > 0`（master `rendezvous_mediator.rs:1095`），即「A 自报非对称 + B 本地判定非对称 + A 的 UDP NAT 测试已成功」——正是目标场景。其余场景行为不变：A 自报 SYMMETRIC / B 本地判定 SYMMETRIC / A 的 UDP 测试失败或被 `legacy_secure` 跳过（⇒ `udp_port=0`）/ LAN 同内网 / 强制中继。**残留权衡**：B 侧网络封 UDP 但放行 TCP 时，原先可能的 TCP punch 变为 relay（受控端不再 `connect_tcp_local`，只有控制器侧单方面在后台 connect，同时打开无法完成）；A 侧封 UDP 时会因 `TestNatRequest` 失败使 `udp_port=0` 而自愈。这是与官方服务端能力对齐的**固有权衡、非实现缺陷**（官方 Pro 服务端若不下发 `udp_port`，官方 UDP punch 本身不可能工作）。
- **灰度回退方式（t7 §5.5 / §6.4 建议项未实现的后果）**：§6.4 建议的灰度总闸 `UDP_PUNCH_FORWARD=off` **未实现**，实现方选择了该节明列的备选口径（「以 `git revert` 为回退路径」）。**后果：灰度期若需关断本能力，必须重启/回滚二进制，无法原位关断。** 不构成偏离，但须写入验收记录。

### 7.3 本期实现口径（M1-M3 状态对齐）

> 本节用于让本文档与代码状态保持一致，避免「文档说已实现/未实现」与工作区不符。状态随 t5/t6/t7/t9 推进，须同步更新。

| 项 | 状态（修订时点：2026-11-11，t9 修复轮后口径） | 说明 |
|---|---|---|
| M1 | **已实现**（rustdesk-liusir 工作区未提交 diff；`handle_punch_hole_request` 的 `PunchHole` 组装处 `udp_port: ...` 在 `:1191`） | 含评审 F4 的 TCP-only 回填（`handle_udp_punch_hole_request` 入口 `ph.udp_port = 0`，`:1286`，参数为 `mut ph`）与 `lan_overridden ⇒ 0` 双保险；F-R4 已把赋值收为 rustfmt 规范单行 |
| M2 | **已实现**（`handle_hole_sent` `:960-963`） | `if socket.is_some() { p.is_udp = true; }`；`is_udp` 是普通字段（非 oneof），生成代码无 setter，直接赋字段 |
| M3 | **已实现**（`handle_hole_sent` `:974-984`） | 先 `tcp_punch.lock().await.remove(&try_into_v4(addr_a))`，命中则 `spawn(Self::send_to_sink)`，未命中退回 `socket.send(msg_out, addr_a)` |
| M4 | **未实现**（按评审 F1 降级为 §6.3 验证项） | 预期结论：两个请求是两条独立键、无需改动；由 t6 实测确认 |
| M5 | **未实现**（可裁剪为 lite：`TestNatRequest` arm 内一行 debug 日志 + 复用 `:929-934` 既有日志行） | 未上前，§6.2 判据 1 只能靠抓包与 `af0c356` 的既有应答日志替代 |
| M6 | **未实现**（默认关） | 规格含 `!lan_overridden` 守卫（§3.6） |
| M7/M8 | **未实现**（需客户端升级，本服务端方案不依赖） | — |

- t9（repair-round-2）已完成 t7 的 F-R1/F-R2/F-R3/F-R4：代码侧只改注释文字与 `udp_port` 赋值的 rustfmt 形态（**可执行逻辑零变化**，`git diff` 仍为 1 file / 41 insertions / 2 deletions，比 t5 结束时多出的 2 行全部是注释文字）；文档侧重写 §5.2 两条安全断言、更正 §4 cone–cone 行机理、在 §6.4 增补灰度回退口径，并在 §7.2 增补 2 条已知限制/权衡（F-R2b 伪造 UDP 源、F-R3 切换边界与残留权衡）+ 1 条灰度回退记录。F-R5（`handle_local_addr` 同构缺陷）按要求仅记录不改，见本节末条。
- 该 diff 尚未 `git commit`（工作区 ` M apps/server/src/rendezvous_server.rs`）；t7 判定为 needs_revision（0 blocker / 0 major / 5 minor），**第 2 轮评审尚未进行**。**在 t6/t7-round2 通过前，不要把本节状态当作已验证。**
- 行号口径：本文档中的 `:1155-1161` / `:946-952` / `:957-961` 等均为**改动前**（a635a9b）口径，用于描述「改动位置」；上表为**改动后**口径。行号会随后续改动漂移，**代码注释已按 t7 F-R1 要求改用函数名 + 代码片段定位（如 `send_to_tcp`、`handle_tcp 连接关闭清理`）以抗漂移**，语义定位请以函数名（`handle_punch_hole_request` / `handle_hole_sent` / `handle_udp_punch_hole_request`）为准。
- **`handle_local_addr` 的同构缺陷未修（t7 F-R5，仅记录）**：`handle_local_addr` 与 M3 修复前的 `handle_hole_sent` 同构——经 UDP 到达时应答同样被 UDP 发到 A 的 TCP 观察端口（会被 A 的 NAT 丢弃），同样不置 `is_udp`。标准客户端的信令连接恒为 TCP（master `client.rs:824`/`:832`/`:882` 均 `connect_tcp`；punch socket 只发 `PunchHoleSent`，不发 `LocalAddr`）⇒ 该路径对标准客户端不可达，本期不修合理。**不要把「M3 已覆盖全部 UDP 应答路径」当作结论。**
