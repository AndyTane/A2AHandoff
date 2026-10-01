# 功能清单实现状态核对

对照 `docs/FEATURES.md` 的 44 项逐条核对代码，记录**实现状态**与**描述偏差**。

核对方式：读源码 + 对活会话跑只读探针（DSH `observe.mjs`、Claude `observe.ps1`），不以 UI 文案是否存在作为实现证据。

日期：2026-10-01。代码基线：`config-public-v1`。

---

## 一、结论摘要

- **绝大多数能力真实存在**，包括 DSH 原生监听、Claude UIA 监听、Draft-first 投递、精确目标校验、去重、回执、配置系统、CI。
- **2 项描述明显强于实现**（第 12、13 项：两个 observer 都不输出用户消息**正文**，只输出索引与哈希）——**已把清单措辞改准**。
- **1 项当时列了 7 个状态、只有 1 个真实存在**（第 15 项）——**七个状态已全部实现**，清单同步改为实际状态表。
- **1 项存在 UI 自相矛盾**（第 9 项：横幅说「监听仍在」，徽标说「已停止监听」）——**已修复**。
- **5 项在今天之前是坏的**（第 3、7、24/25、27 项），均已修复并有测试锁定。

### 2026-10-01 第二轮：清单与实现现已对齐

| 项 | 处理 |
|---|---|
| 12 / 13 | 清单改为「user message 的序号与哈希（正文不出本机）」，与两个 observer 的实际输出一致。这是个**有意的隐私取舍**，不是缺失功能。 |
| 15 | 七个监听状态全部实现（`ListenerState`），由 `watch_age` / `watch_session_matches` / `dsh_turn` / `reply_turn` / 最近投递方向推导；卡片徽标不再硬编码「监听中/未监听」。 |
| 3 | 编号缺口补上（「起草即核验」），并补记倒计时从**写入成功那刻**起算。 |
| 7 | 补记手动点击的四条语义：自选方向、不等倒计时、可把自动排队转为立即发送、对已投递/已取消的回复仍然有效。 |
| 27 | 回执状态按「终态 / 未送达可重试」重新分类，并注明 `queued` 与 `superseded_by_user` 的存放位置不同。 |
| 31 | 说明剪贴板是 DSH 的**默认**写入方式（按输入框身份选择），并补上聚焦前/粘贴前的两次身份复核与 readback。 |


---

## 二、今天修好的（此前不成立）

| 项 | 当时的问题 | 修复提交 |
|---|---|---|
| 3 取消本次 | 取消会把该回复的 fingerprint 记成「已投递」，之后手动发送永远报「已经投递过」 | `2ab3dd0` |
| 3 取消本次（第二处） | 取消还会把该 delivery 的**回执**写成终态，`drive()` 见到非 `draft_ready` 回执即拒绝——即使手动点击也报「已有投递记录，未重复发送」 | 本轮 |
| 7 手动发送 | 基线把自己要发的那条排除在外，点击落到「没有新的完整回复」而不做任何事 | `0fee879` |
| 24 / 25 去重 | 去重账本把 `cancelled` 当作已投递，是上一条的直接原因 | `2ab3dd0` |
| 27 Durable Receipt | 回执校验不通过会**直接终止 runtime**，草稿留在输入框无人投递 | `cf2c63f` |

另一处 UI 自相矛盾已在第五节说明并修复（第 9 项）。

**第 3 项现在的完整语义**（三处曾各自出错，现已一致）：

- **「取消」阻止自动重发**——自动路径不会复活被取消的回复；
- **「取消」不阻止手动发送**——点击是用户的新决定，可以重发；
- **回执的终态语义**——只有 `sent` 与 `submit_uncertain` 是终态（真的出去过，必须永久阻止重发）；其余状态（`cancelled_before_send`、`draft_write_attempted`、`draft_unverified`、`send_attempted`）都记录「草稿写了但没发出去」，必须保持可重试。

---

## 三、描述强于实现（**已改准**）

### 第 12 项 — DSH 原生 Session 监听

其余 11 个子项都真实存在且已实测：

| 子项 | 状态 | 依据 |
|---|---|---|
| Session ID / 标题 / 当前 turn / 是否执行中 | ✅ | `observe.mjs:43-47` |
| user message | ⚠️ **只有 `user_seq` + `user_hash` + `user_request_id`，没有正文** | `observe.mjs:28`、`:45` |
| assistant reply | ✅ 仅对「最后一条且匹配最新人类输入的 completed/blocked 轮」输出 | `:31`、`:35`、`:41-42` |
| turn start / end | ✅ | `:25-26`、`:35` |
| blocked / completed | ✅ 白名单 `['completed','blocked']` → `result.reason`；实测返回 `reason:"completed"` | `:35`（`blocked` 见 DSH `types.d.ts:174`） |
| ask_user_question | ✅ → `pending_question` + `last_question_answer_seq` | `:33-34` |
| goal 状态 | ✅ `goal/change` → `goal` + `goal_seq`；实测 `goal_seq=828` | `:27` |
| native sequence | ✅ `last_seq`/`user_seq`/`open_seq`/`goal_seq`/`end_seq` | `:43-47` |

**建议措辞**：把「user message」改为「user message 序号与哈希（正文不出本机）」。这个取舍是合理的——变化检测只需要哈希，正文不外泄反而更符合第 40 项的隐私目标。

### 第 13 项 — Claude Desktop 会话监听

| 子项 | 状态 | 依据 |
|---|---|---|
| Cowork Session；身份按 `claude.ai/cowork/<cse_id>` 而非窗口标题 | ✅ | `observe.ps1:40`、`:15-16`；同一规则 `draft-context.ps1:41` |
| 当前消息总数 | ✅ `ui_total_messages` | `:111` |
| 当前是否生成中 | ✅ `state=='running'`（Stop 按钮探针） | `:66`、`:71` |
| 是否处于会话尾部 | ✅ `tail_present` | `:65`、`:111` |
| 当前回复是否完整 | ✅ `reply_available`（尾部 + "Claude finished the response" + 最终 Message actions 工具条） | `:68-71`、`:111` |
| 最新 Claude 回复 | ✅ `reply_text`（需 `-IncludeReply` 且 state=replied） | `:112` |
| 最新用户消息 | ⚠️ **只有 `latest_user_index` / `latest_user_hash` / `latest_user_body_hash`，没有正文** | `:111`（`:90` 读到的文本只用于哈希） |

**建议措辞**：同第 12 项，注明只输出索引与哈希。

---

## 四、第 15 项：七个监听状态（**已全部实现**）

清单列了 7 个监听状态。实际代码里的状态文案是另一套：

| 清单所写 | 代码中是否存在 |
|---|---|
| 自动交接已暂停 | ✅ `main.rs:499`、`main.rs:511`、`view.rs:668` |
| 监听正常 | ❌ 不存在（最近的是 `监听中` `view.rs:1071`） |
| 当前轮已连接 | ❌ 不存在 |
| 等待下一轮 | ❌ 不存在（最近的是 `已恢复监听：等待第 N 轮开始` `main.rs:257-264`） |
| 心跳过期 | ❌ 不存在（最近的是 `监听正在启动或已中断` `view.rs:781`） |
| 会话不匹配 | ❌ 不存在 |
| 会话不可读取 | ❌ 不存在（最近的是 `会话尚不可读取` `main.rs:608`、`当前会话暂不可读取` `main.rs:756`） |

实际存在的监听状态是：`V1 运行中` / `V1 已停止监听`（`view.rs:950-953`）、`监听中` / `未监听`（`:1070-1073`）、`监听正在启动或已中断`（`:781`）。

**最后一条宣称是成立的**：全部代码里没有 `秒前` 这类字符串，「0 秒前更新」确实不存在；显示秒数的地方是真实的发送倒计时（`view.rs:691`、`:1364`）。

**建议**：把这一项改成实际状态清单，或明确写为规划中。

---

## 五、第 9 项：UI 自相矛盾（已修复）

**问题**：`paused_by_user` 阶段下

- 横幅：「自动交接已暂停 · 监听仍在」（`main.rs:499` → `view.rs:828-838`）
- 但同一个快照里 `ui.listening == false`（`view.rs:616-628`），于是标题栏徽标渲染成 **`V1 已停止监听`**（`view.rs:950-954`），DSH 卡片徽标渲染成 **`未监听`**（`view.rs:1070-1074`）

同一个界面上两处说法互相打脸，而且**「已停止监听」是错的**——runtime 在 `paused_by_user` 里照样每拍读两个会话、照样刷新状态、手动发送也照样可用。暂停的只是**自动发送**。

**修复**：`listening` 现在表示「runtime 是否仍在观察两个会话」，只在 `unclaimed`（尚未绑定任何会话）时为假；「自动发送是否开启」本来就由 `auto_on` 单独表达。原来的测试把 `!listening` 当作 `paused_by_user` 的正确行为锁了下来，已改为断言 `listening == true` 且仍提供「恢复监听」。

---

## 六、安全类核对（第 5、24–31 项）

| 项 | 结论 | 依据 |
|---|---|---|
| 5 草稿变更保护 | ✅ **四个子项全覆盖** | 草稿被改/输入框变化 → `Test-ExactDraft`（`composer-draft.ps1:28,53,60`）抛 `DRAFT_EDITED_SEND_CANCELLED`，回执改写为 `cancelled_before_send`（`draft-flow.ps1:89`）；会话变化 → runtime 倒计时中清 pending（`main.rs:897-908`）+ 适配器复验（`draft-context.ps1:73,75`）；目标窗口变化 → `TARGET_CHANGED`（`draft-context.ps1:11`） |
| 24 DSH 回复去重 | ✅ | 只有 `sent` 记账（`main.rs:287-289`）；`dsh_after_seq` 水位线排除同一 `end_seq`（`:350,:540`）；id 由 binding+方向+内容确定（`:409`），已有回执即阻止重发（`staged_delivery.rs:190-196`） |
| 25 Claude 回复去重 | ✅ | 同一 `consumed()` 内容哈希闸门（`main.rs:401-403`）；`claude_floor` 推进到已投递回复（`:549` → `:352-356`）；planner 账本阻止同 fingerprint（`lib.rs:157-166`） |
| 26 Session-aware Delivery ID | ✅ | id = `hash(binding_key\|direction\|source_hash)`（`main.rs:409`），`binding_key = hash(bindings.json)`（`:59-61`）；改绑定会丢弃 workflow 账本（`:664-666,711-719`） |
| 28 不确定发送保护 | ✅ | 全程只有**一次** `Invoke`（`composer-draft.ps1:61`），且在有持久意图回执之后（`draft-flow.ps1:64`）；无法确认即写 `submit_uncertain` 并抛 `SUBMIT_UNCONFIRMED_NO_AUTOMATIC_RETRY`（`:81`）；恢复流程是只读的（`recover-uncertain.ps1:1,22-26`） |
| 29 延迟回执恢复 | ✅ **四个信号都用到了** | 会话一致（`submit-evidence.ps1:5`）、用户序号 `== reply_index+1` 且 `> claude_user_index`（`:6-7`）、输入框已清空（`:4`，来自 `recover-uncertain.ps1:24-26`）、native Send 已执行（`:4`）；恢复在人工介入检查**之前**运行（`main.rs:875-883` vs `:884`），并把观察到的索引采纳为新锚点（`:592-597,541-543`），因此不会把工具自己发的消息误判为人工介入 |
| 30 输入框已有内容保护 | ✅ | 写入前拒绝（`composer-draft.ps1:31` `DRAFT_OCCUPIED_PRESERVED`；附件 `:33`）；runtime 清 pending 进 `hold_preparation` 且不消费账本（`staged_delivery.rs:101-116`）；占用检查之前不写回执，故重试路径开放（`draft-flow.ps1:46-50`）。**注意**：清空后重新发起需要点一次按钮（`hold_preparation` 已离开 `waiting_*`） |
| 31 Clipboard 保护 | ✅ **四个子步骤齐全** | 仅 DSH 用剪贴板（`composer-draft.ps1:35-40`）；精确聚焦含前台/焦点/`HasKeyboardFocus` 校验（`dsh-keyboard-input.ps1:20-33`）；聚焦前与粘贴前各做一次身份复核（`:24,:43`）；写入 + 完整 readback（`:46`、`composer-draft.ps1:38`）；`finally` 中尽力恢复剪贴板（`:48-50`）。**全部适配器里唯一的 `SendWait` 是 `^v`，没有任何 Enter** |

### 两处实现与描述的细微出入

1. **第 31 项**：粘贴分支是按 DSH 输入框的**身份特征**选择（className `_input$` / 文档标题），**不是**「SetValue 失败后才改用剪贴板」。因此形态不匹配的 DSH 编辑器仍会走 SetValue 分支（`composer-draft.ps1:44`）。清单已改为「对 DSH 输入框使用受保护的剪贴板粘贴」，不再声称是失败后的回退。
2. **第 27 项**：六个状态里只有 `draft_ready` / `sent` / `submit_uncertain` 是真正的**回执**状态；`queued` 只作为 `pending.stage` 存在于 request/workflow，`cancelled` 只作为账本条目，`superseded_by_user` 只作为 `last_delivery`。语义都在，但不在同一个文件里——措辞宜说明这一点。
3. `draft-context.ps1:1` 的注释「paste/Enter」已过时（没有 Enter 路径）。

---

## 七、已核实无误的项

| 项 | 依据 |
|---|---|
| 12 其余 11 子项 | 见上表 |
| 13 其余 6 子项 | 见上表 |
| 14 恢复监听 | `main.rs:244-265`（busy → `open_seq-1` 接续当前轮；idle → `last_seq` 等下一轮）；`pending` 清空故不重发；回执与账本从不删除 |
| 16 轮询周期可配置 | `config.rs:109`、`:204-220`；设置界面 `settings_dialog.rs:184`；runtime 使用 `main.rs:730` |
| 17 下次轮询显示真实 deadline | `view.rs:871` + `:1357` |
| 20 发送前精确校验 | `draft-flow.ps1:37-39`（Prepare 与 Commit 都跑）→ `draft-context.ps1:40-41`、`:73-75`；歧义时 `:51` 抛错、`:92` 转为不发送 |
| 10 DSH 人工介入识别 | `main.rs:266-269` + `:897-908`；陈旧结果另由 `observe.mjs:41` 作废 |
| 11 重新接管 | `view.rs:670-672` 使按钮在 `paused_by_user` 可用；`main.rs:829-855`（adopt + 基线退一位 + 立即派发）；`commit_receipt` 回到 `waiting_*` |
| 21 / 22 交接文案 | `message_template.rs:63-79`：`prefix + body + suffix`，正文原样不改写 |
| 23 无内部 Marker | 实测真实请求：正文不含 delivery id、`A2A`、`source_hash`、`fingerprint` |
| 26 Session-aware Delivery ID | id = `hash(binding_key \| direction \| source_hash)`，含两个 Session |
| 32 Runtime 配置 | 9 个公开字段齐全 |
| 33 基础 / 高级分层 | 对话框确有两组（`settings_dialog.rs:158`、`:204`） |
| 34 First-run | `config.rs:524`（占位符）、`:590`（首次建立）、`:553`（检测 DSH） |
| 35 配置迁移 | `config.rs:37`（`DEPRECATED_KEYS`）、`:192`，并有测试 `:758` |
| 36 状态面板 | 两个会话卡、轮次、下次轮询、监听、自动状态 |
| 40 本地隐私隔离 | `.gitignore` 排除 `runtime/`、`backups/`、`artifacts/` |
| 41 release audit | `release-audit.ps1:11-48`：禁止路径、用户机器路径、真实 Session、secret、暂存区空白 |
| 42 自动 CI | `.github/workflows/ci.yml` 7 步，覆盖格式、测试、PowerShell、staged delivery、occupied-draft |
