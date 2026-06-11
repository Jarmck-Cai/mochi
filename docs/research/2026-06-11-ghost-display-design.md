# Ghost/补全的显示形式设计（Copilot 范式 → IME 三区域映射）

- 日期：2026-06-11（第四轮真实使用反馈后）
- 背景：补全候选与用户已打内容无视觉区分 → 输入位置认知错位（实测：用户
  漏打 `hou` 的 `ou`，后续整句错位）。`+ou` comment 缓解但"不够直观"。

## Copilot 为什么直观（四要素）

1. **就地显示**：建议出现在光标处，无视线移动
2. **样式对比**：灰色/斜体 vs 正常文本——一眼分层
3. **边界即光标**：你打的终点 = 灰色的起点，无需解码
4. **零成本拒绝**：继续打字即穿透；Tab 单键接受

要害是 2+3：**在文字空间（不是拼音空间）、紧贴边界、用样式区分**。

## IME 的三个显示区域与可控性

| 区域 | 渲染者 | 子串样式可控？ | 结论 |
|---|---|---|---|
| ① 应用内联 composition | 应用按 TSF display attributes 渲染 | **可**（TF_DISPLAYATTRIBUTE 支持按 range 设前景色/下划线，Weasel 已用它区分已转换/未转换段） | 真·Copilot 形态的落点，需改 WeaselTSF（M5） |
| ② 候选窗 preedit 行 | WeaselUI | 仅两段（高亮段 vs 其余），由 rime 的 sel_start/sel_end 控制 | 可表达"打到哪"，但表达不了"补了什么" |
| ③ 候选窗候选项 | WeaselUI | **候选文本单一样式**；comment 是独立样式元素（浅色） | 今天唯一的杠杆 = comment |

## 分阶段方案

**阶段 0（已上线，今天再改进）**：comment 标记从拼音空间换到**文字空间**——
`自动补全后 ▸后`（▸ 后面的字是引擎补的）、`congratulations ▸ulations`。
对中文按"读音是否被完整敲出"逐字判定（`cesh`→测试 只标 `▸试`，因为 ce 已
完整、shi 只敲了 sh）。比 `+ou` 少一次拼音→字的翻译。

**阶段 1（M5 前菜，改 WeaselUI，我们已在分发自编译 rime.dll，fork weasel
是同一条供应链）**：候选项支持子串样式——协议在 comment 里带结构（或扩展
ipc-v0 候选对象加 `ghost_from` 字段），WeaselUI 把候选文本 ghost 部分画成
灰色。视觉效果 = 候选内灰字，无需动 TSF。

**阶段 2（M5 主菜，真·ghost）**：WeaselTSF 在应用内联 composition 末尾追加
ghost range，display attribute 设灰色无下划线；Tab/右箭头接受，继续打字
穿透。退化路径：应用不支持 display attributes（部分 Electron/旧应用）时
回落到阶段 1 的候选窗形态。续写预测（LLM）接的就是这个显示通道。

## 协议影响（提前记录，避免临时破约）

- ipc-v0 candidate 对象需加可选字段 `ghost_from`（int，文字空间的 ghost
  起始字符下标）；comment 退回纯注释用途。加字段向后兼容（插件忽略未知
  字段），列入 ipc-v1 待办而非现在动
- 输入区域（②）与候选区域（③）的分工在阶段 2 后：② 显示已敲拼音 +
  灰色 ghost 读音（可选），③ 显示完整候选 + 灰色 ghost 文字

## 现状记录

- 本轮先落阶段 0（文字空间 ▸ 标记），阶段 1/2 归 M5（与续写预测共用通道）
