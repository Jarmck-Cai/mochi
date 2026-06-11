# 实验 004：Mochi 部署进 Weasel 真实打字（M2-4）

> 目标：把 mochi（自建 rime.dll + brain 服务）部署进小狼毫 0.17.4 真实使用，
> 完成 M2 验收：混打切换次数 -80%、整句首选准确率 ≥ 微软拼音。

## 部署（需管理员）

```powershell
# 0) 前置：librime 已按 experiments/003 build-steps.cmd 构建（dist\lib\rime.dll）
# 1) 装 Weasel（任一方式）
winget install Rime.Weasel
# 2) 替换 rime.dll + 部署 mochi schema（可重复执行；-Restore 一键回滚原版）
powershell -ExecutionPolicy Bypass -File deploy-weasel.ps1
# 3) 起 brain（个人记忆默认在 <repo>\user_data\）
..\..\src\brain\target\release\mochi-brain.exe
```

- 输入法切到「小狼毫」，F4 选单里选 **Mochi**（保留了明月拼音作对照/兜底，
  brain 出问题随时 F4 切回，打字不中断）
- brain 日志（stderr）每键一行：`query input=.. app=.. top=..`——`app=`
  即场景分桶生效的证据（微信里是 weixin.exe，记事本是 notepad.exe）

## 风险与回滚

| 风险 | 缓解 |
|---|---|
| 我们的 rime.dll（librime master）与 Weasel 0.17.4 ABI 不合 | rime C API 稳定；首次部署自动备份 `rime.dll.stock`，`-Restore` 秒回滚 |
| brain 不在 → mochi 无候选，整行字母上屏 | F4 切明月拼音；长期方案是插件 EnsureConnected 失败时自动拉起 brain（遗留） |
| WeaselServer 全局锁 | 已由 15ms 硬超时 + QPC 兜底覆盖（M2-1/2 实测） |

## M2 验收协议（真实使用，建议 ≥3 天）

1. **混打切换次数**：选 20 条你真实会打的中英混合句（来自微信/笔记历史），
   分别用 Mochi 与微软拼音输入，记录每句需要的「切换动作」次数
   （Shift/选字修正英文词都算）。目标：Mochi ≤ 微软拼音的 20%
2. **整句准确率**：同 20 句 + 日常使用抽样，首选即正确的比例。
   目标：≥ 微软拼音
3. **即时学习体感**：纠正过的词（如"随机梯度"）下次是否直接首选；
   微信场景学的词在写作场景的表现（场景分桶）
4. brain 日志里 `elapsed_us` 分布（验证真实 app 下热路径预算）

## 文件

- `mochi.schema.yaml`——真实打字版 schema（v0.2：+标点/recognizer/Shift 切换）
- `default.custom.yaml`——Weasel 用户目录补丁（mochi + luna 并存）
- `deploy-weasel.ps1`——部署/回滚脚本（停服→换 dll→部署 schema→重启）

## 结论

- **2026-06-11 部署成功**：Weasel 0.17.4 + 我们的 rime.dll（librime 1.17.0
  master + mochi merged plugin），模块注册实证（服务端日志
  `registering component: mochi_translator`）。用户已真实打字，场景信号
  （notepad.exe / windowsterminal.exe）与即时学习全链路工作
- **首日反馈驱动两项改进**：① 候选 5→24 + 翻页（schema v0.3，page_size 9，
  -/= 翻页）；② 通用层补 rime essay 八股文底座（opencc 繁→简 + pypinyin
  注音，词典 6.5 万→37.3 万词条，unigram λ=1.0 混权）——"有些常用词"类
  缺词案例修复，用户语料 80 句 A/B：通用层 top-1 42.5%→46.2%（5 胜 2 负）。
  加载 11.8s→1.15s（前缀集改哈希），查询延迟无回归（len20 中位 189µs）
- 已知观察：commit 学习会记下用户上屏的错词（引用错例场景），目前靠
  essay 底座 + 正确选择的分数压制；信号分级仍在待办
- 真实使用对比微软拼音（验收协议见上）进行中
