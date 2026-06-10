# 项目状态

> 每个 session 收工时更新本文件。历史细节见 devlog/。
> 最后更新：2026-06-10（第四次更新：M1 门禁通过 + ADR-001 实测定案）

## 当前阶段

**M1 完成 ✅ → M2 开工就绪**（基本盘：RIME translator + 词图 + 记忆库 + 中英混打）

## 已完成

- [x] 产品愿景与方案 v2（README.md），项目定名 **Mochi**
- [x] 技术方案总纲（docs/DESIGN.md），已按调研结果修正进程拓扑与风险清单
- [x] 开发目录脚手架（session 协议、ADR、devlog、agent、git）
- [x] **librime translator 可行性调研**（docs/research/2026-06-10-librime-translator-feasibility.md）：方案 B 成立；Windows 需静态合并进 rime.dll；WeaselServer 全局锁 → Brain IPC 须 15-20ms 硬超时
- [x] **实验 001 流水线**（experiments/001-personalization-gain/）：端到端跑通，替身语料 +16.7pp 整句命中、+17.6pp 术语命中、通用对照回退 <1pp
- [x] **UIA 实测**（experiments/002-uia-probe/ + docs/research/2026-06-10-uia-context-readability.md）：微信 4.x 完全可读（对话历史+输入框），最大风险初步解除；caret rect 可行
- [x] **GitHub**：https://github.com/Jarmck-Cai/mochi （main 分支已推送）
- [x] **实验 001 真实语料初跑**：用户语料为英文论文（HFO/癫痫领域）→ 流水线增强为全文英文术语提取（文档导入冷启动路径）。提取 854 词个人词库（HFOs/pHFOs/EEG/SOZ，含大小写习惯），混打演示：baseline "监测俄eg信号" → +personal "监测EEG信号" 全部正确。**M1 门禁指标（中文个性化排序）需要中文个人语料，待用户提供**
- [x] **ADR-004（Accepted）**：上下文三级阶梯 + 三点补充——冷启动素材完全可选；场景与语料绑定（个人记忆按场景分桶，层级回退）；Tier 2 读屏为备选/补强，专注 M2-M4。场景分桶已写入 DESIGN 打分公式

- [x] **M1 门禁通过（2026-06-10）**：用户中文语料（微信沟通+电话纪要）实测，整句首选命中率 41.6%→55.1%（**+13.5pp ≥ 10pp**），字级 +4.9pp，41 赢/4 输，通用对照无回退。仅 285 条训练子句即达标——记忆假设强验证。正式报告：experiments/001-personalization-gain/report.md
- [x] **ADR-001 实测定案（条件解除）**：librime+MochiTranslator 编译成功（VS Build Tools 2022），候选接管 ✓、每键恰 1 次 Query ✓、插件开销 <1μs ✓、15ms 模拟延迟下每键 15.5-16.5ms 无阻塞 ✓。实测数据见 ADR-001 与 experiments/003-translator-poc/README.md

## 进行中

（无）

## 下一步（按优先级）

1. **M2 开工**：src/ 起项目骨架——ime-plugin（基于 003 的 MochiTranslator 扩展真实词图解码）+ brain 服务 + IPC（15-20ms 硬超时 + 降级）。实验 001 的 Python 解码器是算法参考实现
2. 定案 ADR-003（Brain 服务语言：C++ vs Rust）——M2 开工前必须定
3. M2 内迁移：把实验 001 的词图/打分算法移植到 brain 服务（含场景分桶，ADR-004）
4. UIA 补测（低优先级，M5 前完成即可）：微信输入框 caret rect、QQ/钉钉

## 阻塞 / 待用户决策

- ADR-003（Brain 服务语言）建议尽快讨论定案，阻塞 M2 的 brain 服务部分（ime-plugin 部分不阻塞）
- 仓库中其他工具生成的 AGENTS.md/.codex/.agents 是否保留（不阻塞）

## 关键约束备忘

- 按键热路径 <50ms（预算 ~25ms）；Brain IPC 硬超时 15-20ms（WeaselServer 全局锁）；学习必须即时生效
- 自动上屏指标：覆盖率下错误率 <1%，不是平均准确率
- 暂不考虑成本因素（用户明确）；隐私优先全本地；个人打字数据永不入库
