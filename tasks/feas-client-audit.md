# 标准客户端 NAT 判定与打洞触发行为审计（master 口径）

- 任务：t2（attempt_id `c6c7d1bc-6511-4bbe-906c-e377794f423e`）
- 审计人：client-auditor
- 口径：rustdesk-app **master**（本地 `ba6ee2b2b` = `origin/master`）为「标准客户端」；`feat/udp-punch`（HEAD `0454d61b4`）为定制口径，仅在明确标注处引用。
- 行号来源：master 版文件已导出到 `/tmp/master_ref/`（`src/client.rs` 5911 行、`src/common.rs` 3729 行、`src/rendezvous_mediator.rs` 1811 行）；服务端行号为 `/Volumes/USB-DISK/rust-desk/rustdesk-liusir/apps/server/src/rendezvous_server.rs`（HEAD `a635a9b`）。
- 版本边界（考古结论）：本地无 `1.1.16` tag，且本机无网络（`git ls-remote` 无输出、`web_fetch`/`web_search` 失败，web_search 报 HTTP 402）⇒ 「官方 1.1.16」无法直接取到比对。可确证的是：**1.1.9（`d716e2b40`, 2022-05-02）的 `test_nat_type_()` 与 master 结构完全一致，但完全没有 UDP punch**（proto 无 `udp_port`/`is_udp`/`force_relay`，client.rs 无 `test_udp_uat`/`udp_nat_wait`）；UDP punch 首发于 **1.4.0**（`7792ac148`, 2025-06-12，`git tag --contains` 得 1.4.1..1.5.1）。因此任何 ≤1.3.x 的官方客户端（含 1.1.16 一代）**结构上不可能有 UDP 打洞**，链路只能走 TCP punch 或 relay。

---

## 0. 结论摘要（TL;DR）

标准客户端（未修改官方构建）在 UDP 打洞链路上有 **6 道门**，其中 5 道由服务端行为决定、1 道由客户端本地配置决定。仅靠「hbbs 应答 UDP TestNatRequest」（服务端 `af0c356` 已落地）**不足以打通**——实测口径下仍会全量走 relay：

| # | 门 | 位置 | 现状（master 客户端 + liusir 服务端） |
|---|---|---|---|
| G1 | 客户端必须开启 UDP punch | `src/common.rs:1341-1357` + `libs/hbb_common/src/config.rs:2924-2936` | 自建 hbbs 下默认 **关**（需 LocalConfig 显式 `enable-udp-punch='Y'`） |
| G2 | UDP NAT 测试要成功 | `src/client.rs:5445-5542` `test_udp_uat()` | hbbs 未答 UDP TestNat 时恒失败 → `udp_port=0`（`af0c356` 已修服务端侧） |
| G3 | PunchHoleRequest 要带 `udp_port>0` | `src/client.rs:960` | 依赖 G1+G2 |
| G4 | `PunchHoleResponse.is_udp=true` | `src/client.rs:1031` | **服务端从不设置该字段**（结构性）→ 客户端 UDP socket 被永久取走 |
| G5 | `PunchHole.udp_port>0` 转发给受控端 | `src/rendezvous_mediator.rs:1095` | **服务端从不设置该字段**（结构性）→ 受控端 UDP punch 分支永不触发 |
| G6 | PunchHoleResponse 要能回到控制器 | `rendezvous_server.rs:918-963` | 受控端经 UDP 发 PunchHoleSent 时，回程被 UDP 发给控制器的 **TCP** 源端口 → NAT 丢弃 |

另有一条不可由服务端修复的路径：受控端本地 `relay_v4` 判定（`src/rendezvous_mediator.rs:1091-1094`）含 `Config::get_nat_type()==SYMMETRIC` 项，来自客户端自己的 TCP NAT 测试，CGNAT 下会误判 ⇒ 即使 G3/G4/G5 全修，误判端仍直接 relay。

---

## 1. `src/client.rs connect()`：PunchHoleRequest 的构造与发送条件

### 1.1 PunchHoleRequest 构造（`src/client.rs:953-969`）

```rust
// src/client.rs:953-969（master）
let mut msg_out = RendezvousMessage::new();
msg_out.set_punch_hole_request(PunchHoleRequest {
    id: self.data.id.clone(),
    token,                                   // 954-955：来自 interface
    nat_type: my_nat_type.into(),            // 853-857 决定
    licence_key,
    conn_type: self.conn_type,
    version: crate::VERSION.to_owned(),
    udp_port: udp_nat_port as _,             // 960 ← 唯一来源 test_udp_uat()
    force_relay: interface.is_force_relay(), // 961
    socket_addr_v6: ...,
    switch_code, webrtc_sdp_offer: ...,
    ..Default::default()
});
```

- `nat_type`（`src/client.rs:853-857`）：`interface.is_force_relay()` → 直接 `SYMMETRIC`；否则 `NatType::from_i32(my_nat_type)`，`my_nat_type = crate::get_nat_type(100)`（超时 100ms 的 IPC 读 `Config::get_nat_type()`，`src/common.rs:844/850`）。**这是客户端本地结论，服务端无法影响。**
- `transports` 列表（`src/client.rs:935-952`）只用于日志/`punch_type`：`udp_nat_port>0` 推 `"UDP"`、`allow_tcp_punch` 推 `"TCP"`、IPv6 推 `"IPv6"`、offer 推 `"WebRTC"`，全空则 `"Relay"`。
- 发送循环（`src/client.rs:977-1297`）：最多 **3 次**尝试，`attempt_deadline = i * 3000ms`（`i=0,1,2` ⇒ 0s/3s/6s 起），`socket.send(&msg_out)` 后 `get_next_nonkeyexchange_msg`。
- **发送通道是 TCP**：`src/client.rs:956` 是 master 全仓唯一 `set_punch_hole_request` 点，`src/client.rs:977` 的 `socket` 是 TCP rendezvous socket。hbbs `handle_udp()` 的 `Union::PunchHoleRequest` arm（`rendezvous_server.rs:500-509`）对标准客户端是**死代码**。

### 1.2 `udp_nat_port` 来源：`test_udp_uat()`（`src/client.rs:5445-5542`）

- socket：`new_direct_udp_for_unverified(&server_addr, ...)`（`libs/hbb_common/src/socket_client.rs:240-247`），1s 解析超时、**无握手/无注册**，bind `Config::get_any_listen_addr()`。
- 请求：`TestNatRequest` 只带 `serial` 字段（`src/client.rs:5456-5459`），目标 `server_addr` = rendezvous server 的 **UDP 21116**。
- 重试策略：初始 burst 2 包 + 自适应重试，20ms 起 ×1.5 递增，`MAX_INTERVAL=200ms`（`src/client.rs:5462-5502`）。
- 成功即 `*udp_port.lock().unwrap() = response.port as u16` 后 break（`src/client.rs:5510-5511`）。
- 收错误 sleep 10ms 防自旋（`src/client.rs:5519-5527`）；结束打 debug 日志 `"UDP NAT test to {:?} finished: time={:?}, port={}, packets_sent={}, success={}"`（`src/client.rs:5533-5540`）。
- 门控：仅 `src/client.rs:500` `if crate::get_udp_punch_enabled() && !interface.is_force_relay()` 才建该 socket 并 spawn。**`force_relay` 时连测试都不做。**

### 1.3 `udp_nat_wait` 等待窗（`src/client.rs:862-918`）

- `udp_nat_wait_from` 在密钥交换前取 `Instant`；rtt < `Self::TCP_RTT_PLAUSIBLE_MIN`(100ms, `src/client.rs:671`) 时用 `UDP_NAT_TEST_GRACE=400ms`（`src/client.rs:663`），否则 `rtt/2`。
- 轮询 `*udp.lock()` 直到 `port>0` 或超时（`src/client.rs:906-915`），随后 `stop_udp_tx.send(())`（`src/client.rs:918`）。
- **`legacy_secure`（有 token/switch_code）时整段等待被跳过**（`src/client.rs:896` `filter(|_| !legacy_secure)`）⇒ 带 token 的连接路径上 `udp_port` 甚至没机会填上。
- 与 `test_udp_uat` 是**并发**的：等待窗最多 ~400ms（或 rtt/2），而 `test_udp_uat` 的首包探测时延 = 客户端→hbbs 的 UDP RTT。hbbs 若快速应答，400ms 足够；若 hbbs 应答慢/丢包重试，窗口可能不够。这是 `af0c356` 之外的**潜在风险点**，建议服务端应答路径上不要做阻塞查询（当前实现直接回 `port`，OK）。

### 1.4 「直接 relay、不发 PunchHole」的判定（客户端侧，`src/client.rs:1440-1470` 附近）

标准客户端**不存在**「SYMMETRIC 就不尝试 punch」的客户端侧判定；`peer_nat_type` 只影响**超时时长**（`src/client.rs:1429-1460`）：

```rust
// src/client.rs:1429-1460（master）
const MIN: u64 = 1000;
let connect_timeout = if is_local || peer_nat_type == SYMMETRIC { MIN }
    else if relay_server.is_empty() { CONNECT_TIMEOUT }            // 18000, libs/hbb_common/src/config.rs:44
    else if peer_nat_type == ASYMMETRIC {
        match my_nat_type { ASYMMETRIC => CONNECT_TIMEOUT(direct_failures>0 ? punch_time_used*6),
                            SYMMETRIC => MIN, _ => ... }
    } ...
```
⇒ 误判 SYMMETRIC 的后果是**只给直连 1 秒**，然后按 `src/client.rs:1550-1553` 走 relay。

### 1.5 客户端「根本不会尝试 UDP punch」的判定式清单

1. `src/client.rs:500`：`get_udp_punch_enabled()` 为 false 或 `interface.is_force_relay()` → 不打洞 socket、不测 NAT。
2. `src/common.rs:1341-1357` + `libs/hbb_common/src/config.rs:2924-2936`：`option2bool("enable-*", v) = v != "N"`；自建（非 `rustdesk.com` 域）hbbs 下 `get_local_option()` 对空值返回 `"N"` ⇒ **默认关**，除非 LocalConfig 显式 `enable-udp-punch='Y'`（本机 `/Users/liusir/Library/Preferences/com.carriez.RustDesk/RustDesk_local.toml:22` 正是显式 `Y`）。
3. `src/client.rs:896`：`legacy_secure` 时跳过 `udp_nat_wait`。
4. **`src/client.rs:1030-1036`（最硬的一道）**：
   ```rust
   let s = udp.0.take();                                   // :1030 无条件取出
   if udp_nat_port > 0 && ph.is_udp && s.is_some() {        // :1031
       s.connect(peer_addr); ... udp.0 = s;                // :1034
   }
   ```
   `ph.is_udp` 为 false 时 `udp.0` 永久 `None` ⇒ `src/client.rs:1383` 传 `None` ⇒ `src/client.rs:1477` 不建 UDP future ⇒ `udp_nat_connect`/`punch_udp` 从未被调用。
5. `PunchHoleResponse.socket_addr` 为空则按 `failure`/`other_failure` 直接 bail（`src/client.rs:1002-1026`）。

> 注意：控制器是**拿 `PunchHoleResponse.socket_addr` 去 connect 的**（`src/client.rs:1031-1036`），而不是 `udp_port` 字段。所以 G4/G6（回程里必须带 `socket_addr` = 对端 UDP 观察地址，且必须送达）与 G3（Request 里必须带 `udp_port`）是两件不同的事，缺一不可。

---

## 2. `src/common.rs test_nat_type_()`：TCP 双源端口比较法与 CGNAT 误判

### 2.1 算法（`src/common.rs:738-789`）

```rust
// src/common.rs:738-789（master）
let server1 = Config::get_rendezvous_server();                    // 21116
let server2 = increase_port(&server1, -1);                        // 21115
for i in 0..2 {
    let mut socket = connect_tcp_local(..., &local_addr, 30);     // 752-763：**复用同一 local_addr**
    ...
    let port = if i == 0 { Config::set_local_ip_addr(...) ... }   // i==0 写回 "local-ip-addr"
    send TestNatRequest{serial}; recv TestNatResponse{port}
}
let ok = port1 > 0 && port2 > 0;
let t = if port1 == port2 { ASYMMETRIC } else { SYMMETRIC };      // 780-784
Config::set_nat_type(t as _);                                     // 785
```

### 2.2 CGNAT 下的误判机理

- 该测试的语义是「同一 socket 向两个不同目的（hbbs:21116 与 hbbs:21115）建 TCP，观察 NAT 分配的**公网源端口**是否相同」。
- **Endpoint-Independent Mapping（锥形）** 下两个连接的源端口相同 → `ASYMMETRIC`；**Address/Port-Dependent Mapping（对称）** 下不同 → `SYMMETRIC`。
- 在 **CGNAT** 下，运营商侧 NAT 对不同目的端口（21116 vs 21115）**主动轮转/按目的分配**不同的公网源端口（CGNAT 普遍做 per-destination port allocation 以缓解日志压力），于是**明明是 endpoint-independent mapping 的锥形 NAT 也被判成 `SYMMETRIC`**。
- 该测试还有一层放大：两次 TCP 连接是从同一 `local_addr` 发出的，若运营商 NAT 对同一内网地址+源端口的映射本就按目的分裂，误差直接体现为端口不等。
- **UDP 与 TCP 的 NAT 行为无关**：CGNAT 对 UDP 常常是 endpoint-independent mapping（这正是同一网络下 Tailscale 能 UDP 直连的原因），但 `test_nat_type_()` 只测 TCP ⇒ UDP 侧可打洞、TCP 侧判死刑。

### 2.3 结果写回后的影响面

`Config::set_nat_type`（`libs/hbb_common/src/config.rs:1133-1139`，值不变则直接 return，持久化进 `RustDesk2.toml`；本机 `nat_type=1(ASYMMETRIC)`）。读取点：

| 读取点 | 后果 |
|---|---|
| `src/client.rs:852`（构造 PunchHoleRequest.nat_type） | 把误判值**广播**给服务端，服务端再转发给受控端 ⇒ 污染对端决策 |
| `src/client.rs:1457`（connect 超时决策） | 误判 SYMMETRIC ⇒ 直连只等 `MIN=1000ms`（`src/client.rs:1429-1460`） |
| `src/rendezvous_mediator.rs:1092`（受控端 `relay_v4`） | 误判 SYMMETRIC ⇒ **直接 `create_relay` 并 return**（`src/rendezvous_mediator.rs:1117-1132`），PunchHole 都不发 |
| `src/rendezvous_mediator.rs:1157`（构造 PunchHoleSent.nat_type） | 回灌给控制器，控制器据此把超时压到 1s |
| `src/ipc.rs:1097`、`src/ipc.rs:2058-2072` | Flutter UI 显示 NAT 类型 |

### 2.4 分类结果的粘滞性

`test_nat_type()`（`src/common.rs:695-735`）的调用点只有三处：`src/main.rs:15`、`src/flutter_ffi.rs:70/76`（移动端）、`src/rendezvous_mediator.rs:275`（`start_all()`，即 mediator 重启时）。重试循环一旦 `Config::get_nat_type()!=0` 即 break ⇒ **分类只算一次并持久化**，服务端**无法触发重测**。CGNAT 端口分配策略一变（或用户换网络），客户端不会自己纠正，直到重启/换网重连。这是「误判长期固化」的结构性原因。

---

## 3. `src/rendezvous_mediator.rs handle_punch_hole()`：受控端分支

（行号均为 master，`/tmp/master_ref/src/rendezvous_mediator.rs`）

```rust
1042  async fn handle_punch_hole(&self, ph: PunchHole, peer_addr_v4: SocketAddr, server: &mut Server) {
1043      let peer_addr = AddrMangle::decode(&ph.socket_addr);
1044-1048 // 100ms 内同地址去重
1051-1052 let local_proxy = use_ws() || Config::is_proxy();
          let relay = local_proxy || ph.force_relay;            // 服务端从不设 force_relay ⇒ 仅 local_proxy
1091-1094 let relay_v4 = ph.nat_type==SYMMETRIC || Config::get_nat_type()==SYMMETRIC
              || relay || (config::is_disable_tcp_listen() && ph.udp_port<=0);
1095      let punch_udp = !relay_v4 && ph.udp_port > 0;
1096      let punch_tcp = !relay_v4 && ph.udp_port<=0 && webrtc_sdp_offer.is_empty();
1097      let punch_v6  = peer_addr_v6.port()>0 && !relay;        // 服务端从不设 socket_addr_v6 ⇒ 恒 false
1117-1132 if relay_v4 { create_relay(...); return; }             // 不发 PunchHoleSent
1133-1144 let phs = PunchHoleSent{ socket_addr: ph.socket_addr, id, relay_server,
                                  nat_type: Config::get_nat_type(), version, upnp_port,
                                  socket_addr_v6, webrtc_sdp_answer: None };
1145-1150 if ph.udp_port > 0 { peer_addr.set_port(ph.udp_port as u16); punch_udp_hole(...); return; }
1151-1163 // webrtc offer：独立 TCP 连回去回 answer
1165-1194 // 否则 log::debug!("Punch tcp hole to {:?}") + connect_tcp + connect_tcp_local(peer_addr, Some(local_addr), 30)
          // + 发 PunchHoleSent + punch_tcp_until_connected(...)
```

### 3.1 `punch_udp = !relay_v4 && ph.udp_port>0` 分支

- `ph.udp_port` 是 `PunchHole.udp_port`（proto 字段 4）。**liusir 服务端 `rendezvous_server.rs` 全文 grep `udp_port` 零命中**，构造 `PunchHole` 时（`rendezvous_server.rs:1155-1161`，自初始导入 `879edaa` 起）只设 `socket_addr`/`nat_type`/`relay_server`/`relay_servers` ⇒ **该字段恒 0** ⇒ `src/rendezvous_mediator.rs:1095` 恒 false、`src/rendezvous_mediator.rs:1145-1150` 的 UDP 分支**永不触发**。
- `punch_udp_hole`（`src/rendezvous_mediator.rs:1198-1242`）：`new_direct_udp_for(&self.host)` 拿新 UDP socket+addr → 发 `PunchHoleSent` → spawn 两条补发（10-30ms 随机）→ `udp_nat_listen(socket, peer_addr, peer_addr, server, meta, slot)`。
- `udp_nat_listen`（`src/rendezvous_mediator.rs:1464-1497`）：`socket.connect(peer_addr)`；`punch_udp(socket, true)` 拿 init_packet；`KcpStream::accept(socket, CONNECT_TIMEOUT, init_packet)`；成功后 `drop(slot)` 并 `create_tcp_connection(server, stream.1, peer_addr_v4, true, meta)` ⇒ **UDP+KCP 直连**。
- `punch_udp`（`src/common.rs:3035-3101`）：`MAX_TIME=3s`，retry 20ms ×1.5 递增至 `MAX_INTERVAL=200ms`；收 `PUNCH_ACK` 匹配 tid 且 `!listen` → 返回 `Ok(None)`（确认）；收 `PUNCH_PROBE` → 回 ACK；收其他非空数据 → 当确认返回 `Some(bytes)`。

### 3.2 TCP punch（`punch_tcp`）与 relay 兜底时序

- TCP punch 路径（`src/rendezvous_mediator.rs:1165-1194`）：`connect_tcp(peer_addr, 30)` 打通本端映射 → `connect_tcp_local(peer_addr, Some(local_addr), 30)` → 发 `PunchHoleSent` → `punch_tcp_until_connected`（`PUNCH_INTERVAL=0.15`/`PUNCH_BACKOFF=1.5`/`PUNCH_MAX_INTERVAL=2.0`/`PUNCH_GRACE=3000`，`until = now + CONNECT_TIMEOUT(18s)`，`src/rendezvous_mediator.rs:1532+`）。日志形如 `None of 14 punches to 118.112.177.100:64830 was met` ⇒ **运营商 NAT 之间 simultaneous-open 失败**，这是 PRD 已记录的现象。
- relay 兜底：`relay_v4` 为真时**立即** `create_relay` 并 return（`src/rendezvous_mediator.rs:1117-1132`），`PunchHoleSent` 都不发 ⇒ 控制器侧连 `PunchHoleResponse` 都收不到，只能等 `peer_addr.port()==0` → bail `"Failed to connect via rendezvous server"`（`src/client.rs:1344-1354`）。
- 受控端**没有**「先 punch 失败再 relay」的时序：要么立即 relay（`relay_v4`），要么 punch 直到 `CONNECT_TIMEOUT`；`punch_udp` 自身 `MAX_TIME=3s` 后会怎样取决于 `KcpStream::accept` 结果。
- 控制器的 relay 兜底在 `src/client.rs:1550-1574`：`if (interface.is_force_relay() && typ != "WebRTC") || conn.is_err()` → `request_relay`；否则 bail `"Failed to make direct connection to remote desktop"`（`src/client.rs:1572`）。⇒ **标准客户端没有「先 relay 兜底再直连」**，直连失败时只有一次 relay 机会，且 `relay_server` 非空才会走。

### 3.3 定制口径对照（`feat/udp-punch` 相对 master）

`git diff master..feat/udp-punch -- src/client.rs src/common.rs src/rendezvous_mediator.rs`：

- `src/client.rs` +35：`relay_candidates` 接收（`src/client.rs:1030`）、`relay_probe::pick`（`src/client.rs:1360-1367`）、`connect()` 增 `udp_nat_port_used` 形参（`src/client.rs:1403`/`1431`）、`punch_stats` 日志（`src/client.rs:1665-1682`）。
- `src/common.rs` +13：`test_nat_type_` 的 SYMMETRIC 分支改调 `crate::nat_stun::recheck`（US-004）。
- `src/rendezvous_mediator.rs` +25：`punch_req` 埋点（`src/rendezvous_mediator.rs:1097-1110`）、`relay_server` 改 `mut` + `relay_probe::pick`（`src/rendezvous_mediator.rs:1126-1133`）。
- **未触碰** `test_udp_uat` / `udp_nat_wait` / `punch_udp` / `udp_nat_connect` / `is_udp` 门 / `PunchHole` 构造 ⇒ **FR-6 成立，master 即标准口径**，本审计结论对标准客户端全部适用。
- 协议侧新增字段（`libs/hbb_common` submodule `fae47e0`→`784b43c`，仅 protos/rendezvous.proto +8 行）：`PunchHole.relay_servers=11`、`PunchHoleResponse.relay_servers=13`。旧客户端忽略未知字段，**这是唯一安全的「服务端主动下发新信息」通道的先例**。

---

## 4. 标准客户端向服务端暴露的全部可观测量

### 4.1 TCP rendezvous 通道（hbbs `handle_listener`）

hbbs 从控制器的 TCP 连接上可观：

| 可观测量 | 来源 | 可信度 |
|---|---|---|
| A 的 TCP 公网源地址:端口 | TCP accept | **服务端观测，可信** |
| `RegisterPeer{id, serial}` | proto 15-18 | 客户端声明 |
| `RegisterPk{id, uuid, pk, old_id, no_register_device}` | `rendezvous_server.rs` `handle_register_pk` | 客户端声明 |
| `PunchHoleRequest.id` | proto 31-47 | 客户端声明 |
| `PunchHoleRequest.token` / `licence_key` / `conn_type` / `version` / `switch_code` | 同上 | 客户端声明，`MUST_LOGIN` 下 token 已校验 |
| `PunchHoleRequest.nat_type` | **客户端本地 TCP 测试结论** | **不可信（CGNAT 误判）** |
| `PunchHoleRequest.udp_port` | **hbbs 自己在 TestNatResponse 里回显的值** | 服务端自产，可信（除非客户端故意伪造） |
| `PunchHoleRequest.force_relay` | 客户端本地 option/UI | 客户端声明 |
| `PunchHoleRequest.upnp_port` / `socket_addr_v6` | 客户端声明 | 服务端 Rust 代码不读 |
| `PunchHoleRequest.webrtc_sdp_offer` | 客户端声明 | — |

### 4.2 UDP 21116 通道（hbbs `handle_udp`）

- A 的打洞 UDP socket 发 `TestNatRequest` ⇒ hbbs 观测到 **A 的 UDP 公网源地址:端口**（目的固定为 21116）⇒ 这就是 `udp_port` 的来源。
- 标准客户端**不会**从 UDP 发 `PunchHoleRequest`（`src/client.rs:977` 走 TCP）⇒ `rendezvous_server.rs:500-509` 的 UDP PunchHoleRequest arm 是死代码。
- **hbbs 对 A 的 UDP 观测只有一个目的（21116）** ⇒ 服务端**没有第二个 UDP 观测点**可用于判断 A 的 UDP mapping 类型。

### 4.3 关键判断题：服务端能否仅凭协议内信息判断「这个 SYMMETRIC 声明不可信」？

**结论：不能。** 理由：

1. `nat_type` 是客户端本地 TCP 测试的产物，服务端只能**复现同一个测试**（比对 A 到 21116 / 21115 的 TCP 源端口），而复现得到的是**同样的错误答案**——CGNAT 的 per-destination 端口分配对服务端观测同样成立。⇒ 服务端可以验证「客户端是否说谎」，不能验证「客户端是否判错」。
2. 判断 UDP mapping 类型需要客户端向**两个不同目的**发 UDP；标准客户端只向 hbbs:21116 发（`src/client.rs:5456-5459`）。hbbs 没有第二个 UDP 观测点 ⇒ 无数据。
3. 主动探测（hbbs 从另一源端口向 A 的 `udp_port` 发包看是否到达）不可用：A 的 `punch_udp` 尚未运行，且 `punch_udp` 启动时会 `try_recv` 清空缓冲（`src/common.rs:3035-3101`）⇒ 无应用层信号回传给 hbbs。
4. **唯一自产可信的字段是 `udp_port`**（hbbs 自己回显的）。这是服务端在协议内唯一握有的「硬事实」。

**但服务端有两个「改写下游决策」的协议杠杆**（均非欺骗检测，而是覆盖）：

- 杠杆 A：`PunchHole.nat_type` / `PunchHoleSent.nat_type` 是服务端转发的，可把误判值改写成 `ASYMMETRIC`，使对端 `relay_v4` 的 `ph.nat_type==SYMMETRIC` 项失效、使控制器 `connect_timeout` 从 `MIN=1000ms` 放宽到 `CONNECT_TIMEOUT`。**但这治不了受控端本地的 `Config::get_nat_type()` 项**（`src/rendezvous_mediator.rs:1092`），也治不了控制器本地的 `my_nat_type`（`src/client.rs:1457`）。
- 杠杆 B：`TestNatResponse.port`（`rendezvous_server.rs` TCP 21115 与 UDP 21116 两处）是服务端自产。若两处**回显同一个端口**，`test_nat_type_()` 必然得到 `ASYMMETRIC`（`src/common.rs:780-784`）⇒ 全网客户端 NAT 分类被中和。代价：真正的对称型 NAT 也会去尝试 UDP punch，失败后由客户端自身 relay 兜底承担（延迟代价，非失败代价）。这是**策略性**改动，不是检测，需 captain 决策。

---

## 5. 关键判断题

### (a) hbbs 应答 UDP TestNat（服务端 `af0c356` 已做）后，UDP 打洞是否自行打通？还差什么？

**结论：不会打通。** `af0c356` 只解决 G2（`test_udp_uat` 能拿到 `udp_port`）。仍缺 4 项，全部在服务端：

| 缺口 | 位置 | 说明 |
|---|---|---|
| ① `PunchHoleResponse.is_udp` 未设置 | `rendezvous_server.rs:918-963` | `src/client.rs:1030-1036` 的硬门：`udp.0.take()` 无条件执行，`is_udp=false` 时 UDP socket 永久丢失 ⇒ 控制器**根本不会尝试 UDP punch**。历史成因：`libs/hbb_common` `df95f44`（2025-06-10）删掉 `PunchHoleSent.is_udp=6` 却保留 `PunchHoleResponse.is_udp=9`；app `git log --all -S set_is_udp` / `-S "is_udp: "` 均无命中，liusir 服务端 `git log --all -S is_udp -- apps/server/src/` 亦无命中 ⇒ 双方都不写。 |
| ② `PunchHole.udp_port` 未转发 | `rendezvous_server.rs:1155-1161` | `src/rendezvous_mediator.rs:1095` 恒 false、`src/rendezvous_mediator.rs:1145-1150` 永不触发 ⇒ 受控端不会向控制器的 UDP 端口发包。 |
| ③ PunchHoleResponse 回程路由错 | `rendezvous_server.rs:918-963` | 受控端 `punch_udp_hole` 用 UDP 发 `PunchHoleSent` ⇒ `handle_udp` → `handle_hole_sent(phs, addr, Some(socket))`，`addr_a = AddrMangle::decode(&phs.socket_addr)`，而 `phs.socket_addr` 是 hbbs 在 `PunchHole` 里填的 A 的 **TCP** 源地址 ⇒ `socket.send(&msg_out, addr_a)` 把 `PunchHoleResponse` 从 UDP 21116 发给 A 的 TCP 公网端口，A 的 NAT 无该 UDP 映射 ⇒ 静默丢弃 ⇒ 控制器 3 次尝试全超时。**修法**： PunchHoleSent 走 UDP 到达时，若 `tcp_punch` map 里仍有 A 的 TCP sink（`rendezvous_server.rs:606-613` 已按 `try_into_v4(addr)` 保留），优先 `send_to_tcp(msg_out, addr_a)`（一次性，正好够 A break 出循环），否则才走 UDP。 |
| ④ 客户端 UDP punch 开关 | `src/common.rs:1341-1357` | 自建 hbbs 下默认关；需运维在 LocalConfig 显式 `enable-udp-punch='Y'`（本机已是）。**服务端无协议内办法开启它。** |

另有一个**时序风险**（非阻塞）：`udp_nat_wait` 窗口 400ms（`src/client.rs:663`/`862-918`）要容纳 `test_udp_uat` 的完整往返；hbbs 应答必须低延迟（当前直接回 `port`，无阻塞查询，OK）。

**附带**：即使 ①②③ 全修，控制器 `src/client.rs:558-562` 在 UDP punch 开启时会**并发发 2 个 PunchHoleRequest**（第二个 `udp_nat_port=0` 走 TCP punch，受控端 `src/rendezvous_mediator.rs:1044-1048` 100ms 同地址去重）。这是标准客户端的固有行为，服务端必须容忍。

### (b) TCP NAT 误判 SYMMETRIC 的场景，纯服务端有无任何合规手段挽救？

**结论：没有「合规」的挽救手段；只有两个覆盖式杠杆，都有代价。**

- 服务端**无法检测**误判（见 §4.3）。
- 杠杆 A（改写转发的 `nat_type`）治不了**两端各自的本地判定**：
  - 受控端 `relay_v4 = ... || Config::get_nat_type()==SYMMETRIC || ...`（`src/rendezvous_mediator.rs:1092`）——纯本地，服务端零控制权。
  - 控制器 `my_nat_type`（`src/client.rs:1457`）决定 punch 超时——同样纯本地。
  - ⇒ 只要**任一端**本地误判为 SYMMETRIC，UDP punch 就不会被尝试。PRD 记录的日志「无 PunchHole，直接 `create_relay requested`」正是受控端本地 `relay_v4` 直接成立。
- 杠杆 B（中和 `TestNatResponse.port` 使分类恒为 ASYMMETRIC）会让**所有**客户端（含真正对称型）都去尝试 punch，失败后由客户端自身 relay 兜底（`src/client.rs:1550-1553`）。代价是**建立时延**（最多 `punch_time_used*6` 或 `CONNECT_TIMEOUT=18s`，`src/client.rs:1429-1460`），不是失败。是否采用是产品/运维策略，且它与「不修改客户端行为」的团队目标相容（回显什么端口是服务端自由），但**它破坏了协议字段语义**，需明确写进方案的风险栏。
- **合规解**只有一条：让客户端用 STUN 复测修正分类——即 `feat/udp-punch` 的 US-004（`src/nat_stun.rs`，`test_nat_type_` 的 SYMMETRIC 分支改调 `crate::nat_stun::recheck`，`src/common.rs` +13 行）。**标准客户端没有这段代码**（master `test_nat_type_` 在 SYMMETRIC 分支直接 `Config::set_nat_type(SYMMETRIC)`，`src/common.rs:780-785`）。
- 附加结构性障碍：分类结果**粘滞**（`src/common.rs:695-735` 重试循环 `Config::get_nat_type()!=0` 即 break；`Config::set_nat_type` 值不变直接 return，`libs/hbb_common/src/config.rs:1133-1139`）⇒ 服务端无法触发重测，误判会一直持续到客户端重启。

### (c) 官方 1.1.16 与 master 在该链路上是否有行为差异？以哪个为口径？

**结论：以 master 为口径；且 1.1.16 一代在本链路上严格更弱，不会比 master 更能打洞。**

- 本机无网络且无 `1.1.16` tag，无法直接 diff；但考古证据充分：
  - `1.1.9`（`d716e2b40`, 2022-05-02）的 `test_nat_type_()` 与 master **结构完全一致**（server2 = port-1、复用 `local_addr`、`port1==port2 → ASYMMETRIC`）⇒ CGNAT 误判机理在 1.1.16 一代同样存在。
  - `1.1.9` **完全没有 UDP punch**：proto 无 `udp_port`/`is_udp`/`force_relay`，client.rs 无 `test_udp_uat`/`udp_nat_wait`。
  - UDP punch 首发于 **1.4.0**（`7792ac148`, Cargo version 1.4.0；`git tag --contains` = 1.4.1..1.5.1）。
  - ⇒ 任何 ≤1.3.x 的官方客户端**结构上不可能有 UDP 打洞**；其唯一直连手段是 TCP simultaneous-open（在运营商 NAT 间失败，见 §3.2 日志），其余全走 relay。
- `test_nat_type_()` 的双源端口比较法在 1.1.9 → master 之间未变 ⇒ **误判 SYMMETRIC ⇒ relay_v4 的结论在 1.1.16 一代同样成立**。
- 因此：**口径取 master**（本地 `ba6ee2b2b` = `origin/master`，fork 侧与上游 1.5.x 同代；`upstream/master=33a154a5d` 本地领先 16 落后 3，差异未触及 punch 链路的已由 diff 确认：`master..feat/udp-punch -- src/ libs/` 只含 STUN recheck/埋点/relay 探测/proto +8 行/scrap 编解码调参）。
- 对部署的含义：**面向官方 1.1.16 的「UDP 打洞直连」方案不存在**；面向 1.4.0+ 官方客户端（含 master/1.5.x）的方案受 §1.5/§5 六门约束。

---

## 6. 标准客户端行为约束清单（服务端方案必须满足的硬约束）

> 以下每条都是「标准客户端不改一行代码」前提下的硬约束；违反任一条 => 该客户端不会打洞。

**C1 — 客户端本地开关不可由服务端改变。**
自建 hbbs 下 `enable-udp-punch` 默认 `"N"`（`src/common.rs:1341-1357` + `libs/hbb_common/src/config.rs:2924-2936`），且 `force_relay` 时连 UDP 测试都不做（`src/client.rs:500`）。⇒ 方案必须**默认假设 UDP punch 可能关闭**，部署侧需显式配置 `enable-udp-punch='Y'`；不能指望服务端激活。

**C2 — `TestNatRequest` 必须被快速应答。**
标准客户端等待窗 400ms（`src/client.rs:663`，rtt<100ms 时）或 `rtt/2`，且 `legacy_secure` 时完全不等待（`src/client.rs:896`）。hbbs 的 UDP 应答路径**不得**做阻塞 DB/注册查询（当前 `af0c356` 直接回 `port`，符合）。

**C3 — `PunchHoleRequest.udp_port` 必须原样转发给受控端。**
`src/rendezvous_mediator.rs:1095`/`1145-1150` 要求 `PunchHole.udp_port>0`。hbbs 构造 `PunchHole`（`rendezvous_server.rs:1155-1161`）必须带上 `ph.udp_port`。

**C4 — `PunchHoleResponse.is_udp` 必须置 true。**
`src/client.rs:1030-1036` 是硬门：`is_udp=false` 会**永久吞掉控制器的 UDP 打洞 socket**（`udp.0=None` ⇒ `src/client.rs:1477` 不建 UDP future）。⇒ hbbs 回 `PunchHoleResponse` 时必须 `set_is_udp(true)`，且 `socket_addr` 必须是**受控端在 hbbs 处观测到的 UDP 源地址**（控制器 `s.connect(peer_addr)` 用的就是它，`src/client.rs:1031-1036`）。

**C5 — PunchHoleResponse 必须送达控制器，且送达通道要匹配。**
受控端经 UDP 发 `PunchHoleSent`（`punch_udp_hole`，`src/rendezvous_mediator.rs:1198-1242`）时，hbbs 不能把回程 UDP 发给 A 的 TCP 源端口（`rendezvous_server.rs:918-963` 现状）——A 的 NAT 无该 UDP 映射。必须优先用 `tcp_punch` map 里保留的 A 的 TCP sink 回送（`rendezvous_server.rs:606-613` 已保留，`send_to_tcp` 一次性语义正好匹配 A 的 break）。

**C6 — 不能假设客户端只发一个 PunchHoleRequest。**
UDP punch 开启时标准客户端会并发发 2 个（第二个 `udp_nat_port=0` 走 TCP punch，`src/client.rs:558-562`）。服务端必须容忍重复/去重（受控端 `src/rendezvous_mediator.rs:1044-1048` 100ms 去重只覆盖同地址），且不能把第二个请求当作异常。

**C7 — 不能假设客户端的 NAT 分类可信。**
`nat_type` 是客户端本地 TCP 测试结论（`src/common.rs:738-789`），CGNAT 下锥形被判 SYMMETRIC，且**粘滞**（`src/common.rs:695-735` 循环一次即 break）。服务端不得据此跳过打洞尝试，也不得把该值当事实转发给对端做门控（对端 `relay_v4` 的 `ph.nat_type==SYMMETRIC` 项、控制器 `connect_timeout` 的 `peer_nat_type==SYMMETRIC` 项都会读它）。

**C8 — 无法从服务端挽救两端的本地 SYMMETRIC 判定。**
受控端 `Config::get_nat_type()`（`src/rendezvous_mediator.rs:1092`）与控制器 `my_nat_type`（`src/client.rs:1457`）都是纯本地。只要任一端本地判 SYMMETRIC，UDP punch 就不被尝试（受控端直接 `create_relay`，`src/rendezvous_mediator.rs:1117-1132`）。⇒ 面向「已被误判的存量客户端」的纯服务端方案不存在；STUN 复测必须落在客户端（US-004）。

**C9 — 直连失败只有一次 relay 兜底，且必须由 `relay_server` 非空触发。**
`src/client.rs:1550-1574`：非 `force_relay` 且 `conn.is_err()` 才 `request_relay`；否则 bail。⇒ 服务端若想「打洞失败回中继」，必须保证 `relay_server`/`relay_servers` 字段非空下发（`PunchHole.relay_servers=11` 已被 liusir 扩展，`PunchHoleResponse.relay_servers=13` 同理，旧客户端忽略未知字段——这是安全的扩展先例）。

**C10 — 协议扩展必须走「新增字段」而非改语义。**
旧客户端忽略未知 proto 字段（`libs/hbb_common` `fae47e0`→`784b43c` 只 +8 行 relay_servers 即此先例）。⇒ 任何新信息（如 NAT 复测结果、打洞建议）都应新增字段，不得改写既有字段含义；不得要求客户端升级才能维持现有 relay 行为。

**C11 — 官方 ≤1.3.x（含 1.1.16）没有 UDP punch，方案不得以其为目标。**
UDP punch 首发 1.4.0（`7792ac148`）；1.1.9 的 `test_nat_type_` 与 master 同构但无 UDP 路径。⇒ 「标准客户端 UDP 打洞直连」的可行集是**官方 1.4.0+**；1.1.16 一代只能 relay（或 TCP punch，运营商 NAT 间实测失败）。

**C12 — 安全边界不可降级。**
打洞 socket 从不注册（`new_direct_udp_for_unverified`，`libs/hbb_common/src/socket_client.rs:240-247`）⇒ 「仅答注册来源」会让功能失效（FR-2 已作废）。但请求 11B → 响应 ~8B 无放大面，且 `handle_udp` 应答路径不得成为反射放大器；`MUST_LOGIN` 下的 token 校验不可绕过。

---

## 7. 对后续实现阶段的提示（遵守 `rustdesk-app/AGENTS.md`）

- 服务端最小 diff 的落点已定位：`rendezvous_server.rs` 的 `handle_hole_sent`（C4/C5）+ `handle_tcp_punch_hole_request`/`set_punch_hole` 构造处（C3）。
- 客户端侧不需要为「标准客户端可用」改任何代码（FR-6 已由 diff 确认 master 即标准口径）；`feat/udp-punch` 的 STUN recheck/埋点/relay 探测都是**增量优化**，不是标准客户端打通的前提。
- 回归面最小：`handle_hole_sent` 的回程分支改动会影响**所有** relay/TCP-punch 会话 ⇒ 必须以「`socket` 为 None（TCP 到达）时行为逐位不变」为验收底线；建议对 `Some(socket)` 分支加「先查 tcp_punch sink，未命中才走 UDP」的兜底，并用 `runtime/` 下的探测脚本验证（见 `rustdesk-liusir/CLAUDE.md` 的过程产物规范）。

---

## 附：证据索引（文件:行号速查）

| 事实 | 位置 |
|---|---|
| UDP punch 默认关闭 | `src/common.rs:1341-1357`；`libs/hbb_common/src/config.rs:2924-2936` |
| 打洞 socket 创建门控 | `src/client.rs:500` |
| `tcp_punch_allowed()` | `src/client.rs:361-366` |
| 第二个 PunchHoleRequest | `src/client.rs:558-562` |
| `UDP_NAT_TEST_GRACE`/`TCP_RTT_PLAUSIBLE_MIN` | `src/client.rs:663`/`671` |
| `udp_nat_wait` | `src/client.rs:862-918` |
| `nat_type` 取值 | `src/client.rs:853-857` |
| transports 列表 | `src/client.rs:935-952` |
| PunchHoleRequest 构造 | `src/client.rs:953-969`（`udp_port` :960） |
| PunchHoleRequest 发送（TCP） | `src/client.rs:977` |
| PunchHoleResponse 处理 | `src/client.rs:1002-1049` |
| **`udp.0.take()` + `is_udp` 硬门** | `src/client.rs:1030-1036` |
| relay 兜底/bail | `src/client.rs:1550-1574` |
| connect 超时决策 | `src/client.rs:1429-1460` |
| `test_nat_type_()` | `src/common.rs:738-789` |
| `test_nat_type()` 调度/粘滞 | `src/common.rs:695-735` |
| `punch_udp()` | `src/common.rs:3035-3101` |
| `handle_punch_hole` 判定段 | `src/rendezvous_mediator.rs:1091-1097` |
| relay_v4 → create_relay return | `src/rendezvous_mediator.rs:1117-1132` |
| UDP punch 分支 | `src/rendezvous_mediator.rs:1145-1150` |
| TCP punch 分支 | `src/rendezvous_mediator.rs:1165-1194` |
| `punch_udp_hole` | `src/rendezvous_mediator.rs:1198-1242` |
| `udp_nat_listen`（KCP accept） | `src/rendezvous_mediator.rs:1464-1497` |
| `test_udp_uat()` | `src/client.rs:5445-5542` |
| proto 定义 | `libs/hbb_common/protos/rendezvous.proto`（TestNatRequest 90-92、TestNatResponse 95-98、NatType 100-104、PunchHole 73-88、PunchHoleRequest 31-47、PunchHoleSent 106-115、PunchHoleResponse 140-165） |
| 服务端 UDP TestNat arm | `rendezvous_server.rs:512-535` |
| 服务端 UDP PunchHoleRequest arm（对标准客户端死代码） | `rendezvous_server.rs:500-509` |
| 服务端 TCP PunchHoleRequest arm + tcp_punch 保留 | `rendezvous_server.rs:606-613` |
| `handle_hole_sent`（回程路由） | `rendezvous_server.rs:918-963` |
| `handle_punch_hole_request` | `rendezvous_server.rs:1012-1173`（lan_overridden :1105、强制 relay :1111、PunchHole 构造 :1155-1161） |
| `handle_listener2`（21115 TestNat） | `rendezvous_server.rs:1504-1543` |
| `set_nat_type` 粘滞 | `libs/hbb_common/src/config.rs:1133-1139` |
| `CONNECT_TIMEOUT` | `libs/hbb_common/src/config.rs:44` |
| `new_direct_udp_for_unverified` | `libs/hbb_common/src/socket_client.rs:240-247` |
