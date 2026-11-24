# 第 2 轮实现评审：t9（repair-round-2）对 M1–M3 的修复

- 评审人：plan-reviewer（对抗式）
- 被评审任务：t9（修复 t7 第 1 轮 5 条 minor finding）
- attempt_id：`68711b37-5bf5-4d8c-8829-3de18edac1c2`
- 评审对象：`rustdesk-liusir/apps/server/src/rendezvous_server.rs`（未提交 diff）+ `rustdesk-app/tasks/feas-minimal-plan.md`
- 基线：rustdesk-liusir `HEAD=a635a9b`；master 口径参照 `/tmp/master_ref/`（client.rs 5911 行 / common.rs / rendezvous_mediator.rs 1811 行）；fork `rustdesk-app HEAD=0454d61b4`
- 性质：只读复核，未改任何代码，未提交 commit

## 0. 结论

**verdict = needs_revision**（0 blocker / 0 major / 2 minor，全部为「文字引用」类，**可执行代码零 finding**）。

- 三处改动（M1 / M2 / M3）与 `feas-minimal-plan.md` 规格、t7 F4/F1 修正、7 条回归红线**逐条一致**，5 项必查项全部通过。
- t9 对 t7 五条 minor 的修复**全部闭环**，其中 F-R1（注释行号腐烂）与 F-R4（rustfmt）在代码侧已彻底解决，本轮独立复跑 `cargo fmt --check` 确认**未新增格式债**。
- 残留 2 条 minor 均为 t9 本轮**新引入或新暴露**的文字引用问题：一处文档行号引用错误（`master client.rs:1483` 应为 `:1468`），一处注释把清理函数归属写错（`handle_tcp` 应为 `handle_listener_inner`）。二者都是**一 token 级文字修正，不涉及任何逻辑改动，也不需要重新验证编译/测试**。
- 由于契约规定「verdict=pass 仅当全部必查项无 major/minor 遗留」，且第 1 轮已确立「引用不解析/引用错误属 minor、需修复」的先例，本轮如实判 needs_revision。若调度方认为这两条文字修正不值得单独一轮，可由 captain 直接落改后按 §3 的复验清单抽验即可放行集成。

---

## 1. 评审范围与「可执行逻辑零变化」的证实

`git diff --stat`：`1 file changed, 41 insertions(+), 2 deletions(-)`——与第 1 轮（t5 结束时）**完全一致**。t9 的 F-R1（4 处注释锚点改写）净增行数与 F-R4（`udp_port` 三行收为一行，-2 行）相互抵消，故总量不变。

可执行语句逐行比对第 1 轮记录，**完全一致，无任何逻辑差异**：

| 改动 | 当前位置 | 可执行语句 |
|---|---|---|
| M2 | `:960-963` | `if socket.is_some() { p.is_udp = true; }`（置于 `msg_out.set_punch_hole_response(p)` 之前） |
| M3 | `:971-984` | `let mut sink = self.tcp_punch.lock().await.remove(&try_into_v4(addr_a));` → `if sink.is_some()` → `tokio::spawn(async move { Self::send_to_sink(&mut sink, msg_out).await; })` → `else` → `socket.send(&msg_out, addr_a).await?;` |
| M1 | `:1191` | `udp_port: if lan_overridden { 0 } else { ph.udp_port as _ },`（单行，rustfmt 规范） |
| F4 硬化 | `:1279` / `:1286` | `mut ph: PunchHoleRequest,` / `ph.udp_port = 0;`（函数第一条语句） |

TCP 到达分支 `:986` 仍是 `self.send_to_tcp(msg_out, addr_a).await;`，`send_to_tcp` 签名及其两个调用方（`:983`、`:1250`）未动，`handle_local_addr`（`:989-1009`）整体未动。**未夹带 M6、未新增配置项/日志/文件。**

---

## 2. 必查项逐条复核

### 2.1 必查项 1：UDP 到达路径 `udp_port` 是否真的恒 0（含 lan_overridden 组合）——通过

- `handle_udp_punch_hole_request` **第一条语句**即 `:1286 ph.udp_port = 0;`（参数已改 `mut ph`），函数内此后才进入 `handle_punch_hole_request`（`:1287`）。
- `handle_udp` 的 PunchHoleRequest arm（`:499-510`）两个分支——`:501` 直调与 `:507` `tokio::spawn`——**都**进该函数 ⇒ 两条 UDP 入口全覆盖，无遗漏分支。
- `handle_punch_hole_request` 全仓仅 2 个调用点：`:1261`（TCP，保留 `ph.udp_port`）与 `:1287`（UDP，已清零）。
- `lan_overridden`（`:1131 = ALWAYS_USE_RELAY || (peer_is_lan ^ is_lan)`）为真时 `:1191` 走 `{ 0 }` 分支；同分支 `:1137` 已强制 `ph.nat_type = NatType::SYMMETRIC.into(); // will force relay` ⇒ **双保险**，与 `:1187-1188` 注释描述一致。
- 类型安全：proto 两侧 `PunchHoleRequest.udp_port = 7`（`apps/server/libs/hbb_common/protos/rendezvous.proto:27`）与 `PunchHole.udp_port = 4`（同文件 `:37`）均为 `int32`，客户端 `client.rs:960` 为 `udp_nat_port as _`（u16→i32 恒正）⇒ `as _` 是 i32→i32 恒等，无截断。
- 标准客户端只在 TCP 发 `PunchHoleRequest`（master `client.rs:953-969`，信令连接恒 `connect_tcp`，见 `:824/:832/:882`）⇒ F4 覆盖的「未鉴权 UDP 源自报 udp_port」场景对标准客户端不可达，**目标场景零影响**。

### 2.2 必查项 2：`is_udp` 是否只在 `socket.is_some()` 置位——通过

- `:960-963` 条件与 `socket.is_some()` **严格等价**；`handle_hole_sent` 全仓仅 2 个调用点：`:512`（UDP 到达，`Some(socket)`）与 `:651`（TCP 到达，`None`，`allow_err!` 包裹）。
- TCP 路径 `is_udp` 保持默认 `false`，proto3 不序列化缺省字段 ⇒ **逐位不变**（红线 4）。
- 失败响应路径（`:1131-1135` 同内网提前返回、`:1214-1218` peer 不存在）不置 `is_udp`；客户端 failure 分支（master `client.rs:1024-1029`）不建连接，无害。
- 代码生成形态复核：`protobuf_codegen .pure()` 只为 oneof 字段产出 setter，`PunchHoleResponse.is_udp=9`（proto `:112`，oneof 为 `nat_type=5`/`is_local=6` `:106-109`）是普通字段，生成代码仅 `pub is_udp: bool`（`target/debug/build/hbb_common-*/out/protos/rendezvous.rs:1763`）⇒ **字段直接赋值是唯一可行写法**，t9 记录的 `set_is_udp` E0599 偏差属实且已注释说明。

### 2.3 必查项 3：M3 键匹配与一次性 remove 语义——通过

- 插入键 `:610` `self.tcp_punch.lock().await.insert(try_into_v4(addr), sink)`（`handle_tcp` 的 `Union::PunchHoleRequest` arm 内，已复核归属）；移除键 `:974` `try_into_v4(addr_a)`，其中 `addr_a = AddrMangle::decode(phs.socket_addr)`（`:948`）。
- `AddrMangle::encode`（`apps/server/libs/hbb_common/src/lib.rs:131-161`）先 `try_into_v4` 再编码，`decode` 为其逆运算 ⇒ `decode(encode(a)) == try_into_v4(a)`；`try_into_v4`（同文件 `:117-128`）对 V4 与 loopback-V6（`::ffff:a.b.c.d`）幂等 ⇒ 两键**恒等**（双栈监听场景成立）。
- 一次性 `remove` + `tokio::spawn(async move { Self::send_to_sink(&mut sink, msg_out).await; })` 与 `send_to_tcp`（`:1229-1232`）**逐行同构**：锁 guard 仅在 remove 语句内存活（无死锁）、owned `Option<Sink>` 移入满足 `'static`、错误在 `send_to_sink`（`:1236-1240`）内被忽略（与既有行为一致）。UDP 循环不再被 TCP 发送阻塞，属改进。
- `sink` 缺失（键未命中）才 `socket.send(&msg_out, addr_a).await?` ——与改动前行为一致，**未削弱任何既有回程路径**。
- M4（不写代码）：两个并发 `PunchHoleRequest` 来自**不同源端口**（master `client.rs:824` 每次 `_start_inner` 新建 `connect_tcp`），`tcp_punch` 以 `try_into_v4(addr)` 为键 ⇒ **两条独立键，互不覆盖**；`client.rs:977` 起 3 次 attempt 复用同一 socket（`:985` 同一连接重发，不再 insert）⇒ 一次性 remove 不会误消费后续 attempt 的 sink。实现未引入任何按 id 的键假设。

### 2.4 必查项 4：是否有超出最小 diff 的附带改动——通过

- `1 file / 41 insertions / 2 deletions`，无第二文件、无新配置/环境变量/日志/文档随码提交。
- `grep` 复核：diff 内无 `nat_type` 改写逻辑（M6 未夹带）、无 `ALWAYS_USE_RELAY`/`RELAY_CANDIDATES_MAX` 触碰、无 `check_ip_blocker` 调用点变更。
- 独立复跑格式检查：`rustfmt --edition 2021 --check` 对当前文件 = **恰好 3 处 diff**（`:15`/`:40` import 顺序、`:1138` relay_candidates if-let），与 HEAD 基线（`git show HEAD:apps/server/src/rendezvous_server.rs` 导出至 `/tmp/base_rs_r2.rs` 后复跑）**计数与位置完全一致**，且 `udp_port` 行不出现在任何 diff hunk 中 ⇒ F-R4 已解决，**未新增格式债**（未跑全文件 `cargo fmt`，遵守最小 diff）。

### 2.5 必查项 5：实现与 `feas-minimal-plan.md` 表述是否冲突——基本通过，残留 1 处引用错误

新增/改写的文档断言逐条对照代码，**除 F2-R1 外全部属实**：

| 文档断言 | 代码证据 | 判定 |
|---|---|---|
| §4 cone–cone：`connect_timeout = CONNECT_TIMEOUT = 18s`（首次，`client.rs:1431-1460`），`direct_failures>0` 时退化为 `punch_time_used*6` | master `client.rs:1431-1460`；`CONNECT_TIMEOUT=18_000`（`rustdesk-app/libs/hbb_common/src/config.rs:44`） | ✅ |
| §4/§7.2：受控端 `ph.udp_port>0` 提前 return、不再 `connect_tcp_local` | master `rendezvous_mediator.rs:1145-1150`；`punch_tcp` 要求 `<=0`（`:1096`）；`connect_tcp_local` 在 `:1173` | ✅ |
| §4/§7.2：切换条件 `!relay_v4 && ph.udp_port>0`；A 封 UDP 时 TestNatRequest 失败使 `udp_port=0` 自愈 | master `rendezvous_mediator.rs:1091-1094`；`client.rs:5480-5549` `test_udp_uat` 仅在收到响应时写 `udp_port` | ✅ |
| §7.2 M3 边界：3 次 attempt 只有第 1 次 insert sink，`:610`；`client.rs:985` 起复用同一 socket | `:610` 在 `handle_tcp` PunchHoleRequest arm；master `client.rs:977`/`:985`/`:987` | ✅ |
| §5.2/§7.2 F-R2：UDP `PunchHoleSent` arm 无鉴权/无注册校验/无速率限制，`phs.socket_addr` 报文可控 ⇒ 有界反射 + `udp.0.take()` 抢占 | `handle_udp` `:511-513` → `handle_hole_sent`；`get_pk` 未命中仅返回空 `Bytes`、不拒收 | ✅ |
| §5.2 F-R2a：`punch_udp` 3s、约 20+ 个探测包 | master `common.rs:3039 MAX_TIME=3s`；`retry_interval` 20ms→200ms(×1.5)，实测 3s 内发 19 个包 | ✅ |
| §7.3 状态表行号 `:1191`/`:960-963`/`:974-984`/`:1286` | 与当前代码逐一相符 | ✅ |
| §7.2：「控制器后台 `connect_tcp_local`（master `client.rs:1483`）」 | master_ref `client.rs` 中该调用在 **`:1468`**（fork `rustdesk-app/src/client.rs:1484`） | ❌ **F2-R1** |

文档口径与实现形状的一致性亦已核对：t9 按第 1 轮 F-R3 要求把 §3.3/§4 的「先试 send_to_tcp，取不到再退回 UDP」改为实现形状（内联 `remove` + `spawn` + UDP 回退），并说明 `send_to_tcp`（`:1229-1232`）无返回值、sink 缺失时静默丢弃、无法表达「取不到再退回」——**文档现口径与代码一致**，方向为「改文档」而非「改代码」，正确。

---

## 3. 7 条回归红线逐条复核（全部通过）

| # | 红线 | 证据 |
|---|---|---|
| 1 | `ALWAYS_USE_RELAY` 行为不变 | 服务端全仓不读 `ph.force_relay`；强制中继历来由 `nat_type` 透传 + 客户端本地（master `client.rs:500` `&& !interface.is_force_relay()`）实现；diff 未触及 `nat_type` 赋值路径 |
| 2 | `same_intranet` / 同内网直连不变 | `:1150-1157` 未动；PunchHole 组装在 `FetchLocalAddr` 的 **else 分支** `:1171-1196` ⇒ M1 天然不影响同内网路径 |
| 3 | TCP 直连路径不变 | TCP 到达分支 `:986` 仍 `send_to_tcp`；M2 在 TCP 路径不置 `is_udp`（proto3 不序列化） |
| 4 | `RELAY_CANDIDATES_MAX` 默认 0 不变 | `:111` 未动；`relay_candidates` 组装 `:1140` 未动（仅 `RELAY_CANDIDATES_MAX>0 && !lan_overridden` 时下发） |
| 5 | `MUST_LOGIN` 行为不变 | `:1032` 起的 token/jwt/过期校验未动；`handle_udp_punch_hole_request` 的 `ph.udp_port=0` 只收紧 UDP 路径，不放开鉴权 |
| 6 | hbbr relay 兜底不削弱 | relay 请求仍发生在 direct 失败之后（master `client.rs:1550-1574`）；M3 未改任何超时；UDP 失败回落 relay 的最坏时延不变（同分支同超时，机理已按 F-R3 更正为「等 connect_timeout」而非「TCP punch 仍会跑」） |
| 7 | 零协议变更 / 零新配置 | 仅用既有字段 `PunchHole.udp_port=4`、`PunchHoleResponse.is_udp=9`；未改 proto、未加环境变量/CLI |

---

## 4. t9 对 t7 五条 finding 的闭环确认

| t7 finding | 严重度 | t9 处置 | 本轮判定 |
|---|---|---|---|
| F-R1 注释行号腐烂（4 处） | minor | 改为「函数名 + 代码片段」定位 | ✅ 已解决：`:779` `check_ip_blocker`、`:1137` `ph.nat_type = NatType::SYMMETRIC.into(); // will force relay`、`:610` `tcp_punch.insert(try_into_v4(addr), sink)`（确在 `handle_tcp` PunchHoleRequest arm）、`:1229` `send_to_tcp` 内 `remove`、`:1657` tcp_punch 清理——**5 个锚点全部解析且归属正确** |
| F-R2 §5.2 安全断言不准 | minor | 两条断言重写为实现后口径 | ✅ 已解决：hbbs 自身对外 UDP 包数/放大比不变、真实代价是客户端侧有界 UDP 触达；「不存在未注册来源直接触发回包的新路径」的不成立已承认，前置条件与定性写入 §7.2 |
| F-R3 §4「UDP 失败后 TCP punch 仍会跑」不准 | minor | 机理更正为「受控端提前 return，仅控制器后台 connect」 | ✅ 已解决（引用除 F2-R1 外全部准确） |
| F-R4 `udp_port` 多行不符 rustfmt | minor | 收为单行 `:1191` | ✅ 已解决（与 HEAD 基线格式 diff 计数一致，未新增格式债） |
| F-R5 `handle_local_addr` UDP 路径同构缺陷 | minor（仅记录） | 未修（标准客户端不可达） | ✅ 维持「记录不修」：master `client.rs:824/:832/:882` 信令恒 TCP，punch socket 只发 `PunchHoleSent` ⇒ `handle_udp` 的 LocalAddr arm（`:514-516`）对标准客户端不可达 |

---

## 5. 独立验证命令与结果

| 命令（`apps/server`，`PATH=$HOME/.cargo/bin:$PATH`，`DATABASE_URL=sqlite://./test.sqlite3`） | 结果 |
|---|---|
| `git diff --stat` | `1 file changed, 41 insertions(+), 2 deletions(-)` |
| `cargo check --all-targets` | exit 0；6 条既有告警（`:4/:22/:45` unused import、`:735/:1215` unused mut、`:1826` dead code），**改动行零告警** |
| `cargo test --lib` | exit 0，`66 passed; 0 failed` |
| `rustfmt --edition 2021 --check apps/server/src/rendezvous_server.rs` | 恰好 3 处 diff（`:15`/`:40`/`:1138`），与 HEAD 基线逐位置一致 |
| 上述命令与 t9 报告的口径**完全一致**（非引用，系本轮亲自复跑） |

---

## 6. Finding 列表

### F2-R1（minor，文档）`feas-minimal-plan.md` 引用错误的 master 行号
- 位置：`rustdesk-app/tasks/feas-minimal-plan.md` §4 cone–cone 行与 §7.2 F-R3 条：「只有控制器侧仍在后台 `connect_tcp_local`（master `client.rs:1483`）」
- 问题：master 口径（`/tmp/master_ref/src/client.rs`）中 `connect_tcp_local(peer, Some(local_addr), connect_timeout)` 在 **`:1468`**；`:1483` 既不是 master 行号，也不是 fork 行号（fork `rustdesk-app/src/client.rs` 为 `:1484`）。文档其余 master 引用（`:977`/`:985`/`:1030`/`:1031`/`:1431-1460`/`:1461`）全部与 master_ref 精确相符，故可判定为笔误而非口径混用。
- 证据：`/tmp/master_ref/src/client.rs:1468`（`let fut = connect_tcp_local(peer, Some(local_addr), connect_timeout);`）；`rustdesk-app/src/client.rs:1484`
- 要求的最小修正：把两处 `client.rs:1483` 改为 `client.rs:1468`。**不涉及代码，不影响任何结论**（该引用的语义断言「只有控制器侧仍在后台 connect」本身正确）。

### F2-R2（minor，代码注释措辞）tcp_punch 清理函数归属写错
- 位置：`rustdesk-liusir/apps/server/src/rendezvous_server.rs:982` 注释「`tcp_punch` 项在 `handle_tcp` 连接关闭时清理」
- 问题：清理动作（`:1657` `self.tcp_punch.lock().await.remove(&try_into_v4(addr));`）位于 `handle_listener_inner`（`:1593`，即驱动 `handle_tcp` 的每连接循环），而非 `handle_tcp` 自身。注释对「何时清理」（连接关闭、`sink.is_none()`）的描述正确，对「哪个函数清理」归属不精确。
- 证据：`rendezvous_server.rs:1593`（`async fn handle_listener_inner`）、`:1651`（`if !self.handle_tcp(...)`）、`:1656-1658`（清理）；`handle_tcp` 定义在 `:574`
- 要求的最小修正：把该句改为「`tcp_punch` 项在连接关闭时由 `handle_listener_inner` 清理」（或去掉函数名只留「连接关闭时清理」）。纯注释文字，不涉及逻辑。

> 说明：F2-R1/F2-R2 与第 1 轮 F-R1 属同一「引用准确性」类别；本轮如实按 minor 记录，**不夸大**：二者都不改变任何行为结论，修正后无需重跑编译/测试，只需按 §2.5 的引用清单抽验两个行号。

---

## 7. 最小性判断（明确结论）

**当前实现已达到本评审可辩护的最小性上限：**

1. **改动面最小**：1 文件、3 处逻辑 hunk（M1/M2/F4 各 1、M3 1）+ 必要的注释，41 insertions / 2 deletions，零协议变更、零新配置、零新文件、零日志噪声。
2. **每处改动均必要**：M1 补 `PunchHole.udp_port`（受控端 `ph.udp_port>0` 才走 UDP，master `rendezvous_mediator.rs:1145-1150`）；M2 置 `is_udp`（客户端 `udp_nat_port>0 && ph.is_udp && s.is_some()` 三重硬门，master `client.rs:1031`）；M3 把 UDP 应答改走 TCP sink（否则发给 A 的 TCP 观察端口、被 NAT 静默丢弃）。删掉任一处，标准客户端 UDP 直连链路即断。
3. **F4 硬化不可再省**：`ph.udp_port=0` 是 1 行语句，消除「未鉴权 UDP 源自报端口指挥已注册受控端」的攻击面；标准客户端不走该路径，故零功能代价。
4. **可回退性充分**：未提交 commit，`git revert`/`git checkout` 即整体回退；§6.4 已记录「总闸 `UDP_PUNCH_FORWARD` 未实现，灰度期需重启/回滚二进制」这一已知限制（第 1 轮已接受为不算偏离）。
5. **不存在更小的等价替代**：曾评估「只靠 hbbs 一个 match arm + 一个决策条件」——不可行，因为三道缺口分属不同代码路径（PunchHole 组装 `:1191`、PunchHoleResponse 组装 `:960-963`、回程发送分支 `:965-984`），且 M2/M3 共用 `socket.is_some()` 判据已是最小判据集合。
6. **未做的事同样是最小性的一部分**：M4 不写代码（键结构天然支持）、M6 未夹带（nat_type 改写默认关且未实现）、`handle_local_addr` 不修（标准客户端不可达）、全文件 `cargo fmt` 不跑（避免无关 diff）。

---

## 8. 已知限制记录（非 finding，供集成阶段沿用）

1. `handle_local_addr`（`:989-1009`）经 UDP 到达时存在与 M3 修复前同构的回程缺陷，但标准客户端信令恒 TCP ⇒ 不可达，本期不修。
2. UDP `PunchHoleSent` arm（`:511-513`）无鉴权、无注册校验、无速率限制；`phs.socket_addr` 报文可控 ⇒ 有界反射 + `udp.0.take()` 抢占。已写入 §7.2 作为已知限制；可选硬化（校验 UDP 源 IP 与 peer 注册 IP 一致）有误杀 NAT 重绑定/多公网 IP 场景的风险，建议后续单独评估，本期不做。
3. `UDP_PUNCH_FORWARD` 总闸未实现，回退路径为二进制回滚（§6.4 已记录）。
4. 行为边界：受控端 `punch_tcp → punch_udp` 的切换条件是 `!relay_v4 && ph.udp_port > 0`（即 A 自报非对称 + B 本地判定非对称 + A 的 UDP NAT 测试成功）；B 侧封 UDP 放 TCP 时，原可能成功的 TCP punch 会变为 relay（A 侧封 UDP 时因 TestNatRequest 失败使 `udp_port=0` 而自愈）。此权衡与官方服务端能力对齐，§7.2 已记录。

---

## 9. 复验清单（供修复方/调度方抽验，不需要重新编译）

1. `grep -n 'connect_tcp_local(peer, Some(local_addr), connect_timeout)' /tmp/master_ref/src/client.rs` ⇒ 应为 `:1468`；`feas-minimal-plan.md` 中两处 `client.rs:1483` 均已改为 `:1468`。
2. `sed -n '982p' rustdesk-liusir/apps/server/src/rendezvous_server.rs` ⇒ 注释不再把清理归属写成 `handle_tcp`。
3. 以上两处均为纯文字，`git diff --stat` 应仍为 `1 file / 41 insertions(+), 2 deletions(-)`（F2-R1 改文档不改码，F2-R2 同行替换）。
