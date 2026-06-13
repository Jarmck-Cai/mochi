# 项目状态

> 每个 session 收工时更新本文件。历史细节见 devlog/。
> 最后更新：2026-06-12（第九次更新：本机从"只剩源码"全量重建恢复 + 官方 Weasel+fork rime.dll 部署，混打基本盘用户验收通过）

## 当前阶段

**M2-4 部署完成 + 好友试用版已发布 + 本机重新部署真实使用中**（剩：对比微软拼音的正式验收 + 好友反馈收集）

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

- [x] **ADR-003 定案（Accepted）**：Brain 用 **Rust**（内存安全 + cargo + 编译器为 AI 代码兜底；KenLM 缺口经分析为伪需求）。三语分工：C++ 薄插件 / Rust brain / Python 实验工具
- [x] **IPC 协议 v0**（docs/specs/ipc-v0.md）：命名管道消息模式 + JSON，query/commit 两个方法，插件侧 15ms 硬超时 + 2s 退避降级

- [x] **M2-1 E2E 链路 ✅（2026-06-10）**：src/brain（Rust，9 单测过）+ src/ime-plugin（C++，成为插件唯一源）。实测：echo 候选经管道上屏、稳态 e2e 中位 0.26-1.2ms（预算余量 >5 倍）、brain 死 23µs 检出不卡键、重启自愈。主会话复测修复 GetTickCount64 量化 bug（伪超时，换 QPC）。报告：docs/research/2026-06-10-m2-e2e-ipc.md（含 5 个遗留问题，管道抢注防护最重要）
- [x] **lm-artifacts-v0 导出 ✅**：dict 65,121 / ngram 128 万 / english 1 万，21.6s 可重建（experiments/001 export_artifacts.py）

- [x] **M2-2 解码移植 ✅（2026-06-10）**：Rust brain 真实拼音解码上线——`nihao`→你好、`zheshiyigetest`→这是一个test（混打直接工作）。解码中位 ≤200µs/最大 847µs（5ms 预算的 4%）；artifacts 加载 ~600ms/96MB；与 Python 参考 top-1 一致 10/10；26 单测过。又修一个真 bug：WaitForSingleObject 超时被时钟节拍量化提前返回（3030 键压测 133 次误判→0），QPC 重等待兜底。报告：docs/research/2026-06-10-m2-decoder-port.md。**已知锚点：`suijitidu`→"随即梯度"（通用 LM 错），M2-3 个人层接入后应变"随机梯度"**

- [x] **M2-3 个人记忆库 ✅（2026-06-10 晚）**：锚点通过——rime_console 选一次"随机梯度"，下一次裸打 `suijitidu` 首选即出；重启记忆仍在（commits.jsonl 回放，按天衰减 0.98）。场景分桶（pipe 实测）+ 贪心分词/拼音对齐 + 词典外短语促升建弧 + 英文大小写习惯 + 文档导入冷启动（export_personal.py → user_data/personal.tsv，6361 行）。持久化用**追加 jsonl 而非 rusqlite**（用户可查看/删行，理由见报告）。Python full 配置 top-1 一致 12/12；延迟无回归（个人层加载后 len20 中位 199µs）；37 单测全过。插件 commit_notifier 已挂（e2e 遗留 3 清掉）。报告：docs/research/2026-06-10-m2-personal-memory.md

- [x] **M2-4 代码侧 ✅（2026-06-11）**：① 不完整音节尾部修复——纯 fallback 后缀标 FallbackTail（先验 -1.5），`woyaoc`→"我要c"不再"我压oc"（根因：英文垃圾短词弧便宜），39 单测过；② 真实 scene 信号——插件 SceneProbe 前台进程名（HWND 缓存）接入 query/commit，brain 日志带 app=；③ 部署套件 experiments/004-weasel-deploy/（schema v0.2 +标点+Shift 切换、与明月并存 F4 兜底、deploy-weasel.ps1 自动备份/一键回滚、README 含 M2 验收协议）

- [x] **M2-4 部署成功 + 首日真实使用（2026-06-11）**：Weasel 0.17.4 + 自建 rime.dll，模块注册实证；用户真实打字，场景信号（notepad/terminal）与即时学习闭环全工作（用户用 Mochi 打出了反馈消息本身）。首日反馈两项改进当天上线：① 候选 5→24 + 翻页（schema v0.3）；② **essay 八股文进通用层**（繁→简+注音，词典 6.5万→37.3万，unigram λ=1 混权）——"有些常用词"案例修复，80 句 A/B 通用层 top-1 42.5%→46.2%（5胜2负）；加载 11.8s→1.15s（前缀集哈希化）。详见 experiments/004 README 结论

- [x] **灰色 ghost 点亮 + 局部候选（2026-06-11 深夜）**：① ghost 灰字不可见根因 = aqua 配色未定义 comment 色（机制本身正常），weasel.custom.yaml 补 0x999999 已生效；② **局部候选**——变长前缀候选（每前缀位最优，cap 6，长在前；原始字母切点 + 按键均摊分数门双重防垃圾），Candidate 加 len 字段，插件按 len 设候选跨度，rime 部分选择原生兼容（e2e：选「常用词」→ 剩余继续组字 → 上屏「常用词很重要」）。47 单测全过

- [x] **第二轮反馈闭环（2026-06-11 下午）**：① 英文连写——连续英文弧切换成本只收一次（en_continue -0.5）+ 相邻英文词自动空格，`timewindow`→"time window"、`deeplearning`→"deep learning"；② **纠正信号检测**——同输入改选 = 纠正：新选择 3 倍加权、被否定事件按记录的精确增量回撤（RecentCommit 存实际计入的 token 句子，免疫短语促升导致的分词漂移），journal 仍存原始事实、回放重新解释（确定性测试过）。"桔子案例"翻回"句子" ✓。42 单测全过
- [x] **M3 打磨包第一期（2026-06-11 第三轮反馈闭环）**：① **尾部补全弧**（音节/词 key/英文词/个人词四路，惩罚 -2.0）——`congrat`→congratulations、`separ`→separate、`shangxiaw`→上下文、`eeg` 次选 EEGLab（个人补全）；② **模糊音**（z/zh c/ch s/sh in/ing en/eng an/ang，单音节变体，惩罚 -1.5）——`pinying`→拼音；③ **英文底座**：wordfreq top-50k + zipf 频率 λ=0.3 混入 unigram（英文词第一次有彼此排序）。44 单测全过。详见 devlog 2026-06-11-打磨包第一期
- [x] **第四轮反馈闭环（2026-06-11 晚）**：① **补全可视化**——补全候选 comment 标 `+«补入字母»`（如 自动补全后 `+ou`）、模糊候选标 `~`，Weasel 浅色渲染，解决"不知道打到哪了"的认知错位（真 ghost 内联仍归 M5）；② **放弃输入防污染**——≥6 字母且完整切为 ≥3 拼音音节的 ASCII 上屏视为放弃的原始输入，不学进英文词库（"zidongbuquanhou"曾被学成英文词并参与补全，回放即自愈）。46 单测全过
- [x] **Ghost 显示推进（2026-06-11 深夜）**：① 阶段 0 完整落地——comment 标记升级到**文字空间**（`自动补全和 ▸和`、`congratulations ▸ulations`，按读音覆盖度逐字判定）+ **preedit 边界标记**（`shang xia w‥en`，输入区/preedit 行显示真实输入边界）；② **阶段 1 候选内灰字已部署**——fork weasel 0.17.4（4 文件 102 行：GHOST 属性 + ▸ comment 解析 + DWrite 区间灰绘），自编 WeaselServer/weasel.dll/weaselx64.dll 已替换安装（原版 .stock 备份），补丁固化 experiments/004/weasel-ghost-0.17.4.patch。加载时间挂起项关闭（正常功耗 2150ms < 3s）。供应链升级：现在同时分发 rime.dll + weasel 三件套
- [x] **局部候选用户验收通过（2026-06-12）**：混在完整候选中可用，体验提升（用户真实打字确认）
- [x] **ghost 灰字"看不到"根因修复（2026-06-12）**：机制全程在工作（fork 解析/绘制/comment 链路逐环验证全通），凶手是**高亮态配色**——补全候选几乎总是首选=高亮候选，aqua 高亮蓝底白字（0xffffff），ghost 的 0xeeeeee 与白字肉眼不可分。已改半透明白 0x80ffffff（_TextOutGhost 走 alpha 通道，蓝底渲染为褪色浅蓝白），纯配置生效。顺手修部署脚本：server 经 explorer 启动免提权继承 + 补分发 weasel.custom.yaml。详见 devlog 2026-06-12-ghost灰字高亮态配色修复
- [x] **Windows 好友试用版 v0.2.0-alpha 发布（2026-06-12）**：GitHub Release 已发布（https://github.com/Jarmck-Cai/mochi/releases/tag/v0.2.0-alpha ，zip 16MB，SHA256 已附；本地留存 dist\）。发布套件 release/：install.ps1（weasel 三件套+rime.dll 替换、方案部署、**brain 开机自启**经 vbs 隐藏窗口——好友机器重启自愈）、uninstall.ps1（.stock 一键还原，保留个人数据）、测试者 README、package-windows.ps1（个人数据防呆：user_data/personal.tsv/commits.jsonl 永不入包）

- [x] **本机全量重建恢复 + 重新部署（2026-06-12，第九次更新）**：开发机此前被剥到只剩源码——gitignore 的重产物全失（LM artifacts、SIGHAN 语料、.venv、fork weasel 三件套；用户重装官方 Weasel 覆盖了旧 fork，无 .stock）。① **通用层 artifacts 21s 从零重建**——experiments/001 建 venv（wordfreq/opencc/pyyaml/pypinyin）+ prepare_data 自动下载公共数据（SIGHAN 链接仍活）+ essay.txt 取自官方 Weasel data/ + export_artifacts.py，counts 与基线对齐（dict 372717 / tri 235688 / english 47947，仅 +1 漂移，忠实复现）；② **官方 Weasel + fork rime.dll 模式部署**（用户选：暂不重建 fork 三件套，少"候选内灰字 ghost"视觉，其余全功能）——仿真新用户装到 `%LOCALAPPDATA%\Mochi`，仅 rime.dll 换 fork（官方备份 .stock）；③ **标准用户提权坑**——Jarmck 非管理员，UAC 提权切到管理员 profile 致 per-user 路径写错位，故拆分：per-user 部分以 Jarmck 身份做，**只"停 server+换 rime.dll"用最小化提权脚本**（experiments/004/swap-rimedll.ps1，只碰 Program Files；deploy-local.ps1 含同坑警告头）。实测：整句 `wohuikaishishijishiyong`→"我会开始实际使用"，延迟 57µs–1980µs（亚毫秒），场景信号 windowsterminal.exe，即时学习+commits.jsonl 持久化全工作。**混打基本盘用户验收通过**（这是一个test/你好/deep learning/上下文/拼音 全对）。详见 devlog 2026-06-12-全量重建恢复与本机部署

## 进行中

- **M2 正式验收**：真实使用对比微软拼音（协议见 experiments/004 README：20 句混打切换次数 + 整句准确率，建议 ≥3 天）
- **ghost 灰字二次修复待用户验收**：打 `shangxiaw`——首选"上下文"的"文"应呈**褪色浅蓝白**（半透明白在蓝底上）；非首选补全候选（如"上下五千年"的"五千年"）应呈**灰色**。若仍无差别，下一步在 fork 加日志取运行时证据（GHOST attr 是否到达绘制层）。注：preedit 行的 ‥ 分界是阶段 0 设计行为不是 bug，真正的 preedit 内联灰字归 M5（TSF display attributes）
- **好友试用反馈收集**：等待分发方式定案后启动（见待用户决策）

## 下一步（按优先级）

> **★ 开发方案（2026-06-12 定，相位制）**——详见 docs/research/2026-06-12-离线教练分层与快慢协同.md。
> 核心：两个主开发靶点（通用层语料现代化 / 个人教练）都需"真实失误清单"才能精准命中，故分相位。
> - **相位 0 固本 ✅ 完成**：① fork rime.dll+brain.exe 备份到仓库外 E:\Projects\mochi-binbackup（含重建配方；weasel 三件套已丢无副本，记录重建路径）；② brain 自启注册+模拟开机已验（待真实重启终验）；③ 失误速记通道 project/misfire-log.md
> - **相位 1 用+攒数据（这几天，被动）**：日常使用攒真实 misfire 清单 + commits.jsonl 纠正模式；顺带打 M2 正式验收数据（vs 微软拼音）
> - **相位 2 主开发（数天）**：M3 二期 = 云教练首秀——云 LLM 生成现代语料→重训 trigram→回放回归门禁→不退步才换表（通用层纯公共数据零隐私）。**机制已全部打通并端到端验证（2026-06-13）**：① 回归门禁 experiments/001/gate.py（对照集不退+目标集提升才 exit 0，全自动无人工）；② export_artifacts.py 加 `--extra-corpus/--extra-weight` 注入现代语料重训；③ 闭环实证：8 句 demo 现代语料 weight=50 → dazi「大字→打字」、control 零损伤 → 门禁 PASS。④ **真实语料管线（2026-06-13，零模型路线）**：build_modern_corpus.py（Leipzig zho_news CC BY → **pkuseg** 分词，按 PKU 标准与基座对齐，胜 jieba）。首次实测 10K 新闻@w1：control 70→69%（回退 2 句文学句）、dazi 未修（新闻无"打字"）→ 门禁正确拦下。**结论：管线就绪，但选哪种语域/多大量真正阻塞于相位 1 真实 miss**（新闻语域修不动口语词、还伤文学控制句）。详见 docs/research 离线教练分层笔记 §相位2实证
> - 待 ADR 化（笔记 §6）：离线教练分层与隐私分界 / 热路径小模型降级为按需

1. 真实使用观察打磨包体感（补全噪声水平、模糊音误报、50k 词表低频词噪声——必要时按 rank 加先验斜坡）
2. 开发机 brain 自启对齐 release 方式（试用包已做 HKCU Run + vbs；开发机目前仍手动拉起）；插件 EnsureConnected 失败时 CreateProcess 作为兜底仍值得做
3. 加载时间正常功耗复测（低功耗态 24.6s/归一化 ≈2s）；超 3s 则上二进制缓存。底座变更前先备份 artifacts 供 A/B（这轮漏了）
4. 中期：现代大语料重训 trigram（M3 二期，"整句准确率 ≥ 微软拼音"必要条件）；键内 typo 容错（`seperate`/`wn`）归 M4 与置信度同批；ghost 灰色内联归 M5（补全候选 = 其阶段 1）
5. 管道抢注防护（FIRST_PIPE_INSTANCE + ACL，产品化前——好友试用机器上 pipe 是默认 ACL，提上日程）；UIA 补测（M5 前）
6. **重产物备份固化（第九次更新教训）**：artifacts 已证 21s 可重建（脚本+公共数据齐全）；但 **fork weasel 三件套无任何备份、重建要 C++ 工具链**——"装官方覆盖即全失"的脆弱点。建议把 fork 构建产物归档到不入 git 的固定位置。本机当前为官方 Weasel+fork rime.dll，**ghost 候选内灰字暂不渲染（设计内取舍）**，要恢复需重装 fork 三件套
7. 重启本机一次，验证 brain 开机自启实际生效（HKCU Run / vbs 刚落地，未经重启验证）

## 阻塞 / 待用户决策

- **试用包分发方式**：仓库为 private，朋友打不开 release 链接。选项：①直接发 zip 文件（dist\mochi-windows-x64-v0.2.0-alpha.zip，最简单）②邀请朋友为仓库协作者 ③仓库转 public
- **macOS 移植是否排期**：无法从现有代码打包——前端层全部 Windows 专属（fork 的是 Weasel；IPC 是命名管道；brain listener 调 Win32 API）。移植 = fork Squirrel + IPC 抽象 Unix socket + brain listener 跨平台 + Mac 硬件编译签名，里程碑级工作量；核心 Rust（解码/个人记忆/打分）可移植
- 仓库中其他工具生成的 AGENTS.md/.codex/.agents 是否保留（不阻塞）

## 关键约束备忘

- 按键热路径 <50ms（预算 ~25ms）；Brain IPC 硬超时 15-20ms（WeaselServer 全局锁）；学习必须即时生效
- 自动上屏指标：覆盖率下错误率 <1%，不是平均准确率
- 暂不考虑成本因素（用户明确）；隐私优先全本地；个人打字数据永不入库
