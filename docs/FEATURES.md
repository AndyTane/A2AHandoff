# A2AHandoff 产品功能清单

> 按「当前 V1 已有能力 + 正在补齐的公开版能力」整理，可直接用于 README、产品介绍或 GitHub 项目说明。

---

## 一、产品定位

A2AHandoff 是一个运行在本地 Windows 上的 Agent-to-Agent 交接工具。

当前主要解决：

DeepSeek Harness（DSH） ↔ Claude Desktop

之间的自动任务交接。

它不是新的 Agent，也不负责替代 Claude 或 DSH 做任务，而是负责：

* 识别双方当前会话；
* 监听任务是否完成；
* 获取完整回复；
* 将结果交给另一方；
* 控制自动发送；
* 处理人工介入；
* 防止重复投递；
* 保存本地交接状态和证据。

核心目标：

让 Claude 负责判断、规划和审查，让 DSH 负责确定性执行，两边可以持续交接，而用户不需要反复复制粘贴。

---

## 二、Claude ↔ DSH 双向交接

### 1. DSH → Claude

当 DSH 完成当前任务后：

1. A2AHandoff 监听 DSH 原生 Session。
2. 判断当前轮是否真正结束。
3. 获取 DSH 本轮完整回复。
4. 根据本地记录判断是否已经投递过。
5. 按用户配置的 Claude 交接文案包装内容。
6. 自动定位绑定的 Claude Cowork 会话。
7. 将内容写入 Claude 输入框。
8. 核验写入内容和目标会话。
9. 写入成功后开始发送倒计时。
10. 用户未取消，则自动发送给 Claude。
11. 保存本地发送回执。

不会因为轮询多次而重复发送同一份 DSH 回复。

### 2. Claude → DSH

Claude 完成分析、决策或更新任务文件后：

1. A2AHandoff 监听绑定的 Claude Cowork 会话。
2. 判断 Claude 是否产生了新的完整回复。
3. 确认这条回复属于当前自动交接链路。
4. 判断 DSH 当前是否空闲。
5. 根据用户配置生成发给 DSH 的执行指令。
6. 自动写入 DSH 输入框。
7. 写入成功后启动倒计时。
8. 未取消则自动发送。
9. DSH 开始下一轮执行。

默认不把 Claude 整段回复复制给 DSH。

例如只发送：

读取 DSH_TASKS.MD 并执行。

Claude 的完整回复仅用于：

* 来源识别；
* 去重；
* 会话关联；
* 自动交接判断。

---

## 三、Draft-first 安全发送

A2AHandoff 使用 Draft-first 投递机制。

顺序严格为：

发现新回复 → 写入目标输入框 → 核验草稿 → 开始倒计时 → 自动发送

不是：

先倒计时 → 再写输入框

这样用户在倒计时阶段已经能看到真正准备发送的内容。

另外，倒计时的长度由**写入成功的那一刻**开始计算，所以「准备阶段」花掉的时间不会
偷偷吃掉用户可以取消的窗口。

### 3. 起草即核验

写入之后、倒计时之前，必须先核实：

* 目标 Session 仍是绑定的那一个；
* 页面仍属于目标应用、输入框仍属于目标窗口；
* 输入框里此刻的内容与预期文案**逐字一致**。

任何一项不成立就停止本次发送，并保留草稿。提交前还会再核验一次，因此不存在
「先倒计时、后发现写错了」的窗口。

### 4. 可取消发送倒计时

草稿成功写入后：

* 默认等待 10 秒；
* 主界面显示明显倒计时；
* 用户可以点击“取消本次”；
* 取消后不会自动发送；
* 已经写入的草稿保留在目标输入框中。

倒计时长度支持配置。

### 5. 草稿变更保护

倒计时期间如果检测到：

* 用户修改了草稿；
* 输入框内容变化；
* 会话发生变化；
* 目标窗口变化；

本次自动发送立即失效。

不会拿旧内容继续发送。

---

## 四、自动模式与手动模式

### 6. 自动双向交接

开启自动模式后：

DSH → Claude → DSH → Claude

可以持续交接。

每个方向仍然经过：

* 来源校验；
* 去重；
* 草稿写入；
* readback；
* 倒计时；
* 提交验证。

自动模式不是“无限按 Enter”。

### 7. 手动发送

主界面保留：

* 发送给 DSH
* 发送给 Claude

用户可以在需要时手动接管交接。点击的语义是**「现在就把这一条发出去」**：

* 点击自己指定方向，不受自动优先级影响；
* 点击即确认，因此**不等自动倒计时**，草稿核实通过后立即发送；
* 如果自动交接已经把同一条排进队列并正在倒计时，点击会把它**转成立即发送**
  （保留同一份草稿与投递身份，只去掉等待），而不是被回一句「已经排队」；
* 对已经投递过、或曾经被取消过的那条回复，点击仍然有效——自动去重只是为了
  防止轮询重复投递，不该让用户手动也发不出去；
* 重复点击不会重复创建相同 pending。

安全语义不变：会话关联、目标校验与草稿核验在手动路径上同样强制执行。

### 8. 暂停自动交接

用户可以随时暂停自动模式。

暂停时：

* 监听继续；
* 当前会话状态继续读取；
* 不再自动产生新的发送动作；
* 手动发送仍然可以使用。

---

## 五、人工介入识别

### 9. Claude 人工接管识别

如果用户直接在 Claude 会话中继续聊天：

A2AHandoff 会识别自动链路已经被人工推进。

状态变为：

自动交接已暂停 · 监听仍在

而不是错误显示为“监听停止”。

### 10. DSH 人工介入识别

如果 DSH Session 被用户直接追加任务或出现与自动链路不同的新输入：

A2AHandoff 不会假设仍然是原来的自动任务。

会停止自动续接，避免把错误结果传回 Claude。

### 11. 重新接管

人工讨论结束后：

用户可以通过发送按钮重新选择当前最新结果作为新的交接起点。

之后自动模式重新进入正常链路。

---

## 六、监听与恢复

### 12. DSH 原生 Session 监听

A2AHandoff 直接读取 DSH 本地原生 Session 数据。

包括：

* Session ID；
* 会话标题；
* 当前 turn；
* 当前是否执行中；
* user message 的**序号与哈希**（用于变化检测，正文不出本机）；
* assistant reply（仅针对最后一个已完成或 blocked 且匹配最新人类输入的轮）；
* turn start / turn end；
* blocked / completed；
* ask_user_question；
* goal 状态；
* native sequence。

不依赖屏幕 OCR，也不把整段对话正文读出或落盘。

### 13. Claude Desktop 会话监听

通过 Windows UI Automation 读取 Claude Desktop：

* Cowork Session；
* 当前消息总数；
* 最新用户消息的**索引与哈希**（同上，正文不出本机）；
* 最新 Claude 回复；
* 当前是否生成中；
* 是否处于会话尾部；
* 当前回复是否完整。

绑定身份主要依据：

claude.ai/cowork/<cse_id>

而不是窗口标题。

### 14. 恢复监听

提供：

恢复监听

功能。

用途：

当程序重启、状态基线失效或用户人工介入后，可以重新校准监听位置。

规则：

* DSH 正在执行 → 接续当前轮；
* DSH 当前空闲 → 以当前状态作为基线，等待下一轮。

恢复监听：

* 不重新发送任务；
* 不重新发送历史结果；
* 不删除历史回执。

### 15. 监听状态显示

UI 区分七种监听状态，各自有独立文案，不互相冒充：

| 状态 | 含义 |
|---|---|
| 监听正常 | 正在观察双方会话，没有待办 |
| 当前轮已连接 | 已接上正在进行的那一轮 |
| 等待下一轮 | 基线已就位，等 DSH 下一轮开始 |
| 自动交接已暂停 | 观察继续，但不再自动产生发送 |
| 心跳过期 | runtime 心跳不新鲜，不能声称仍在观察 |
| 会话不匹配 | 绑定的 Session 与 runtime 正在读的不是同一个 |
| 会话不可读取 | 尚未绑定任何会话 |

它们由 runtime 实际发布的数据推导（`watch_age`、`watch_session_matches`、
`dsh_turn`、`reply_turn`、最近一次投递方向），不是固定的界面文案。

同时保证：**「暂停自动交接」不会被显示成「停止监听」**——观察在暂停期间照常进行，
手动发送也照常可用；只有「自动发送」停下来。

不再使用没有信息量的：

0 秒前更新

这类状态。时间只出现在真实的发送倒计时和下次轮询上。

---

## 七、自动轮询

### 16. 自动轮询

A2AHandoff 定期检查 Claude / DSH 是否产生新状态。

轮询周期可配置。

### 17. 下次轮询时间

主界面显示真实 runtime deadline，例如：

下次轮询：2 分 37 秒后

不是 UI 自己简单估算。

轮询完成后自动刷新下一次时间。

---

## 八、Session 绑定

### 18. Claude Session 绑定

用户只需要输入：

cse_...

不需要填写：

* Claude 窗口标题；
* URL；
* PID；
* UI Selector。

A2AHandoff 自动寻找对应 Cowork 会话。

### 19. DSH Session 选择

自动扫描本机 DSH Session。

绑定界面提供下拉列表。

显示：

* 会话标题；
* Session ID 简写；
* 最近 Session 优先。

内部保存完整 Session ID。

### 20. 精确目标校验

消息发送之前重新确认：

* Claude Session 是否仍一致；
* DSH Session 是否仍一致；
* 页面是否仍属于目标应用；
* 输入框是否仍属于绑定窗口。

目标不明确时不发送。

---

## 九、交接文案

### 21. Claude → DSH 文案配置

用户可以自行配置 DSH 执行指令。

例如：

读取 DSH_TASKS.MD 并执行。

可以加入额外约束：

* 不自动续跑 goal；
* 阻塞时结束当前轮；
* 不调用 ask_user_question 让用户代答；
* 把问题和证据写进最终回复。

### 22. DSH → Claude 文案配置

支持：

prefix + DSH 完整回复 + suffix

例如：

DSH 已回复:
[
<DSH 原始结果>
]
如需继续确定性工作，请更新任务文件。

DSH 原始正文不会被改写。

### 23. 无可见内部 Marker

不会向 Claude / DSH 消息中添加：

* delivery ID；
* A2AHandoff ID；
* internal hash；
* hidden marker；
* 自动化编号。

所有关联信息仅保存在本机。

---

## 十、去重与幂等

### 24. DSH 回复去重

同一个 DSH 原始结果即使被多次轮询发现：

只允许交接一次。

### 25. Claude 回复去重

已经交给 DSH 的 Claude 回复不会重复交付。

### 26. Session-aware Delivery ID

投递身份包含：

* 来源回复；
* 来源 Session；
* 目标 Session；
* 方向；
* 本地 fingerprint。

改变绑定后不会把旧 delivery 当成新 Session 的合法任务。

---

## 十一、发送回执与异常保护

### 27. Durable Receipt

每次投递保留本地回执。回执状态分两类：

**终态**——该投递到此为止，永远不会再自动或手动重发：

* `sent`：已核实送达；
* `submit_uncertain`：已点击发送但无法确认对方是否接受。

**未送达**——草稿写过但没出去，保持可重试：

* `draft_write_attempted`：刚开始写入；
* `draft_ready`：草稿已写入并核实，等待倒计时；
* `cancelled_before_send`：草稿留下但本次被取消；
* `draft_unverified`：写入后无法核实。

另有两个记账用的名字，语义相同但存放位置不同：`queued` 是 pending 的阶段名（在
request/workflow 里）；`superseded_by_user` 记录在 `last_delivery` 上，表示旧稿因人工
介入而作废。

用于程序重启后的状态恢复与防重复。**注意「取消」只阻止自动重发，不阻止手动点击**：
取消过的回复仍然可以由用户点按钮发送。

### 28. 不确定发送保护

如果已经执行 Send，但无法确认目标 UI 是否接受：

进入：

submit_uncertain

不会自动再次点击发送。

### 29. 延迟回执恢复

如果发送实际上成功，只是 Claude UI 更新较慢：

A2AHandoff 可以在后续轮询中通过：

* Session 一致；
* 用户消息序号；
* 输入框已经清空；
* native Send 已执行；

重新确认发送成功。

避免把自己发送的消息误判成用户人工介入。

### 30. 输入框已有内容保护

如果目标输入框已经存在用户草稿：

A2AHandoff 不覆盖。

进入等待状态。

用户清理后可以重新发起。

### 31. Clipboard 保护

DSH 输入框以受保护的 clipboard paste 写入（按输入框身份选择该方式，不是因为
SetValue 报错才退回）。过程包括：

* 精确聚焦，并核实窗口已在前台且确实获得键盘焦点；
* 在聚焦前、以及粘贴前各做一次目标身份复核；
* 写入；
* 完整 readback，与预期文案逐字比对；
* 尽可能恢复原剪贴板。

Claude 一侧使用输入框自己的发送控件。

不会直接使用 Enter 作为默认输入策略——适配器里唯一的键盘注入是粘贴用的 Ctrl+V。

---

## 十二、配置系统

### 32. Runtime 环境配置

公开版支持配置：

* DSH data home；
* DSH Web origin；
* DSH 浏览器进程；
* workspace 校验；
* Claude host；
* DSH 页面标题辅助特征；
* 自动轮询周期；
* 自动发送倒计时。

### 33. 基础 / 高级设置分层

基础设置面向普通用户：

* DSH 数据目录；
* DSH Web 地址；
* 轮询周期；
* 发送倒计时。

高级设置：

* Browser processes；
* Workspace；
* Claude Host；
* DSH 页面识别特征。

避免把 UIAutomation selector 等内部实现暴露给用户。

### 34. First-run 初始化

首次使用支持：

* 自动检测 DSH；
* 自动建立本地 config；
* 生成默认 message templates；
* 创建安全 placeholder bindings；
* 用户随后只需绑定 Claude / DSH Session。

目标是不要求用户手写 JSON。

### 35. 配置迁移

兼容早期配置。

旧字段如：

* task_file
* mode
* next_poll_at_ms

可以被识别并忽略。

不会因为旧配置存在而启动失败。

---

## 十三、诊断与可观测性

### 36. 状态面板

主界面可看到：

* Claude Session；
* DSH Session；
* DSH 当前轮次；
* DSH 是否执行中；
* 自动交接状态；
* 监听状态；
* 当前 handoff 状态；
* 下一次轮询。

### 37. 诊断日志

记录关键状态变化，例如：

* runtime started；
* delivery draft ready；
* delivery receipt；
* manual intervention；
* listener restored；
* delayed submit recovered；
* runtime stopped。

方便定位为什么没有发送或为什么自动暂停。

### 38. 本地事件与 Workflow

本地保存：

* workflow；
* request；
* receipt；
* events；
* runtime state。

用于：

* 故障诊断；
* 幂等；
* 重启恢复；
* 防止历史消息重新投递。

---

## 十四、开源与隐私

### 39. 全本地运行

A2AHandoff 自身不需要：

* A2AHandoff 云服务器；
* A2AHandoff 账号；
* Claude API Key；
* DSH API Key。

它操作的是用户本地已经登录的客户端。

### 40. 本地隐私隔离

以下数据不进入 Git：

* Session ID；
* conversation-derived content；
* runtime request；
* receipts；
* workflow；
* event logs；
* 用户本地路径。

### 41. 开源泄漏检查

提供 release audit。

发布前检查：

* 用户路径；
* 真实 Session；
* runtime 数据；
* secret；
* token；
* build artifact；
* backup。

### 42. 自动 CI

公开仓库支持 CI 检查：

* Rust format；
* Rust tests；
* PowerShell parser/correlation tests；
* staged delivery integration；
* occupied-draft safety tests。

测试使用 fake editor / fake session，不向真实 Agent 发消息。

---

## 十五、当前平台支持

### 43. Windows

当前正式实现：

* Windows Native UI；
* Windows UI Automation；
* PowerShell adapter；
* Edge / Chrome DSH Web；
* Claude Desktop Windows。

### 44. 架构可扩展

核心状态机与平台/client adapter 分层。

当前结构包括：

* handoff-core
* handoff-runtime
* handoff-adapter-api
* Claude adapter
* DSH adapter
* Windows input adapter
* Windows native UI

未来可以增加新的 Agent 或平台 adapter，而不把具体客户端逻辑写进核心状态机。

---

## 一句话功能总结

A2AHandoff = 一个本地、安全、有回执、有去重、支持人工接管的 Claude ↔ DSH 双向任务交接控制器。

它解决的不是“让两个 AI 聊天”，而是：

让一个 Agent 的执行结果可靠地成为另一个 Agent 的下一步输入，同时保留用户随时介入和停止自动化的控制权。
