# 第 3 轮实现评审：t11（repair-round-3）对 F2-R1/F2-R2 的修复

- 评审人：plan-reviewer（对抗式）
- 被评审任务：t11（修复 t10 第 2 轮 2 条 minor finding）
- attempt_id：`9f1d3b76-76d6-497d-834e-c325ca98f48c`
- 评审对象：`rustdesk-liusir/apps/server/src/rendezvous_server.rs`（未提交 diff）+ `rustdesk-app/tasks/feas-minimal-plan.md`
- 基线：rustdesk-liusir `HEAD=a635a9b`；master 口径参照 `/tmp/master_ref/`；fork `rustdesk-app HEAD=0454d61b4`
- 性质：只读复核，未改任何代码，未提交 commit

## 0. 结论

**verdict = pass**（0 blocker / 0 major / 0 minor）。

- t10 的 2 条 minor **均已正确修复**：F2-R1（文档行号 `client.rs:1483`→`:1468`）与 F2-R2（注释清理函数归属 `handle_tcp`→`handle_listener_inner`）。
- **可执行逻辑与第 1/2 轮逐字节一致，零逻辑变化**；5 项必查项与 7 条回归红线本轮全部独立复验通过。
- 独立复跑（非引用）：`cargo check --all-targets` exit 0、`cargo test --lib` 66 passed / 0 failed / 0 ignored、`rustfmt --check` 与 HEAD 基线同计数同位次（未新增格式债）。
- 工作区范围干净：server 仓仅 `rendezvous_server.rs` 一个 tracked 文件被修改，app 仓 tracked 文件零修改（全部任务文档为 untracked）。
- 附带如实记录：t10 的 F2-R1 要求「两处」修正，实际全文仅 1 处 `1483`（§7.2 F-R3 条从未带行号），t11 已如实上报该偏差；修正后 `1483` 全文 0 命中，语义断言未动，不影响任何结论。

---

## 1. 两条 finding 的闭环验证

### F2-R1（文档）——已修复

| 项 | 证据 |
|---|---|
| 修正位置 | `rustdesk-app/tasks/feas-minimal-plan.md:141` |
| 修正后原文 | 「…且 `punch_tcp` 要求 `ph.udp_port <= 0`（:1096）⇒ 受控端不再 `connect_tcp_local`（:1173）；只有控制器侧仍在后台 `connect_tcp_local`（master `client.rs:1468`）⇒ TCP 同时打开无法完成，只能等 `connect_timeout` 超时回落 relay。」 |
| 旧值清除 | `grep -n 'client\.rs:1483' tasks/feas-minimal-plan.md` → **0 命中**（exit 1） |
| 新值正确 | `grep -n 'client\.rs:1468'` → 命中 `:141`；`/tmp/master_ref/src/client.rs:1468` = `let fut = connect_tcp_local(peer, Some(local_addr), connect_timeout);`（fork `rustdesk-app/src/client.rs:1484`，与评审记录一致） |
| 条数偏差 | t10 写「两处」，实际全文仅 1 处（§7.2 F-R3 条在 `:257` 讲同一机制但**从未带行号**，t9 改写时即如此）⇒ t11 如实上报，修正 1 处即完整闭环 |

### F2-R2（代码注释）——已修复

| 项 | 证据 |
|---|---|
| 修正位置 | `rustdesk-liusir/apps/server/src/rendezvous_server.rs:982`（同行替换，未增删行） |
| 修正后原文 | `// A 的请求连接已断（tcp_punch 项在连接关闭时由 handle_listener_inner 清理），退回 UDP 直发` |
| 旧文清除 | `grep -c 'tcp_punch 项在 handle_tcp'` → **0 命中**；`grep -n '在 handle_tcp 连接关闭'` → 0 命中 |
| 归属正确 | `handle_listener_inner` 在 `:1593`、`handle_tcp` 在 `:574`；清理语句 `self.tcp_punch.lock().await.remove(&try_into_v4(addr));` 在 `:1657`，外围 `:1656 if sink.is_none() {`，位于 `:1651 if !self.handle_tcp(&bytes, &mut sink, addr, key, ws).await {` 之后的每连接循环尾部 ⇒ 确属 `handle_listener_inner` 而非 `handle_tcp` |
| 「何时清理」保留 | 注释仍说明「A 的请求连接已断」，与 `sink.is_none()`（sink 已被 PunchHoleRequest arm 取走）语义一致 |

---

## 2. 必查项逐条复核（本轮独立复验，全部通过）

### 2.1 UDP 到达路径 `udp_port` 恒 0（含 lan_overridden 组合）——通过

- `handle_udp_punch_hole_request` 第一条语句即 `:1286 ph.udp_port = 0;`（参数 `mut ph` 在 `:1279`），函数内此后才 `:1287` 调 `handle_punch_hole_request`。
- `handle_udp` 的 PunchHoleRequest arm（`:499-510`）两分支——`:501` 直调（`pm.is_in_memory` 命中）与 `:507` `tokio::spawn`（未命中、查库）——**都**经该函数清零，无遗漏分支。
- `handle_punch_hole_request`（定义 `:1038`）全仓仅 2 个调用点：`:1261`（TCP arm，保留 `ph.udp_port`）与 `:1287`（UDP，已清零）。
- `lan_overridden`（`:1131 = ALWAYS_USE_RELAY.load(SeqCst) || (peer_is_lan ^ is_lan)`）为真时 `:1191` 取 `{ 0 }`；同分支 `:1137 ph.nat_type = NatType::SYMMETRIC.into(); // will force relay` ⇒ **双保险**，与 `:1187-1188` 注释描述一致。
- 类型：`PunchHoleRequest.udp_port`（proto `:27`）与 `PunchHole.udp_port`（proto `:37`）均 `int32`，客户端 `master client.rs:960` 为 `udp_nat_port as _`（u16→i32）⇒ `as _` 恒等，无截断。
- 标准客户端只在 TCP 发 `PunchHoleRequest`（master `client.rs:953-969`；信令连接恒 `connect_tcp`，`:824`/`:832`/`:882`）⇒ F4 覆盖的「未鉴权 UDP 源自报 udp_port」对目标场景不可达，**零功能代价**。

### 2.2 `is_udp` 仅在 `socket.is_some()` 置位——通过

- `:960-963` 条件与 `socket.is_some()` **严格等价**；`handle_hole_sent`（定义 `:918`）全仓仅 2 个调用点：`:512`（UDP 到达，`Some(socket)`）与 `:651`（TCP 到达，`None`，`allow_err!` 包裹）。
- TCP 路径 `is_udp` 保持默认 `false`，proto3 不序列化缺省字段 ⇒ **逐位不变**。
- 失败响应路径（`:1131-1135` 同内网提前返回、`:1214-1218` peer 不存在）不置 `is_udp`；客户端 failure 分支（master `client.rs:1024-1029`）不建连接，无害。
- `is_udp` 为普通字段（proto `:112`，oneof 是 `nat_type=5`/`is_local=6`，`:106-109`），`protobuf_codegen .pure()` 只对 oneof 出 setter ⇒ **字段直接赋值是唯一可行写法**，t5 记录的 E0599 偏差属实且已注释说明。

### 2.3 M3 键匹配与一次性 remove 语义——通过

- 插入键 `:610 self.tcp_punch.lock().await.insert(try_into_v4(addr), sink)`（在 `handle_tcp` 的 `Union::PunchHoleRequest` arm 内，已复核归属）；移除键 `:974 try_into_v4(addr_a)`，`addr_a = AddrMangle::decode(&phs.socket_addr)`（`:925`）。
- `AddrMangle::encode`（`apps/server/libs/hbb_common/src/lib.rs:131-161`）先 `try_into_v4` 再编码，`decode` 为逆运算 ⇒ `decode(encode(a)) == try_into_v4(a)`；`try_into_v4`（`:117-128`）对 V4 与 loopback-V6 幂等 ⇒ 两键恒等（双栈监听成立）。
- `remove` + `tokio::spawn(async move { Self::send_to_sink(&mut sink, msg_out).await; })` 与 `send_to_tcp`（`:1228-1233`，`remove` 在 `:1229`、`spawn` 在 `:1230`）**逐行同构**；`send_to_sink`（`:1236-1240`）签名 `&mut Option<Sink>` 与调用形态匹配，内部忽略 send 错误 ⇒ 错误处理与既有行为一致。
- `sink` 缺失（键未命中，即 A 的请求连接已断）才 `socket.send(&msg_out, addr_a).await?` ——与改动前行为一致，**未削弱任何既有回程路径**；UDP 循环不再被 TCP 发送阻塞（改进）。
- M4（不写代码）：两个并发 `PunchHoleRequest` 来自不同源端口（master `client.rs:824` 每次 `_start_inner` 新建 `connect_tcp`），`tcp_punch` 以 `try_into_v4(addr)` 为键 ⇒ 两条独立键互不覆盖；`:977` 起 3 次 attempt 复用同一 socket（`:985` 同一连接重发、不再 insert）⇒ 一次性 remove 不会误消费。

### 2.4 无超出最小 diff 的附带改动——通过

- `git diff --stat` = `1 file changed, 41 insertions(+), 2 deletions(-)`；`git diff --numstat` = `41 2`，与 t9 结束时**逐字节相同** ⇒ F2-R2 同行替换未增删行、F2-R1 只改文档不改码。
- `git status --porcelain`（server 仓）：仅 ` M apps/server/src/rendezvous_server.rs`；`??` 的 `tasks/feas-m1m3-verification.md`、`tasks/feas-server-audit.md` 是 t1/t5 的既有文档产物（第 1/2 轮即在位）。
- `git status --porcelain`（app 仓）：tracked 文件**零修改**（`git diff --stat` 空），5 个任务文档全部 untracked ⇒ F2-R1 只动了 untracked 文档，未碰任何源码。
- diff 仅 3 个 hunk（`:953`+26、`:1155`→`:1181`、`:1242`→`:1276`），未夹带 M6（无任何 `nat_type` 改写）、无新配置/环境变量/日志/文件。

### 2.5 与 `feas-minimal-plan.md` 的一致性——通过

- §7.3 状态表（改动后口径）`:1191`/`:1286`/`:960-963`/`:974-984` 与当前代码逐一相符。
- 文档 `:276` 已明确声明「`:1155-1161` / `:946-952` / `:957-961` 等均为**改动前**（a635a9b）口径」，且这些基线引用经 `/tmp/base_rs_r3.rs`（`git show HEAD:` 导出）复验**全部准确**：
  - 基线 `:1155-1161` = `PunchHole` 组装 + `..Default::default()` ✓
  - 基线 `:946-952` = `PunchHoleResponse` 组装 ✓；基线 `:957-961` = 发送分支 ✓
  - 基线 `:1111` = `ph.nat_type = NatType::SYMMETRIC.into(); // will force relay` ✓；基线 `:1115-1126` = `relay_candidates` 块 ✓；基线 `handle_hole_sent` = `:918` ✓
- §4 cone–cone 行其余引用复验准确：master `rendezvous_mediator.rs:1145-1150`（`if ph.udp_port > 0 { … return Ok(()); }`）、`:1096`（`punch_tcp = !relay_v4 && ph.udp_port <= 0 && …`）、`:1173`（`connect_tcp_local`）、`client.rs:1431-1460`（`connect_timeout` 分支）、`CONNECT_TIMEOUT=18_000`（`rustdesk-app/libs/hbb_common/src/config.rs:44`）。
- §5.2/§7.2 安全断言与代码相符（UDP `PunchHoleSent` arm `:511-513` 无鉴权、`phs.socket_addr` 报文可控、有界反射；`punch_udp` 3s 内约 19 个探测包，master `common.rs:3039 MAX_TIME=3s` + `retry_interval` 20ms→200ms×1.5）。

---

## 3. 7 条回归红线逐条复核（全部通过）

diff 仅触及 4 个可执行点，红线相关区域全部未动：

| # | 红线 | 证据 |
|---|---|---|
| 1 | `ALWAYS_USE_RELAY` 行为不变 | `:1131` 读取点未动（在 diff 之前）；服务端全仓不读 `ph.force_relay`，强制中继历来由 `nat_type` 透传 + 客户端本地（master `client.rs:500` `&& !interface.is_force_relay()`）实现 |
| 2 | `same_intranet` / 同内网直连不变 | `:1150-1157` 未动；`PunchHole` 组装在 `FetchLocalAddr` 的 **else 分支**（基线 `:1171` 起）⇒ M1 天然不影响同内网路径 |
| 3 | TCP 直连路径不变 | TCP 到达分支 `:986 self.send_to_tcp(msg_out, addr_a).await;` 未动；M2 在 TCP 路径不置 `is_udp` |
| 4 | `RELAY_CANDIDATES_MAX` 默认 0 不变 | `:111` 未动；`relay_candidates` 组装未动（仅 `RELAY_CANDIDATES_MAX>0 && !lan_overridden` 时下发） |
| 5 | `MUST_LOGIN` 行为不变 | `:1032` 起的 token/jwt/过期校验未动；`ph.udp_port=0` 只收紧 UDP 路径，不放开鉴权 |
| 6 | hbbr relay 兜底不削弱 | relay 请求仍发生在 direct 失败之后（master `client.rs:1550-1574`）；M3 未改任何超时；UDP 失败回落 relay 的最坏时延不变（同分支同超时） |
| 7 | 零协议变更 / 零新配置 | 仅用既有字段 `PunchHole.udp_port=4`、`PunchHoleResponse.is_udp=9`；未改 proto、未加环境变量/CLI |

---

## 4. 独立验证命令与结果

| 命令 | 结果 |
|---|---|
| `git -C rustdesk-liusir diff --stat` / `--numstat` | `1 file changed, 41 insertions(+), 2 deletions(-)` / `41 2`，与第 2 轮逐字节相同 |
| `git -C rustdesk-liusir status --porcelain` | 仅 ` M apps/server/src/rendezvous_server.rs`；两个 `??` 为 t1/t5 既有文档产物 |
| `git -C rustdesk-app status --porcelain` + `diff --stat` | tracked 零修改；5 个任务文档全部 untracked |
| `cd rustdesk-liusir/apps/server && export PATH=$HOME/.cargo/bin:$PATH && export DATABASE_URL=sqlite:////tmp/r3_review.sqlite3 && cargo check --all-targets`（**私有 DB 副本**，规避 t11 遇到的共享 `test.sqlite3` 锁争用） | exit 0 / Finished dev profile；6 条既有告警（`:4`/`:22`/`:45` unused import、`:735`/`:1215` unused mut、`:1826` dead code），**改动行零告警** |
| 同上环境 `cargo test --lib` | exit 0；`66 passed; 0 failed; 0 ignored` |
| `rustfmt --edition 2021 --check apps/server/src/rendezvous_server.rs` | exit 1，3 处 diff：`:15`/`:40`/`:1138` |
| `git show HEAD:apps/server/src/rendezvous_server.rs > /tmp/base_rs_r3.rs` 后复跑 rustfmt | exit 1，3 处 diff：`:15`/`:40`/`:1112` |

- 格式债判定：当前 `:15`/`:40`/`:1138` 与基线 `:15`/`:40`/`:1112` 同计数、同性质（import 顺序 + `relay_candidates` if-let），`:1112 → :1138` 的 +26 位移与第一个 diff hunk（`@@ -953,9 +953,35 @@`，+26 行）完全吻合 ⇒ **未新增格式债**，F-R4 的修复保持有效。
- t11 报告的 `cargo check` 前两次 exit 101（`database is locked`，`src/database.rs:72/:97/:114/:134` sqlx 编译期宏）**确认为环境性问题**：本轮改用私有 DB 副本后 exit 0，零条错误涉及 rendezvous 代码 ⇒ 与本次改动无关，t11 的归因正确。

---

## 5. 最小性判断（明确结论）

**维持第 2 轮结论，且本轮进一步确认：已达可辩护的最小性上限。**

1. **改动面最小**：1 文件、3 个逻辑 hunk、41 insertions / 2 deletions，零协议变更、零新配置、零新文件、零日志噪声；app 仓 tracked 文件零修改。
2. **每处改动必要**：M1 补 `PunchHole.udp_port`（受控端 `ph.udp_port>0` 才走 UDP，master `rendezvous_mediator.rs:1145-1150`）；M2 置 `is_udp`（客户端 `udp_nat_port>0 && ph.is_udp && s.is_some()` 三重硬门，master `client.rs:1031`）；M3 把 UDP 应答改走 TCP sink（否则发给 A 的 TCP 观察端口、被 NAT 静默丢弃）。删掉任一处，标准客户端 UDP 直连链路即断。
3. **F4 硬化不可再省**：`ph.udp_port=0` 一条语句消除「未鉴权 UDP 源自报端口指挥已注册受控端」的攻击面；标准客户端不走该路径，零功能代价。
4. **三轮修复的可执行逻辑零变化**：第 1 轮 5 条 minor（注释腐烂 + rustfmt + 文档口径）、第 2 轮 2 条 minor（文档行号 + 注释归属）**全部只在注释文字与文档上落刀**，`git diff --numstat` 三轮恒定 `41 2` ⇒ 说明实现一次到位，后续修复未触碰行为。
5. **可回退性充分**：未提交 commit，`git revert`/`git checkout` 即整体回退；§6.4/§7.2 已记录「灰度总闸 `UDP_PUNCH_FORWARD` 未实现，回退需重启/回滚二进制」这一已知限制（第 1 轮已接受为不算偏离）。
6. **不存在更小的等价替代**：三道缺口分属不同代码路径（`PunchHole` 组装、`PunchHoleResponse` 组装、回程发送分支），且 M2/M3 共用 `socket.is_some()` 判据已是最小判据集合；曾评估的「只靠 hbbs 一个 match arm + 一个决策条件」不可行。

---

## 6. Finding 列表

**无。** 本轮 0 blocker / 0 major / 0 minor。

（记录性质、非 finding：t10 的 F2-R1 要求「两处」而实际仅 1 处，属评审方计数偏差，t11 已如实上报且修正完整；不影响任何结论。）

---

## 7. 已知限制记录（非 finding，供集成/验收阶段沿用）

1. `handle_local_addr`（`:989-1009`）经 UDP 到达时存在与 M3 修复前同构的回程缺陷，但标准客户端信令恒 TCP（master `client.rs:824`/`:832`/`:882`）⇒ `handle_udp` 的 LocalAddr arm（`:514-516`）对标准客户端不可达，本期不修。
2. UDP `PunchHoleSent` arm（`:511-513`）无鉴权/无注册校验/无速率限制，`phs.socket_addr` 报文可控 ⇒ 有界反射 + `udp.0.take()` 抢占。已写入 §7.2；可选硬化（校验 UDP 源 IP 与 peer 注册 IP 一致）有误杀 NAT 重绑定/多公网 IP 场景的风险，另案评估。
3. `UDP_PUNCH_FORWARD` 总闸未实现，回退路径为二进制回滚（§6.4/§7.2 已记录）。
4. `punch_tcp → punch_udp` 切换边界：`!relay_v4 && ph.udp_port > 0`（master `rendezvous_mediator.rs:1095`）；B 侧封 UDP 放 TCP 时 TCP punch 变 relay、A 侧封 UDP 时自愈。定性为与官方服务端能力对齐的固有权衡。
5. 验收客户端必须 ≥1.4.0（1.1.16 一代结构上无 UDP 打洞）；`punch_stats:`/`punch_req:` 为定制客户端埋点，官方构建不可用 ⇒ 判据靠 M5 lite（未实现）替代方案：抓包 + `af0c356` 既有应答日志 + hbbr 连接数反推。

---

## 8. 放行建议

代码侧**无任何遗留**，5 项必查项 + 7 条回归红线 + 独立编译/测试/格式复验全部通过，最小性已达上限。建议：

1. 由 captain 决定提交时机（当前为未提交工作区 diff，`1 file / 41 ins / 2 del`）。
2. 集成/验收按 §7 的 5 条已知限制执行，特别是「灰度期无法原位关断」需写入验收记录。
3. 若要彻底消除第 2 轮暴露的「文档行号口径」类问题，可在提交时把 §7.3 的改动后口径表与文档 `:276` 的基线口径声明一并保留（现已齐备），无需额外改动。
