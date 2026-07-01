# Automation 设计交接

## 目标

桌面端 Automation tab 是一个以提示词为起点的入口，用来创建和管理简单的
automation，不引入画布。当前实现只覆盖 UI，使用本地 Svelte state 模拟创建、
已保存 automation、可编辑设置和运行记录，方便在接入后端合约前先确认产品形态。

设计意图：

- 创建路径保持线性，用户可以 review 后再保存。
- 视觉风格贴近 Puffer 现有桌面端 UI，保持紧凑、克制。
- 所有可见文案都面向用户，不使用内部实现口吻。
- 避免节点图、无限画布控制、内部状态说明等复杂表达。

## 当前入口

### 侧边栏

Automation 已作为桌面端 shell 的侧边栏入口出现。页面标题为 `Automation`。

### 首页输入框

首页以 `Create an automation` 开始，并复用 Puffer 现有 composer 结构：

- 附件按钮。
- 模型选择器。
- Fast toggle。
- Thinking 选择器。
- Permissions 选择器。
- 发送按钮。

输入框 placeholder 引导用户用自然语言描述想自动化的事情。提交后会进入完整创建页；
如果提示词命中当前已支持的模式，会自动预填部分配置，等待用户 review 后保存。

### 列表区域

输入框下方是一个 segmented control：

- `Your automations`
- `Template Library`

`Your automations` 初始为空。空态文案是
`创建你的第一个automation，处理重复的工作流`，主按钮是 `create automation`。
右上角工具栏按钮是 `new`。

`Template Library` 展示模板卡片。点击模板会打开创建页，并带入预设名称、说明和
trigger。

## 当前创建路径

创建页是完整页面，不是 modal，也不是侧边面板。

顶部栏：

- 返回 `Automations` 的面包屑。
- `Create New` 标签。
- `Cancel`。
- `Save`。

页面主体：

- `Name`
- `Triggers`
- `Instructions`
- `Tools`
- `Cloud Agent Environment`

点击 Save 会创建一个本地 automation 卡片，并返回首页。点击 Cancel 会直接返回首页，
不会创建卡片。

### 自然语言预填

当前 prompt parser 会识别几类宽泛关键词：

- Pull request 相关提示词会生成 `PR review draft`。
- Calendar、invite、RSVP、meeting 相关提示词会生成 `Calendar RSVP`。
- Gmail 或 email 相关提示词会生成 `Email reply draft`。
- Slack、message、reply 相关提示词会生成 `Reply draft`。
- Daily、weekday、morning、digest、every 相关提示词会生成 `Morning digest`。

这部分预填是本地启发式逻辑，只用于让 UI review 路径看起来更接近真实使用。

### 模板

当前模板：

- `Review PRs`
- `Reply drafts`
- `Calendar RSVP`
- `Morning digest`

每个模板都会映射到名称、说明、图标和初始 trigger。

## 当前 Trigger 模型

Trigger 以紧凑的句子式 row 展示。当前选项包括：

- `Every day at` `09:00`
- `Custom schedule` `Cron`
- `PR opened in` `Select repos` `by` `Anyone`
- `Draft opened in` `Select repos`
- `Comment added in` `Select repos`
- `Label changes in` `Select repos`

已添加的 trigger 可以通过 trigger picker 修改，也可以在 row 上删除。点击 picker 外部
会关闭 trigger picker。

当前限制：

- 虽然 UI 显示的是 `Add Trigger`，但 state 里目前只表示一个 trigger。
- Trigger 的文案和 target 仍然是 mock copy，后续需要替换为现有 connector catalog
  里的真实 trigger 名称、来源 app、事件类型和必填配置。

## 当前 Tool 和 MCP 模型

Tool 按 app 的 API capability 粒度选择。一个 app 可以提供多个可选能力，每个能力都
会成为单独一行。

当前 app 和能力：

- GitHub: `Watch Pull Requests`, `Comment on Pull Request`, `Update Commit Status`
- Slack: `Read Slack Channels`, `Send to Slack`, `Reply in Slack Thread`
- Gmail: `Read Gmail Threads`, `Create Gmail Draft`, `Apply Gmail Label`
- Google Calendar: `Read Calendar Events`, `Check Availability`, `Draft RSVP`
- Linear: `Read Linear Issues`, `Create Linear Issue`, `Comment on Linear Issue`
- Notion: `Search Notion`, `Create Notion Page`, `Update Notion Page`

带目标或模式的能力会展示 inline target chip，例如
`Send to Slack` `to` `#teams`。target chip 当前会在本地候选项之间切换。

已选择的 tool 可以编辑或删除。点击 picker 外部会关闭 tool picker。

`Memories` 始终作为内置 context tool 展示。

当前限制：app 名称和 API capability 文案仍然是 mock copy，后续需要替换为已有
connector 和 MCP server 暴露的真实 action，包括必填输入、可选 target、权限要求和
连接就绪状态。

## 当前详情页

点击已保存的 automation 卡片会打开完整详情页。

顶部栏：

- 返回 `Automations` 的面包屑。
- `Test Run`。
- `Save`。
- 带 `Delete` 的更多菜单。

身份区域：

- 可编辑 automation 名称。
- Active toggle。
- Owner 文案，目前是 `You`。

Tab：

- `Settings`
- `Run History`

### Settings Tab

Settings 复用创建页的控制：

- Trigger row 和 trigger picker。
- Instructions 输入区域。
- Tool rows 和 tool picker。

修改会先保存在本地编辑态里，用户点击 `Save` 后才更新本地卡片，包括标题、描述、
状态、trigger 摘要、已选 tools、启用状态和图标。

### Run History Tab

没有运行记录时展示 `No runs yet`。

点击 `Test Run` 会创建一条本地 history：

- Title: `Test run`
- Status: `Waiting for review`
- Started: `Just now`
- Duration: `-`
- Summary: `Puffer is checking the current configuration.`

点击后也会自动切换到 `Run History` tab。

### 删除

更多菜单会打开一个紧凑操作菜单。点击 `Delete` 会删除当前本地 automation，并返回首页。

## State 边界

当前实现位于 `apps/puffer-desktop/src/lib/screens/Automation.svelte`。

重要本地 state：

- `screenMode`: `home`, `new`, `detail`。
- `savedAutomations`: 本地已保存的用户 automation。
- `selectedAutomationId`: 当前选中的详情 automation。
- `automationName`, `automationPrompt`, `automationTrigger`, `selectedTools`,
  `automationEnabled`: 当前草稿或详情编辑态。
- `activeAutomationLibraryTab`: 首页列表 tab。
- `activeAutomationDetailTab`: 详情页 tab。
- `triggerMenuOpen`, `toolMenuOpen`, `automationActionMenuOpen`: 弹窗状态。

当前还没有接入后端持久化、daemon RPC、connector 执行或真实调度。

## 已补上的交互

当前已经实现：

- 从侧边栏打开 Automation。
- 从首页 prompt 创建。
- 从 `new` 创建。
- 从模板卡片创建。
- 保存本地 automation。
- 取消创建。
- 打开已保存 automation 的详情页。
- 在详情页重命名 automation。
- 在详情页编辑 instructions。
- 在详情页切换 active 状态。
- 保存详情页修改。
- 添加、编辑、删除 trigger row。
- 添加、编辑、删除、切换 tool target。
- 在 tool picker 里选择 app API capability。
- 点击外部关闭 trigger 和 tool picker。
- 在 `Settings` 和 `Run History` 之间切换。
- 创建本地 test-run history。
- 打开更多菜单并删除本地 automation。
- 当前页面自有可见文案尽量使用 automation 语义，避免多余的 automation 堆叠。

## 还没补上的交互

### 创建和编辑

- 一个 automation 内支持多个 trigger。
- 把 mock trigger 文案替换成 connector 支撑的真实 trigger 选项，包括来源 app、
  event 名称、必填输入和配置状态。
- 把 mock tool 和 MCP 文案替换成真实 connector actions 和 MCP tools，包括能力名称、
  必填输入、可选 target 和权限要求。
- Trigger 专属配置面板，例如 repo picker、cron editor、contact picker、calendar picker、
  label picker。
- Trigger target chip 的手动编辑。
- 创建页和详情页里的独立模型选择器。
- `Use Configured Environment` 之外的 environment 详情。
- 脏状态、未保存离开提示、保存成功反馈。
- 使用 Escape 关闭弹窗。
- Trigger 和 tool 菜单内更完整的键盘导航。
- 点击外部关闭更多菜单。
- Trigger 搜索。
- Trigger 搜索空结果状态。
- Picker 打开时，更清楚地区分“新增 tool”和“编辑已有 tool”。
- Duplicate automation。
- 卡片上的 archive 或 pause 操作。

### 首页和列表

- 搜索或过滤已保存 automations 和 templates。
- 按最近更新、名称、状态或来源排序。
- 保存卡片上更明确的 status chips。
- 卡片级快捷操作。
- Template 分类。
- 打开创建页前的 template 详情预览。
- 导入或粘贴已有 automation 配置。

### 详情页

- 真实运行记录，包含结果、来源事件、耗时和 approval metadata。
- Run history 过滤。
- Run history 详情 drawer 或 timeline。
- Test run 输入，例如选择 sample event 或历史消息。
- Test run 结果预览，包括生成的 draft、上下文和错误。
- Active toggle 的保存行为和 pending 状态。
- 删除确认。
- 对危险或不可用操作展示 disabled 状态。
- Owner 选择器或分享信息。
- 最近保存时间。

### Review 和 Approval

- Review inbox。
- Pending draft review 详情页。
- 可编辑的 proposed action 或 draft output。
- Approve、reject、snooze、edit 决策控件。
- Outward action 的 destination preview。
- Reject reason 记录。
- 清晰的 audit trail，显示谁在什么时候批准了什么。

### 后端和合约

- Automation 持久化存储。
- 用于 create、update、delete、test run、run history、enable/pause 的 daemon RPC。
- Connector 支撑的 trigger discovery。
- Connector 支撑的 tool capability discovery。
- Permission 和 credential readiness 状态。
- 后端合约返回的 validation errors。
- 真实执行调度。
- 真实 dry-run execution。
- Workspace 或 team policy 约束。

## 建议的下一步设计

1. 给创建页和详情页补脏状态与保存反馈。
2. 优先补 GitHub repo 和 schedule 相关的 trigger 专属配置。
3. 设计 test-run preview 路径，包括 sample input 和 generated output。
4. 设计 review inbox 和 approval 详情页。
5. 定义 saved automation、trigger config、tool config、test run 和 run history 的后端合约。
6. 补删除确认，以及 duplicate/archive 操作。

## 验证资产

当前 UI 覆盖位于 `apps/puffer-desktop/tests/automation-ui.spec.ts`。

测试覆盖：

- Prompt-first home。
- `Your automations` 空态。
- Template library。
- Builder 布局和控件。
- Trigger 和 tool picker 行为。
- Capability-level tool selection。
- 保存卡片创建。
- 详情页 settings。
- Run history 空态和 test-run 状态。
- 更多菜单里的 delete 可见性。
- Segmented-control 背景对比度。
