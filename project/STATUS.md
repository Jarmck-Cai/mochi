# 项目状态

> 每个 session 收工时更新本文件。历史细节见 devlog/。
> 最后更新：2026-06-10（第二次更新：三路并行调研完成）

## 当前阶段

验证阶段：M1 流水线已就绪（待真实语料），ADR-001 接近定案

## 已完成

- [x] 产品愿景与方案 v2（README.md），项目定名 **Mochi**
- [x] 技术方案总纲（docs/DESIGN.md），已按调研结果修正进程拓扑与风险清单
- [x] 开发目录脚手架（session 协议、ADR、devlog、agent、git）
- [x] **librime translator 可行性调研**（docs/research/2026-06-10-librime-translator-feasibility.md）：方案 B 成立；Windows 需静态合并进 rime.dll；WeaselServer 全局锁 → Brain IPC 须 15-20ms 硬超时
- [x] **实验 001 流水线**（experiments/001-personalization-gain/）：端到端跑通，替身语料 +16.7pp 整句命中、+17.6pp 术语命中、通用对照回退 <1pp
- [x] **UIA 实测**（experiments/002-uia-probe/ + docs/research/2026-06-10-uia-context-readability.md）：微信 4.x 完全可读（对话历史+输入框），最大风险初步解除；caret rect 可行

## 进行中

（无）

## 下一步（按优先级）

1. **最小 translator 插件编译实测**（注册、每键调用频率、延迟手感）→ 定案 ADR-001。这是 ADR-001 唯一剩余实证环节
2. **实验 001 真实数据**：用户提供个人语料（.txt/.md 放 experiments/001-personalization-gain/data/personal/）→ 跑 M1 门禁（≥10pp 通过）
3. **UIA 补测**：微信输入框 caret rect（敲字后）、QQ/钉钉、记事本/VS Code/Word
4. 定案 ADR-003（Brain 服务语言）
5. GitHub 仓库：建议名 mochi-ime，等用户提供地址后 push

## 阻塞 / 待用户决策

- 真实个人语料待用户提供（M1 门禁的前提）
- GitHub 仓库地址待用户提供
- ADR-001 待编译实测后最终拍板

## 关键约束备忘

- 按键热路径 <50ms（预算 ~25ms）；Brain IPC 硬超时 15-20ms（WeaselServer 全局锁）；学习必须即时生效
- 自动上屏指标：覆盖率下错误率 <1%，不是平均准确率
- 暂不考虑成本因素（用户明确）；隐私优先全本地；个人打字数据永不入库
