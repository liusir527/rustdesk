# t7 第 1 轮实现评审：M1-M3 diff（对抗式）

- 评审人：plan-reviewer（t4 方案评审人，本方案 M1-M3 规格与 F1/F2/F4 修正的提出者）
- 被评审对象：t5 的 M1/M2/M3 实现（rustdesk-liusir 工作区**未提交** diff）
- 工作区：`/Volumes/USB-DISK/rust-desk/rustdesk-liusir`（HEAD `a635a9b`，工作区仅 `apps/server/src/rendezvous_server.rs` 变更）
- diff 规模：`1 file changed, 41 insertions(+), 2 deletions(-)`，3 个 hunk（`handle_hole_sent` 2 个 + `handle_punch_hole_request` 1 个 + `handle_udp_punch_hole_request` 1 个）；t5 报告写 40 insertions，与实际差 1，属报告笔误，不影响结论
- 评审方式：只读代码复核 + 独立复跑编译/测试 + 协议字段号与生成代码交叉核对；**未修改任何代码**

---

## 1. 总体结论

**verdict = needs_revision**（0 blocker / 0 major / 5 minor）

三处改动在功能上**忠实落地**了 `feas-minimal-plan.md` 的 M1-M3 规格与评审 F4/F1 修正：未发现任何逻辑缺陷、协议不兼容、键匹配错误、死锁/错误处理回归或回归红线破坏。**5 个 finding 全部是文档/注释/风格级**，无一需要重新设计或改变实现形状；其中 2 条要求改代码（F-R1 注释行号、F-R4 rustfmt），3 条要求改方案文档表述（F-R2/F-R3/F-R5）。

- 必查项 5/5 通过（§2）
- 七条回归红线 7/7 通过（§3）
- 独立复跑 `cargo check --all-targets` exit 0、`cargo test --lib` 66 passed / 0 failed（§6）
- 最小性判断：三处改动是最小可工集，不存在更小的等价替代路径（§5）

放行条件见 §7：处理 F-R1 + F-R4（代码）与 F-R2/F-R3（文档）后，第 2 轮可直接 pass；F-R5 仅作记录不需改动。

---

## 2. 必查项逐条结论

### 2.1 M1（PunchHole.udp_port 回填）— **通过**

| 检查点 | 结论 | 证据 |
|---|---|---|
| 回填值来源 | 控制器自报的 punch socket 公网端口，服务端只转发不推断 | `rendezvous_server.rs:1187-1191` `udp_port: if lan_overridden { 0 } else { ph.udp_port as _ }`；客户端侧 `client.rs:960`（master）`udp_port: udp_nat_port as _`，`udp_nat_port` 来自 `test_udp_uat` 的 `TestNatResponse.port`（`client.rs:5545`） |
| 类型安全 | i32→i32 恒等，无截断/无符号反转 | 两侧 proto `PunchHoleRequest.udp_port=7` / `PunchHole.udp_port=4` 均为 int32（server `libs/hbb_common/protos/rendezvous.proto:27/:37`，app `rustdesk-app/libs/hbb_common/protos/rendezvous.proto:38/:77`，**字段号一致**） |
| UDP 路径恒 0 覆盖全部分支 | 完整覆盖，无遗漏 | `handle_udp_punch_hole_request`（:1276）第一条语句即 `ph.udp_port = 0;`（:1286，参数已改 `mut ph` :1279）；`handle_punch_hole_request` 全仓仅 2 个调用点（:1261 TCP、:1287 UDP）；UDP arm（:499-510）的**两个**分支——:501 同步调用与 :507 spawn——都进入同一函数，都在 :1286 清零 |
| lan_overridden 时恒 0 | 是，且与 nat_type 双保险 | `lan_overridden` 定义 :1128（`ALWAYS_USE_RELOAD \|\| (peer_is_lan ^ is_lan)`）在 :1187 作用域内；:1134 同时强制 `ph.nat_type = SYMMETRIC` |
| 无「强制中继却带 udp_port」实际组合 | 客户端侧兜底成立 | 受控端 `punch_udp = !relay_v4 && ph.udp_port > 0`（master `rendezvous_mediator.rs:1095`），`relay_v4` 含 `ph.nat_type == SYMMETRIC`（:1091）⇒ 矛盾组合下 UDP punch 也不触发 |
| 关闭 UDP 打洞时无新增触达 | 死代码，逐位一致 | 无 punch socket ⇒ `udp_nat_port=0` ⇒ `ph.udp_port=0` ⇒ 受控端走 `punch_tcp`（master `rendezvous_mediator.rs:1096`），与改动前一致（红线 7） |

### 2.2 M2（PunchHoleResponse.is_udp 置位）— **通过**

| 检查点 | 结论 | 证据 |
|---|---|---|
| 触发条件严格等于 `socket.is_some()` | 是 | `:960-963` `if socket.is_some() { p.is_udp = true; }`，置于 `:964 msg_out.set_punch_hole_response(p)` 之前；`handle_hole_sent` 全仓仅 2 个调用点：:512（UDP arm，`Some(socket)`）与 :651（TCP arm，`None`，`allow_err!` 包裹）⇒ 置位 ⟺ PunchHoleSent 经 UDP 到达 |
| TCP 路径 PunchHoleResponse 逐位不变 | 是 | `p.is_udp` 默认 false 且 proto3 不序列化默认值 ⇒ 线上字节不变；else 分支 `:982-984` 仍是原 `self.send_to_tcp(msg_out, addr_a).await`，一行未动 |
| 与 oneof 无冲突 / 写法正确 | 是 | proto 中 `nat_type=5` 与 `is_local=6` 同处 oneof（server `rendezvous.proto:106-109`），`is_udp=9` 是独立 bool（:112）；生成代码 `pub is_udp: bool`（`target/debug/build/hbb_common-*/out/protos/rendezvous.rs:1763`）为普通字段、无 setter ⇒ t5 的「直接赋字段」偏差说明属实，且是唯一可行写法 |
| failure 响应不受影响 | 是 | failure 响应在 `handle_punch_hole_request` 内提前 return（:1118-1122 OFFLINE / :1201-1205 ID_NOT_EXIST），不经 `handle_hole_sent`；客户端 failure 分支不连接 UDP |

### 2.3 M3（响应优先 TCP sink）— **通过**

| 检查点 | 结论 | 证据 |
|---|---|---|
| tcp_punch 键匹配 | 恒等 | insert 键 :610 `try_into_v4(addr)`（A 的 PunchHoleRequest TCP 连接源址）；remove 键 :972 `try_into_v4(addr_a)`，`addr_a = AddrMangle::decode(phs.socket_addr)`，而 `phs.socket_addr` 是受控端回显的 hbbs 下发值 `AddrMangle::encode(A 的 TCP 源址)`（:1158）⇒ `decode(encode(a)) == try_into_v4(a)`，且 `try_into_v4` 对 V4 / v4-mapped-V6 幂等（`libs/hbb_common/src/lib.rs:117-161`）⇒ V4 与 V6 双栈下两键均恒等 |
| 一次性 remove 语义 | 与既有完全同构 | `:972-977` 与 `send_to_tcp`（:1228-1233）逐行同构：`remove` + `spawn(Self::send_to_sink)`；锁 guard 只在 remove 语句内存活，无跨 await 持锁、无死锁面；`send_to_sink`（:1236-1240）内部忽略 send 错误 ⇒ 错误处理与既有行为一致 |
| sink 缺失回退 | 保留且与原行为一致 | `:978-981` sink 缺失时退回 `socket.send(&msg_out, addr_a).await?`；sink 缺失的成因（A 的连接已断、项被 :1656-1658 清理，或该键从未注册）在注释中说明 |
| 未误改周边 | 是 | `send_to_tcp` 签名及其两个调用方（:983、:1250）未动；`handle_local_addr`（:989-1009）完全未动，diff 无该函数 |
| **前提复核：M2/M3 在活路径上** | 是，非死代码 | 受控端 `punch_udp_hole` 用 `new_direct_udp_for(&self.host)` 新建 UDP socket 发 PunchHoleSent（master `rendezvous_mediator.rs:1206-1210`）⇒ 确实经 hbbs UDP arm（:511-513）到达。且该 UDP 源址正是受控端 punch socket 的公网映射，是控制器建 UDP future 所需的**正确对端地址**（`p.socket_addr = AddrMangle::encode(addr)`，:947）——UDP 到达不只是传输通道问题，更是对端地址正确性问题，M2/M3 的前提成立 |

### 2.4 最小 diff — **通过**（1 条 minor）

仅 1 文件 3 hunk，无附带改动、无新配置项、无新日志、无新文件、无 `git commit`。未夹带 M6（diff 无任何 nat_type 改写逻辑）。行号口径与格式问题见 F-R1/F-R4。

### 2.5 一致性（实现 vs 方案文档）— **通过**（3 条 minor，均为文档侧）

`§7.3` 状态表已按实现形状更新且与代码一致（M1 `:1181-1190` + `:1282-1286`、M2 `:956-961`、M3 `:963-978`；实际精确定位：M1 `:1187-1191`/`:1279`/`:1286`、M2 `:956-963`、M3 `:965-984`）。但 `§3.3` 的字面规格与实现形状不同（见 F-R3 同类问题，另见 §5 最小性），`§4` 矩阵与 `§5.2` 安全断言与实现后行为不符（F-R2/F-R3）。

---

## 3. 七条回归红线逐条对照（方案 §5.4）

| # | 红线 | 结论 | 证据 |
|---|---|---|---|
| 1 | `ALWAYS_USE_RELAY=Y` / `force_relay` / LAN 跨网 → 仍强制 relay 且 PunchHole 不诱导 UDP punch | **通过** | `lan_overridden` ⇒ `udp_port=0`（:1187-1189）+ `nat_type=SYMMETRIC`（:1134，未改）。另：`force_relay` 时控制器根本不建 punch socket（master `client.rs:500` `&& !interface.is_force_relay()`）⇒ `udp_nat_port=0` 双保险。附注：服务端从不读 `ph.force_relay`（全仓无匹配），强制中继历来由 nat_type 透传 + 客户端本地实现，本 diff 未触及该机制 |
| 2 | 自报/本地 SYMMETRIC → `relay_v4` 秒切中继 | **通过** | `relay_v4` 含 `ph.nat_type == SYMMETRIC`（master `rendezvous_mediator.rs:1091`）与受控端本地 `Config::get_nat_type()`（:1092）；该场景下 `punch_tcp`/`punch_udp` 均要求 `!relay_v4`（:1095-1096）⇒ 受控端直接 `create_relay`，改动前后一致 |
| 3 | `same_intranet` → 仍走 `FetchLocalAddr` | **通过** | 判定 :1150-1157，:1159-1170 走 `set_fetch_local_addr`；PunchHole 组装在 **else 分支**（:1171-1196）⇒ M1 天然不影响同内网路径（方案 §4 该行结论正确） |
| 4 | TCP `PunchHoleResponse` → 仍走 `send_to_tcp`，`is_udp` 不置位 | **通过** | :982-984 未动；`p.is_udp` 默认 false 不序列化 |
| 5 | `RELAY_CANDIDATES=0` → 不下发 `relay_servers` | **通过** | :1138-1149 未动；`relay_servers: relay_candidates.into()`（:1193）仍为空 |
| 6 | af0c356 的 UDP `TestNatResponse` 内容不变 | **通过** | :521-535 未动（`port = addr.port()` + `serial > tar.serial` 时附 `cu`） |
| 7 | 客户端 UDP 打洞关闭 → 整个 UDP 路径不被触达 | **通过** | 无 punch socket ⇒ `udp_nat_port=0` ⇒ `ph.udp_port=0` ⇒ 受控端 `punch_tcp`、不发 UDP `PunchHoleSent` ⇒ M2/M3 为死代码 |

---

## 4. Finding 列表

### F-R1（minor，需改代码）注释行号口径混用，4 处不解析

- 问题：`handle_hole_sent`/`handle_punch_hole_request`/`handle_udp_punch_hole_request` 的新注释引用了 4 个**改动前（a635a9b）行号**，在当前文件不解析；`:610` 与 `:779` 仍有效。
- 证据：
  - `:1184` 注释「此处与 :1111 的 nat_type=SYMMETRIC」→ 实际在 `:1134`
  - `:968` 注释「send_to_tcp :1195 的 remove」→ 实际在 `:1229`
  - `:974` 注释「与 send_to_tcp :1196 相同的 spawn 风格」→ 实际在 `:1230`
  - `:979` 注释「tcp_punch 项被 :1618 清理」→ 实际在 `:1656-1658`
- 定性：注释的**事实陈述全部为真**（已逐一对照改动前文件验证），纯定位腐烂；方案 `§7.3`（feas-minimal-plan.md:271）也声明了文档用改动前口径，但代码注释活在改动后的文件里，会误导读者。
- 要求的最小修正：把这 4 处改为当前行号，或改用函数名定位（`send_to_tcp` / `handle_tcp` 连接关闭清理），后者更抗漂移。

### F-R2（minor，需改文档）方案 §5.2 两条安全断言在 M3 落地后不准确

- 问题 (a)：`feas-minimal-plan.md:166` 称「M1/M2/M3 都不新增对外发送的 UDP 包，只改变既有响应包的传输通道与字段内容，放大面不变」。精确表述：M3 使原先被 A 的 NAT 静默丢弃的 UDP 应答改走 TCP 真正送达 ⇒ 控制器首次通过 `udp_nat_port > 0 && ph.is_udp && s.is_some()`（master `client.rs:1031`）建起 UDP future 并向 `p.socket_addr` 打洞（`common.rs` `punch_udp`，3s、约 20+ 个探测包）。这些包是客户端既有行为、非服务端发送，但「从不到达」变「到达」确实改变了实际触达量。
- 问题 (b)：`feas-minimal-plan.md:167` 称「不存在『未注册来源直接触发回包』的新路径」，其论据是「链路起点是 A 的已认证 PunchHoleRequest」。该论据把意图当成了代码不变量：UDP `PunchHoleSent` arm（`:511-513`）**无鉴权、无注册校验、无速率限制**，`phs.socket_addr` 完全由报文自身决定。任意 UDP 源伪造 `phs.socket_addr = A 的公网 TCP 地址` 即可让 hbbs 向 A 的 TCP sink 投递一次 `is_udp=true` 的应答，其 `socket_addr` 指向攻击者自选地址 ⇒ A 的客户端会向该地址发约 20+ 个探测包/3s（有界反射），并可能因 `udp.0.take()`（master `client.rs:1030`）被提前消费而**抢占**合法 UDP punch（拒止直连）。前置条件：攻击者知道 A 的公网 IP:端口，且落在 A 的 `tcp_punch` 项存活窗口内（:1656-1658 连接关闭才清理）。
- 定性：不是新增放大源（请求数十字节、响应一次性、客户端探测有界）；「让 A 向某地址打洞」的原语在改动前已可经 TCP `PunchHoleSent` 达成（攻击者自建 TCP 连接、`socket_addr` 指向自己）；M3 的新增量仅是「可用伪造 UDP 源指向第三方」。
- 要求的最小修正：**改文档**——重写 `§5.2` 这两条 bullet 为实现后口径，并在 `§7.2` 增补一条已知限制（含前置条件与影响面）；**代码不改**。可选硬化（另案评估，**不作本期条件**）：校验 UDP `PunchHoleSent` 源 IP 与 `phs.id` 对应 peer 的注册 IP 一致（注意 NAT 重绑定 / 多公网 IP 可能误杀合法流量）。

### F-R3（minor，需改文档）§4 cone–cone 行「UDP 失败后 TCP punch 仍会跑」不准确

- 问题：`feas-minimal-plan.md:141` 的该断言与客户端代码不符。受控端在 `ph.udp_port > 0` 时**提前 return**（master `rendezvous_mediator.rs:1145-1150`），且 `punch_tcp` 要求 `ph.udp_port <= 0`（:1096）⇒ 受控端不再做 `connect_tcp_local`（:1173）；只有控制器侧仍在后台 `connect_tcp_local`（master `client.rs:1483`）⇒ TCP 同时打开无法完成，只能等 `connect_timeout` 超时回落 relay。18s 最坏回落结论仍成立（超时由 `connect_timeout` 决定，与今天同一分支同一超时），但机理表述错误。
- 本次复核新确认的行为变化边界（建议写入文档）：受控端从 `punch_tcp` 切换为 `punch_udp` 的条件是 `!relay_v4 && ph.udp_port > 0`，即「A 自报非对称 + B 本地判定非对称 + A 的 UDP NAT 测试已成功」——正是目标场景；其余场景（A 自报 SYMMETRIC / B 本地 SYMMETRIC / A 的 UDP 测试失败或被 `legacy_secure` 跳过 ⇒ `udp_port=0` / LAN / 强制中继）行为不变。
- 残留权衡：B 侧网络封 UDP 但放行 TCP 时，原先可能的 TCP punch 变为 relay（A 侧封 UDP 时会因 `TestNatRequest` 失败使 `udp_port=0` 而自愈）。这是与官方服务端能力对齐的**固有权衡、非实现缺陷**（官方 Pro 服务端若不下发 `udp_port`，官方 UDP punch 本身不可能工作）。
- 要求的最小修正：改 `§4` 该行表述 + 在 `§7.2` 增补上述权衡与边界；代码不改。

### F-R4（minor，需改代码，discretionary）改动行的 rustfmt 形态不规范

- 问题：`:1187-1191` 的 `udp_port: if lan_overridden { 0 } else { ph.udp_port as _ }` 多行写法与 rustfmt 输出不符（会合并为单行，79 字符 < `max_width=100`）。`cargo fmt --check` 对该行报 diff。
- 定性：**仓库基线本就非 rustfmt-clean**——同文件 `:15`/`:40`/`:1135` 及 `libs/hbb_common/src/{lib,proxy,tcp}.rs`、`src/common.rs` 均有存量 diff，`cargo fmt --check` 在改动前即失败 ⇒ 本条不构成新的门失败，且改动行零新增编译告警。
- 要求的最小修正：把该赋值写成 rustfmt 规范单行（或对改动行局部格式化）以缩小 diff 噪音。若团队不采纳 cargo fmt 纪律，可关闭本条。

### F-R5（minor，仅记录，不要求改动）`handle_local_addr` 的 UDP 路径存在同构未修缺陷

- 问题：`handle_local_addr`（:989-1009）与 M3 修复前的 `handle_hole_sent` 同构——经 UDP 到达时（:515 `Some(socket)`）应答同样被 UDP 发到 A 的 TCP 观察端口（会被 A 的 NAT 丢弃），同样不置 `is_udp`。
- 定性：标准客户端的信令连接恒为 TCP（master `client.rs:824`/`:832`/`:882` 均为 `connect_tcp`；punch socket 只发 `PunchHoleSent`，不发 `LocalAddr`）⇒ `:515` 对标准客户端**不可达**，本期不修合理；这与方案「只覆盖 TCP/UDP punch 两条路径」的范围一致。
- 要求的最小修正：无（仅记录此不对称，避免后续把「M3 已覆盖全部 UDP 应答路径」误当作结论）。

---

## 5. 对最小性的明确判断

**结论：三处改动是最小可工集，不存在改动更小的等价替代路径。**

1. **无冗余**：M1/M2/M3 分别对应标准客户端 6 道门中的 G5/G4/G6（t2 审计口径），缺任一则目标场景仍不通——M1 缺 ⇒ 受控端 `punch_udp` 恒不触发；M2 缺 ⇒ 控制器 UDP socket 被 `udp.0.take()` 永久吞掉；M3 缺 ⇒ 应答被 A 的 NAT 静默丢弃、控制器 3 次尝试全超时。
2. **无更小替代**：三处分布在三个不同函数（`handle_punch_hole_request` / `handle_hole_sent` / `handle_udp_punch_hole_request`），服务端可改点本来就只有这些；M2 的 `is_udp` 与 M3 的回程是同一报文的两个不同侧面（字段内容 + 传输通道），无法合并为一个 match arm 或一个条件达成。「一个 match arm + 一个决策条件」的方案不存在。
3. **F4 修正以最小代价落地**：`ph.udp_port = 0` 一行（:1286）即消除「未鉴权 UDP 源指挥受控端向任意 addr:port 打洞」的面，未引入按 IP 的鉴权判断（更重、有误杀风险）。
4. **F1 修正落地正确**：M4 未写代码；实现也未引入任何按 `id` 的键假设——键恒为 `try_into_v4(addr)`，两个并发 PunchHoleRequest 因 `_start_inner` 每次新建 TCP 连接（master `client.rs:824`）而是两条独立键，一次性 `remove` 不会误消费。
5. **可回退性**：diff 未提交，`git checkout` 即整体回退；无新增配置项/进程内状态 ⇒ 无残留。方案 §6.4 建议的总闸 `UDP_PUNCH_FORWARD=off` **未实现**，实现方选择了该节明列的备选口径（「若不接受新增开关，则以 `git revert` 为回退路径」）⇒ 不算偏离，但意味着灰度期需要重启/回滚二进制而非原位关断，建议在验收记录里注明。

---

## 6. 验证记录（本次独立复跑）

| 项 | 命令 / 方式 | 结果 |
|---|---|---|
| 编译 | `cd apps/server && PATH=$HOME/.cargo/bin DATABASE_URL=sqlite://./test.sqlite3 cargo check --all-targets` | **exit 0**，`Finished dev profile`；告警全部为既有（`common.rs:14` UserStatusMap、`:193`/`:202` dead code 等），**改动行零告警** |
| 单测 | 同环境 `cargo test --lib` | **66 passed / 0 failed / 0 ignored**，exit 0（与 t5 报告一致） |
| 格式 | 同环境 `cargo fmt --check` | 存量失败（见 F-R4）；改动行新增 1 处格式 diff |
| 协议字段号 | server `libs/hbb_common/protos/rendezvous.proto:27/:37/:112` ↔ app `rustdesk-app/libs/hbb_common/protos/rendezvous.proto:38/:77/:157` | `PunchHoleRequest.udp_port=7` / `PunchHole.udp_port=4` / `PunchHoleResponse.is_udp=9` **两侧一致**，零协议变更声明成立 |
| 生成代码 | `target/debug/build/hbb_common-*/out/protos/rendezvous.rs:1763` | `pub is_udp: bool` 为普通字段（非 oneof），无 setter ⇒ `p.is_udp = true` 是唯一写法 |

---

## 7. 放行条件（第 2 轮准入）

1. **F-R1**：把 4 处注释行号更新为当前口径（`:1134`/`:1229`/`:1230`/`:1656-1658`）或改用函数名定位。
2. **F-R4**：把 `:1187-1191` 的 `udp_port` 赋值写成 rustfmt 规范单行（若团队明确不接受 cargo fmt 纪律，可豁免并在记录中注明）。
3. **F-R2**：重写 `feas-minimal-plan.md:166-167` 两条安全断言为实现后口径，并在 `§7.2` 增补已知限制。
4. **F-R3**：修正 `feas-minimal-plan.md:141` 的「TCP punch 仍会跑」表述，并在 `§7.2` 增补「受控端 punch_tcp→punch_udp 切换的行为边界与残留权衡」。
5. **F-R5**：无需改动，仅记录。

完成 1-4 后第 2 轮可判 pass；F-R5 不构成阻塞。**本期未发现任何需要改变实现形状或重新设计的问题。**
