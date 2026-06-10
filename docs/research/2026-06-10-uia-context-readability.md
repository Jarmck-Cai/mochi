# UIA 上下文可读性实测（2026-06-10）

> 对应 DESIGN.md 第四节"上下文采集"的最大不可控风险项："微信等自绘 UI 很可能读不到对话区"。
> 工具与原始输出：`experiments/002-uia-probe/`（probe.py + *-output.txt）。
> 环境：Windows 11 Home 26200，Python 3.11.8，uiautomation 2.0.29。
> 约束：只测当时正在运行的应用，未启动新应用。

## 一句话结论

**风险初步解除（好于预期）：微信 4.x 的消息历史和输入框都能通过 UIA 直接读到。** 本次实测的 4 个应用（微信、Chrome、Claude 桌面端、Discord）全部能读到编辑区文本；其中 3 个验证了 caret 坐标可得。记事本 / VS Code / QQ / 钉钉当时未运行，待补测。

## 可读性矩阵

✓ = 实测通过；✗ = 实测失败；△ = 部分通过/有条件；? = 未能定论；— = 未测

| 应用 | UI 技术 | (a) 编辑区全文 | (b) 光标前文 | (c) caret 坐标 | (d) 对话历史 | 备注 |
|------|---------|:---:|:---:|:---:|:---:|------|
| **微信 Weixin 4.x** | 自绘（mmui/Qt，自带 UIA provider） | ✓ | ✓ | ? | **✓** | 输入框 `mmui::ChatInputField` 支持 TextPattern；消息列表逐条可读；实测时输入框为空，caret rect 拿不到矩形，待输入文字后补测 |
| **Chrome（GitHub 页面）** | Chromium | ✓ | ✓ | ✓ | △ | 页面 Document + 表单 Edit 均支持 TextPattern；(d) 取决于具体网页（实测页非聊天页，但页面全文可读，网页聊天原理上同样可读） |
| **Claude 桌面端** | Electron | ✓ | ✓ | ✓ | ✓ | 对话历史正文直接出现在内层 Document 全文里；selection rect 实测 (1290,1870,1291,1905) |
| **Discord** | Electron | ✓ | ✓ | ✓ | △ | 焦点模式实测输入框 (a)(b)(c) 全过，caret rect (894,1968,895,2011)；(d) 本次抓到的最大 List 是频道列表，消息区需窗口前台时补测 |
| 资源管理器 | Win32 | — | — | — | n/a | 当时只有任务栏/桌面进程，无文件浏览窗口 |
| 记事本 | Win32 | — | — | — | n/a | 未运行（预期 ✓，Win32 Edit 是 UIA 最佳情况） |
| VS Code | Electron | — | — | — | n/a | 未运行（预期同 Claude/Discord：✓） |
| QQ / 钉钉 | 未知 | — | — | — | — | 未运行，**待用户配合测试** |

## 关键实测证据

### 1. 微信 4.x（最大风险项）——可读

微信 4.x 用 Qt 重写（窗口类 `mmui::MainWindow`），**自带 UIA provider**，关键控件全部暴露：

- 消息历史：`mmui::RecyclerListView`（Name='Messages'）的 ListItem 直接给出消息正文，逐条可读：

```
[ListItemControl] '<消息正文A>'
[ListItemControl] '12:15'            ← 时间戳是独立 item
[ListItemControl] '<消息正文B>'
[ListItemControl] '<URL>'
[ListItemControl] '<消息正文C>'
```

（真实捕获样本已匿名化；原始输出存档在 experiments/002-uia-probe/\*-output.txt，含真实聊天内容，已 gitignore 永不入库）

- 聊天输入框：`mmui::ChatInputField`，支持 TextPattern + ValuePattern + LegacyIAccessible，**无焦点状态下** DocumentRange.GetText 和 GetSelection 都正常返回
- 左侧会话列表（`mmui::XTableView`）还附带每个会话的最后一条消息预览——免费的额外上下文

注意点：消息 ListItem 不带结构化的"发送者"字段（时间戳是独立 item，群聊中正文 item 的 Name 就是消息文本），对话角色归属需要启发式拼装；caret rect 因实测时输入框为空拿不到矩形（空文档的 degenerate range 无 bounding rect），不能定论为 ✗。

### 2. Chromium / Electron 系——可读，但只在窗口可见时

Chrome、Claude、Discord 的网页/渲染区都通过内层 `DocumentControl` 暴露完整 TextPattern：页面全文、光标前文（MoveEndpointByRange 截取）、selection rect 全部实测通过。两个工程上必须知道的行为：

1. **窗口最小化/被遮蔽时 UIA 子树折叠**——Discord 最小化时整棵树只剩 6 个空 Pane，什么都读不到；恢复前台后输入框和列表立即可读。对输入法**无影响**（永远只读正在打字的前台应用），但意味着探针/采集器不能指望后台抓取 Chromium 系应用。
2. **嵌套 WebView 要走到内层**：外层 `WebView` Document 只有 LegacyIAccessible，必须找到内层 Name 非空的 Document 才有 TextPattern（Claude 桌面端实测：外层读不到，内层 3000+ 字符全文含对话历史一次拿全）。另外 Chromium a11y 树懒激活，首次查询可能要等几百 ms。

### 3. caret 坐标（幽灵文本阶段 2 的前提）

UIA TextPattern 的 selection range → GetBoundingRectangles 路线在 Chrome / Claude / Discord 实测全部成功，**未触发** GetGUIThreadInfo 回退。degenerate（光标无选区）range 的 rect 是宽 1-2px 的竖条，正好当幽灵文本锚点。

## 对 DESIGN.md 第四节风险项的初步结论

1. **"微信等自绘 UI 很可能读不到对话区"——4.x 版本不成立**。微信 4.x 自带 UIA provider，对话区、输入框、会话列表全可读。续写功能最关键的上下文来源（"对方刚问了什么"）拿得到。
2. **降级方案（窗口级场景信号）仍要保留**：旧版微信 3.x 未测（实现完全不同）；QQ / 钉钉未测；游戏类 / 终端类自绘应用大概率仍读不到。
3. **采集器实现要点**（从踩坑中得出）：
   - 读取必须在目标应用前台时进行（与输入法场景天然一致）
   - 控件定位要按应用做适配表（微信认 `mmui::ChatInputField` / `RecyclerListView`；Electron 认内层 Document），通用兜底是"焦点控件 TextPattern + 窗口内最大 List"
   - 子树遍历必须带节点数/深度/超时预算，否则浏览器大页面会卡住采集线程
   - 全文读取上限建议 ~500 字（DESIGN.md 既定），实测 GetText(20000) 在 3000+ 字符文档上无感知延迟

## 待用户配合补测清单

按价值排序（方法见 `experiments/002-uia-probe/README.md` 的"如何补测"小节）：

1. **微信输入框非空时的 caret rect**：在输入框敲几个字（别发送），跑 `probe.py --focus`——本次唯一没定论的微信项
2. **QQ / 钉钉**：打开后跑 `probe.py --window "QQ"` / `--window "DingTalk"`，重点看消息区 List 是否可读
3. **Discord 消息历史区**：Discord 前台时跑 `probe.py --focus`，确认消息 List（本次只抓到频道列表）
4. **记事本 / VS Code / Word / 资源管理器地址栏**：打开后各跑一次 `--focus`，补全矩阵（预期都 ✓，低风险）
5. **旧版微信 3.x**（如有机器装着）：验证降级方案是否真的需要
