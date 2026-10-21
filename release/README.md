# 发布构建输入快照

`archive-build-artifacts.ps1` 固化 `package-windows.ps1` 使用的六个二进制输入。依据的打包脚本 Git 原文 SHA256：`6fe1122878233e4bb89b0ddd4f94da09c417e7c00200c59381571a08e446742c`（工作树 CRLF 换行会影响文件哈希）。固定输入如下，快照保持这些相对目录：

- `src/brain/target/release/mochi-brain.exe`
- `experiments/003-translator-poc/librime/dist/lib/rime.dll`
- `experiments/weasel-fork-probe/output/WeaselServer.exe`
- `experiments/weasel-fork-probe/output/weasel.dll`
- `experiments/weasel-fork-probe/output/weaselx64.dll`
- `experiments/weasel-fork-probe/output/WeaselDeployer.exe`

在 Windows PowerShell 中调用：

```powershell
& .\release\archive-build-artifacts.ps1 -BackupRoot 'E:\构建备份\Mochi snapshots'
```

`-BackupRoot` 必填，必须是源码树外的全限定绝对路径；支持空格和 Unicode。拒绝相对、根相对、盘符相对路径以及涉及 junction/reparse point 的路径。所有输入与路径先预检，缺少任一文件不会创建备份根。每次成功调用直接在备份根创建唯一的 `snapshot-…` 子目录，不覆盖历史快照；复制失败仅清理本次部分快照。

每个快照只有上述六文件与 `manifest.json`。manifest 顶层只有 `schema_version`（1）、`created_utc`（UTC ISO8601）、`files`；六个文件条目各只有 `relative_path`、`size`（字节）、`sha256`（复制后字节的 SHA256）。不记录源码绝对路径或私人元数据。

这是发布输入的构建归档，不是部署或完整发布包：不采集语言模型 artifacts、配置、用户数据、personal.tsv 或 commits.jsonl，不运行二进制，也不修改安装或注册表。开发部署中使用官方 Weasel 加 fork rime.dll 的变体不能替代这里要求的完整六输入。脚本不会恢复已经丢失的文件；缺失的 fork 构建产物需要先通过原构建流程重建。
