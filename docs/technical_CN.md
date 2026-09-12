# 技术实现说明

一个用 Rust 编写的跨平台服务守护进程，由 Node.js 项目 `NodeDaemon` 移植而来。它负责管理常驻进程、异常退出自动重启、按天轮转日志，并对外提供 HMAC 签名保护的 HTTP API 和可选的需要登录的 Web UI。

**语言切换：** [English](technical.md) | 简体中文 | [繁體中文](technical_TW.md)

---

## 1. 架构总览

守护进程在**同一个 Tokio runtime** 内运行**两个独立的 HTTP 服务器**：

| 服务器 | 端口（默认） | 用途 |
|---|---|---|
| 守护进程 API（`daemon_server.rs`） | `DAEMON_PORT` = 10300 | 机器对机器的控制 API，由 HMAC-SHA256 签名保护 |
| Web UI（`webui_server.rs`） | `WEBUI_PORT` = 10400 | 面向人的管理界面，带登录、会话和 CSRF 保护 |

两个服务器共享同一个 `Arc<ServiceManager>` 和 `Arc<ConfigLoader>`，因此无论从哪个接口发出控制命令，操作的都是同一份进程注册表。

```
                 ┌────────────────────────── Tokio runtime ──────────────────────────┐
  POST /service/... │  daemon_server.rs     webui_server.rs                          │
  ─────────────────┼─────────────────────────────────────────────────────────────────┤
                 │         └──────────────┬──────────────┘                          │
                 │                        ▼                                          │
                 │              ServiceManager（共享）                                │
                 │   spawn / kill / restart / status / auto-restart                 │
                 │                        │                                          │
                 │               ConfigLoader（services/*.json）                      │
                 │                        │                                          │
                 │               ServiceLogger（logs/<name>/）                        │
                 └───────────────────────────────────────────────────────────────────┘
```

## 2. 模块说明

| 模块 | 职责 |
|---|---|
| `main.rs` | 入口：加载 `.env`、构建 runtime、绑定两个服务器、自动启动服务、处理退出 |
| `config.rs` | 解析/校验 `services/*.json` 服务定义，套用默认值并做数值钳制 |
| `service_manager.rs` | 进程生命周期：启动、停止、强制停止、重启、状态查询、异常退出自动重启 |
| `logger.rs` | 每个服务独立的按天轮转文件日志，带保留期清理 |
| `daemon_server.rs` | HMAC 保护的 API 端点 + CORS 中间件 |
| `webui_server.rs` | 登录/会话/CSRF 管理、Web UI API 端点、静态文件服务 |
| `os_status.rs` | 通过 `sysinfo` 采集 CPU/内存/运行时长 |
| `util.rs` | 共享工具：JSON 响应、恒定时间比较、MIME 映射、cookie 解析 |

## 3. 进程管理

### 3.1 进程启动

服务命令**通过系统 shell 执行**，与原版 `spawn(cmd, { shell: true })` 保持一致：

- **Windows**：`cmd /C <command>`，并加上 `CREATE_NO_WINDOW` 标志，避免弹出控制台窗口。
- **Unix**：`sh -c <command>`，并把子进程放入**独立的进程组**（`setpgid`），这样可以用一条 `killpg` 杀掉整棵进程树。

stdout/stderr 被管道化，两个后台任务（`spawn_output_pump`）持续把管道输出写入服务日志。

### 3.2 终止进程树

`kill_process_tree(pid, force)`：

- **Windows**：先 `taskkill /pid <pid> /T`（优雅），再 `/T /F`（强制）。`/T` 递归终止以服务 shell 为根的整棵进程树。
- **Unix**：先发优雅的 `SIGTERM`，再发 `SIGKILL`。因为服务运行在独立进程组中，信号能到达所有子孙进程。

### 3.3 状态跟踪

每个服务都跟踪：

- **running** —— `processes` 表中是否有对应 pid
- **operation** —— 操作进行中时为 `"starting"`、`"stopping"`、`"restarting"`、`"force-stopping"`
- **duration** —— 运行秒数
- **abnormalExitCount** —— 意外退出次数

### 3.4 异常退出自动重启

每个服务有一个后台**监视任务**在 `child.wait()` 上等待。进程退出时：

1. 如果 `stopping` 集合中包含该服务（停止/重启由守护进程发起），监视任务直接返回，不重启。
2. 否则，除非退出码为 0 或终止信号是 `SIGTERM`/`SIGKILL`（Unix），否则视为异常退出。
3. 异常退出后，**重试定时器**在 1 秒后触发，重新从磁盘加载最新配置并再次 `start`——最多连续失败 `maxRetries` 次（默认 3）。

`stopping` 标志是关键防线：它防止守护进程自己刚杀掉的服务被监视任务重新拉起，否则会陷入无限重启循环。

### 3.5 并发注意点

- 所有共享状态都用 `std::sync::Mutex` 保护，锁的持有范围刻意保持很短（没有任何锁跨 `.await` 持有）。
- `start`/`stop`/`restart`/`force-stop` 都是**同步方法**——只负责派发后台任务并立即返回，因此可以在任意 async handler 中安全调用。
- 开发过程中修复过一个隐蔽陷阱：`if let Some(x) = mutex.lock()...` 会把临时的 `MutexGuard` 生命周期**延长到整个 if 块**，导致对同一把锁再次 `lock()` 时**死锁**（std 的 Mutex 不可重入）。现在所有 pid 查询都先把 pid 拷贝出来、释放锁，再进行后续加锁操作。

## 4. 线程与内存模型

`main.rs` 构建的是 Tokio 多线程 runtime：

- worker 线程数上限为 `min(4, CPU 核数)`——守护进程是 I/O 密集型，4 个 worker 足够
- 每线程栈从默认 2 MiB 降到 1 MiB

这样在多核机器上也能保持较低的内存常驻占用。

## 5. 安全设计

| 层 | 机制 |
|---|---|
| 守护进程 API | HMAC-SHA256 对 `X-Timestamp + body` 签名（共享密钥）；时间戳必须在 5 分钟以内；签名用恒定时间比较 |
| Web UI 登录 | 会话 cookie（`session-id`，24 小时有效，`HttpOnly`）+ CSRF cookie |
| CSRF | `csrf-token` cookie 与 `X-CSRF-Token` 请求头必须同时匹配登录时签发的令牌 |
| 登录限流 | 同一 IP 连续失败 5 次 → 封禁 1 分钟 |
| 响应头 | `X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`、`Referrer-Policy`、禁止缓存 |
| 静态文件 | 拒绝路径穿越（只允许 `Component::Normal` 路径段） |

## 6. 日志系统

每个服务写入 `logs/<name>/current.log`。日期翻转时（`open()`），内容会合并进 `logs/<name>/<YYYYMMDD>.log`，`current.log` 被清空。超过 `retentionDays` 的日期日志会被删除，`current.log` 始终保留。所有写入都是同步的，每行带 ISO 时间戳前缀。

## 7. 系统状态采集

`get_server_status()` 使用 `sysinfo`：

- CPU 使用率在**100 毫秒窗口**内采样（两次 `refresh_cpu_usage()`），对应原版的两次 `os.cpus()` 读取
- 内存总量/已用/空闲 + 使用率百分比
- 系统运行时长、平台（`std::env::consts`）、主机名

## 8. 发布优化

`Cargo.toml` 内置了一套针对体积与速度调优的 release 配置：

```toml
[profile.release]
opt-level = 3       # 最大化代码优化
lto = "fat"         # 跨 crate 链接期优化
codegen-units = 1   # 更好的优化，编译更慢
panic = "abort"     # 不生成 unwind 表 -> 二进制更小
strip = true        # 去除符号表
```

依赖裁剪到只保留实际用到的 feature（例如 Tokio 不再是 `full`，而是精确的 `rt-multi-thread, macros, signal, process, io-util, time, net`；`sysinfo` 去掉基于 rayon 的 `multithread`；`chrono` 只留 `clock`）。这能明显减小二进制体积并缩短编译时间。

## 9. 数据流示例

通过守护进程 API 重启服务 `example`：

1. 收到 `POST /service/control/example`，请求体为 `{"type":"restart"}`。
2. `verify_signature` 用密钥重新计算 `HMAC-SHA256(secret, timestamp + body)` 并与 `X-Signature` 比对。
3. `restart()` 把 `example` 加入 `stopping`，终止当前进程树，然后调用 `start()`。
4. `start()` 启动新的 shell 进程，登记 pid 与启动时间，并派发新的监视任务。
5. 清除 `stopping` 标志；之后若再次异常退出，监视任务仍会按规则自动重启。
