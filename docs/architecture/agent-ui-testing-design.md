# Agent 自动化 UI 测试工作流设计

日期：2026-07-02
分支：`feat/agent-ui-testing`
状态：已批准

## 背景与目标

Puffer desktop（`apps/puffer-desktop`，Svelte 5 + Tauri 2）已具备完整的测试地基：

- Playwright 1.59.1 + 30+ 个 `tests/*-ui.spec.ts`（桩掉 `__TAURI_INTERNALS__` 的 mock 层）
- `tests/real-daemon-ui.spec.ts` 内嵌的 `DaemonFixture`：spawn 真实 `target/debug/puffer` daemon（隔离临时 HOME/workspace + mock provider），通过 URL query（`corbinaBackend`/`corbinaToken`）把 handshake 注入前端——**前后端走 HTTP，浏览器即可触达真实后端，无需驱动 WKWebView**
- Storybook（端口 6006，含 a11y addon）
- `scripts/ci-gates.sh` 回归门槛

目标：在此地基上整合一套 **agent 驱动的自动化测试工作流**，用"agent 探索 → 产出报告 → 固化为回归 spec"替代人工测试，缩短人工测试时间。测试资产（spec）复利增长是省时间的根本机制。

约束：不考虑向后兼容；只考虑长期收益、稳定和性能；防止过度设计。

## 场景路由（核心决策表）

| 场景 | 工具 | 产出物 | 替代的人工工作 |
|---|---|---|---|
| 新功能验收（功能流程探索） | agent-browser + `agent-app.mjs` 隔离环境 | 探索报告 + 固化的 `*-ui.spec.ts` | 手动点一遍新功能各路径 |
| UI/UX 审查（样式改动、视觉状态遍历） | agent-browser 截图 + agent 视觉判断；组件级走 Storybook:6006 | 视觉问题清单 + `toHaveScreenshot()` 基线 spec | 人眼逐状态检查布局/暗色/hover |
| Bug 深挖（daemon 协议、网络、console） | playwright-mcp（网络拦截、trace、console） | 根因分析 + 最小复现 spec | 手动开 DevTools 抓包排查 |
| 回归防护（每次提交） | repo 的 `@playwright/test`（无 agent、无 LLM） | ci-gates 通过/失败 | 手动回归测试 |

路由原则：高频迭代用 agent-browser（token 效率约 4 倍于 MCP 方案，且底层同为 Playwright，role-based 定位可 1:1 翻译成 spec）；深度调试用 playwright-mcp（网络/trace 能力）；CI 永远不依赖 agent。

工具选型依据（2026-07 调研）：agent-browser（Vercel 官方）与 playwright-mcp（Microsoft 官方，34k+ stars）均为主流稳定工具；实测 10 步任务 token 消耗约 27k vs 114k，故探索层默认 agent-browser。

## 架构与数据流

```
┌─ 探索模式（本地）──────────────────────────────────────┐
│ scripts/agent-app.mjs                                   │
│   ├─ 起隔离 daemon（临时 HOME/workspace，mock provider）│
│   ├─ 复用正在跑的 Vite:1420，没有则自己拉起              │
│   └─ 打印: http://127.0.0.1:1420/?skipOnboarding=1      │
│            &corbinaBackend=<url>&corbinaToken=<token>   │
│                          ↓                              │
│ agent-browser / playwright-mcp 打开该 URL（工具无关）    │
│   → 真实前端 + 真实 Rust 后端，与用户 dev 数据完全隔离    │
└─────────────────────────────────────────────────────────┘
┌─ 固化模式（CI）────────────────────────────────────────┐
│ agent 把发现写成 tests/*-ui.spec.ts                      │
│   → import tests/support/daemonFixture.mjs              │
│   → npm run test:desktop-ui → ci-gates                  │
└─────────────────────────────────────────────────────────┘
```

关键机制：handshake 通过 URL query 注入，同一个 Vite 实例可同时服务用户的 dev app 和 agent 的隔离实例，互不干扰，无需第二端口。

## 组件（4 个改动点，约 200 行新代码，零新增运行时依赖）

1. **`tests/support/daemonFixture.mjs`**
   从 `real-daemon-ui.spec.ts` 抽出 `DaemonFixture` 与 provider mock（OpenAI/Anthropic），改写为 `.mjs` + JSDoc 类型；原 spec 改为 import，不留兼容层。
   选 `.mjs` 而非 `.ts`：CLI 脚本与 TS spec 均可直接 import，不引入 tsx/ts-node 依赖。

2. **`scripts/agent-app.mjs`**（约 80 行）
   - `--provider mock|real`：默认 mock；`real` 从 `RELAYDANCE_API_KEY` 环境变量读密钥、指向 relaydance 网关，密钥缺失即 fail-fast
   - 复用/拉起 Vite，最后打印带 handshake 参数的完整 URL（人和任何 agent 工具都能直接打开）
   - SIGINT 时杀 daemon、清临时目录

3. **`.mcp.json`**
   加 playwright-mcp（`npx @playwright/mcp@latest`），角色限定为 Bug 深挖场景；MCP 工具延迟加载，不用不吃 token。

4. **`AGENTS.md`**
   新增一节：场景路由表 + 探索→固化工作流约定（spec 放 `tests/`、role-based selector 风格、UI/UX 问题优先固化为 `toHaveScreenshot()` 基线、必须过 `npm run test:desktop-ui` 才算固化、组件级 UI 审查首选 Storybook）。

## 错误处理

- `target/debug/puffer` 不存在 → 报错并提示 `cargo build -p puffer-cli`
- handshake 15s 超时 → 转储 daemon stderr 后退出
- Vite 拉起失败 → 明确报错，不静默重试
- `--provider real` 且 `RELAYDANCE_API_KEY` 缺失 → fail-fast
- 一律 fail-fast，不做自动恢复（开发工具，失败要显眼）

## 测试策略

- 抽取重构的正确性：由现有 `real-daemon-ui.spec.ts` 继续通过来保证（它是抽取模块的第一个消费者）
- `agent-app.mjs`：一个最小 smoke spec——启动脚本、解析输出 URL、请求 daemon 健康端点、确认退出时清理
- UI/UX 探索产出的视觉问题优先固化为 `toHaveScreenshot()` 基线 spec

## 明确不做（防过度设计边界）

- 进程内插桩（Victauri 式自研）：Puffer 前后端走 HTTP handshake，浏览器已能触达真实后端，此类方案解决的是 Puffer 不存在的问题
- agent 探索进 CI：CI 只跑固化 spec，不依赖 LLM
- Claude Code 技能封装：流程跑顺（约两周）后按真实痛点再评估
- 截图基线之外的视觉回归服务
- 第二 Vite 端口隔离
- 原生壳层（Dock 角标、窗口、菜单）自动化：无主流方案，维持 OS 级人工验证

## 决策记录

| 决策 | 结论 | 依据 |
|---|---|---|
| 首要用途 | 探索+固化闭环 | 测试资产复利增长，长期收益最大 |
| LLM provider | mock 默认，real（relaydance）可选 | 确定性、免费、快；全链路验证按需 |
| CI 范围 | 仅固化 spec | CI 稳定性最高，不依赖 agent/LLM |
| 入口形态 | 独立 CLI 脚本 | 人和 agent 通用，工具无关 |
| 探索默认工具 | agent-browser | token 效率 ~4 倍、已安装零配置、固化映射与 playwright-mcp 等价 |
| 深挖工具 | playwright-mcp | 网络拦截/trace/console 能力 |
