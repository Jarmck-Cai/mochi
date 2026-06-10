# 项目状态

> 每个 session 收工时更新本文件。历史细节见 devlog/。
> 最后更新：2026-06-10（第三次更新：真实语料初跑 + ADR-004）

## 当前阶段

验证阶段：M1 部分验证（术语提取/中英混打已证实，门禁指标待中文语料），ADR-001 接近定案

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

## 进行中

- translator 编译实测：准备工作全部完成（librime+deps 已克隆、MochiTranslator 源码与 build-all.ps1 一键脚本就绪，见 experiments/003-translator-poc/README.md），**卡在本机无 C++ 工具链**——需安装 VS Build Tools + CMake（见"阻塞"）

## 下一步（按优先级）

1. **安装 C++ 工具链**（VS Build Tools + CMake，约数 GB）→ 运行 experiments/003-translator-poc/build-all.ps1 完成编译实测（M2 第一项任务）
2. **M1 门禁补全**：需要用户的**中文**个人语料（微信聊天导出/中文笔记/中文文章，.txt/.md 放 data/personal/）跑个性化中文排序指标
3. **UIA 补测**（优先级因 ADR-004 下降）：微信输入框 caret rect、QQ/钉钉
4. 定案 ADR-003（Brain 服务语言）

## 阻塞 / 待用户决策

- **C++ 工具链安装**（系统级安装，待用户同意）：VS Build Tools 2022（含 MSVC v143 + Windows SDK）+ CMake。winget 一键：`winget install Microsoft.VisualStudio.2022.BuildTools` + `winget install Kitware.CMake`
- 中文个人语料待用户提供（M1 门禁的前提；英文论文已用于术语词库验证；用户已确认素材可选，不阻塞 M2 开工）
- ADR-001 已带条件定案 Accepted（编译实测顺延 M2 第一项，翻车即回退）

## 关键约束备忘

- 按键热路径 <50ms（预算 ~25ms）；Brain IPC 硬超时 15-20ms（WeaselServer 全局锁）；学习必须即时生效
- 自动上屏指标：覆盖率下错误率 <1%，不是平均准确率
- 暂不考虑成本因素（用户明确）；隐私优先全本地；个人打字数据永不入库
