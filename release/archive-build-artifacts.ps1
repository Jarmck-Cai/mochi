# Archive only the six binary inputs of package-windows.ps1.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$BackupRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Check every existing component, including ancestors outside the repository.
function Assert-PlainPath([string]$Path) {
    $current = $Path
    while ($current) {
        $item = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
        if ($null -ne $item) {
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Reparse point is not allowed: $current"
            }
            if ($current -ne $Path -and -not $item.PSIsContainer) {
                throw "Ancestor is not a directory: $current"
            }
        }
        $parent = [IO.Directory]::GetParent($current)
        if ($null -eq $parent) { break }
        $current = $parent.FullName
    }
}

# Windows fully qualified drive or UNC paths only; reject rooted/drive-relative paths.
if ($BackupRoot -notmatch '^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+(?:[\\/]|$))' -or
    $BackupRoot -match '^\\\\[?.]') {
    throw 'BackupRoot must be a fully qualified absolute Windows path.'
}
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')
$backup = [IO.Path]::GetFullPath($BackupRoot)
$sourcePrefix = $source + [IO.Path]::DirectorySeparatorChar
if ($backup.TrimEnd('\', '/').Equals($source, [StringComparison]::OrdinalIgnoreCase) -or
    $backup.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'BackupRoot must be outside the source tree.'
}

$relativePaths = @(
    'src/brain/target/release/mochi-brain.exe'
    'experiments/003-translator-poc/librime/dist/lib/rime.dll'
    'experiments/weasel-fork-probe/output/WeaselServer.exe'
    'experiments/weasel-fork-probe/output/weasel.dll'
    'experiments/weasel-fork-probe/output/weaselx64.dll'
    'experiments/weasel-fork-probe/output/WeaselDeployer.exe'
)

# Finish all input and destination checks before creating anything.
Assert-PlainPath $source
Assert-PlainPath $backup
if (Test-Path -LiteralPath $backup) {
    if (-not (Get-Item -LiteralPath $backup -Force).PSIsContainer) {
        throw 'BackupRoot is not a directory.'
    }
}
foreach ($relative in $relativePaths) {
    $inputPath = Join-Path $source $relative
    Assert-PlainPath $inputPath
    if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) {
        throw "Missing input file: $relative"
    }
}

$snapshot = Join-Path $backup ('snapshot-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffffZ') + '-' + [Guid]::NewGuid().ToString('N'))
$created = $false
try {
    Assert-PlainPath $backup
    [IO.Directory]::CreateDirectory($backup) | Out-Null
    Assert-PlainPath $snapshot
    # New-Item without Force refuses an existing snapshot directory.
    New-Item -ItemType Directory -Path $snapshot -ErrorAction Stop | Out-Null
    $created = $true
    $files = @(
        foreach ($relative in $relativePaths) {
            $inputPath = Join-Path $source $relative
            $outputPath = Join-Path $snapshot $relative
            Assert-PlainPath $inputPath
            Assert-PlainPath $outputPath
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($outputPath)) | Out-Null
            Assert-PlainPath $outputPath
            [IO.File]::Copy($inputPath, $outputPath, $false)
            [ordered]@{
                relative_path = $relative
                size = (Get-Item -LiteralPath $outputPath).Length
                sha256 = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
    )
    $manifest = [ordered]@{
        schema_version = 1
        created_utc = [DateTime]::UtcNow.ToString('o')
        files = $files
    }
    $manifestPath = Join-Path $snapshot 'manifest.json'
    Assert-PlainPath $manifestPath
    $json = $manifest | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText($manifestPath, $json, [Text.UTF8Encoding]::new($false))
    Write-Output $snapshot
} catch {
    if ($created) {
        # Never remove the backup root or any earlier snapshot.
        Assert-PlainPath $snapshot
        foreach ($child in Get-ChildItem -LiteralPath $snapshot -Recurse -Force) {
            if (($child.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Unsafe rollback path: $($child.FullName)"
            }
        }
        Remove-Item -LiteralPath $snapshot -Recurse -Force
    }
    throw
}
