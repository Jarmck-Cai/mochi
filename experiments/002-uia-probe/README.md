# 实验 002：UIA 上下文可读性探针

> 回答 DESIGN.md 第四节的最大不可控风险：**Brain 服务到底能从前台应用读到什么？**
> 实测报告见 [docs/research/2026-06-10-uia-context-readability.md](../../docs/research/2026-06-10-uia-context-readability.md)。

## 依赖安装

```powershell
py -m pip install uiautomation   # 实测版本 2.0.29，Python 3.11，Win11
```

## 用法

```powershell
# 1. 扫描模式：枚举所有顶层窗口，报告进程名/标题/Edit·Document 控件/支持的 Pattern
py -X utf8 probe.py --scan
py -X utf8 probe.py --list-only        # 只列窗口不遍历子树（快）

# 2. 焦点模式：N 秒倒计时内切到目标窗口、把光标放进输入区，然后深度测试焦点控件
py -X utf8 probe.py --focus            # 默认延迟 5 秒
py -X utf8 probe.py --focus --delay 10

# 3. 指定窗口模式：按标题/进程名正则匹配，无需焦点即可深度测试（验证后台读取）
py -X utf8 probe.py --window "Weixin"
py -X utf8 probe.py --window "chrome.exe"
```

深度测试四项（每项输出 ✓/✗ 和 100 字符内容样本）：

| 项 | 含义 | 实现 |
|----|------|------|
| (a) 编辑区全文 | 能否读到编辑控件的完整文本 | TextPattern.DocumentRange → ValuePattern → LegacyIAccessible 三级回退 |
| (b) 光标前文本 | 能否读到光标之前的文本（续写的输入） | TextPattern.GetSelection + MoveEndpointByRange 截取文档开头→选区起点 |
| (c) 光标屏幕坐标 | 能否拿到 caret rect（幽灵文本锚点） | selection range GetBoundingRectangles；失败回退 GetGUIThreadInfo |
| (d) 对话历史 | 聊天类应用能否读到消息区文本 | 找窗口内最大的 List/DataGrid/Tree 控件，读最后几条子项文本 |

## 辅助脚本

```powershell
# 通用控件树挖掘：列出指定窗口内所有 Edit/Document/List 控件（类名/Pattern/坐标/子项数）
py -X utf8 explore_tree.py "Weixin" 5000

# 微信专测：定位 mmui::RecyclerListView（消息列表）和 mmui::ChatInputField（输入框）并读取内容
py -X utf8 test_wechat_deep.py
```

`*-output.txt` 是 2026-06-10 实测的原始输出存档。

## 如何补测微信 / QQ / 钉钉等

1. 打开目标应用并进入一个聊天窗口（微信请选一个有近期消息的会话）
2. 跑 `py -X utf8 probe.py --window "Weixin"`（或 `"QQ"`、`"DingTalk"`、按窗口标题匹配）——验证**无焦点后台读取**
3. 在输入框里**敲几个字但不发送**，再跑 `py -X utf8 probe.py --focus --delay 5`，倒计时内点回输入框——验证非空输入框的 (a)(b)(c)（2026-06-10 实测时微信输入框为空，caret rect 无法定论，这是主要待补测项）
4. 微信可加跑 `test_wechat_deep.py` 看消息列表逐条内容
5. 把输出粘贴回研究报告的"待补测"小节

## 已知坑（写探针时踩过的）

- **Chromium/Electron 窗口最小化或被遮蔽时 UIA 子树会折叠**（整棵树只剩几个空 Pane），必须窗口可见时测；a11y 树是懒激活的，首查可能延迟几百 ms
- Electron 应用（Claude 桌面端）嵌套多层 WebView Document：外层只有 LegacyIAccessible，要走到内层 Name 非空的 Document 才有 TextPattern
- 控制台输出含中文和 ✓/✗，务必用 `py -X utf8` 运行
- 子树遍历必须限节点数/深度/超时（probe.py 内置预算），否则浏览器大页面会卡死
