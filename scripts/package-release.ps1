param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot

& node (Join-Path $PSScriptRoot 'release-version.mjs') check "v$Version"
if ($LASTEXITCODE -ne 0) { throw '发布版本校验失败。' }

$buildDir = Join-Path $repoRoot 'src-tauri/target/x86_64-pc-windows-msvc/release'
$binary = Join-Path $buildDir 'offline-preops-tool.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw "未找到 Windows x64 MSVC 主程序：$binary"
}
$installers = @(Get-ChildItem -LiteralPath (Join-Path $buildDir 'bundle/nsis') -Filter '*-setup.exe' -File)
if ($installers.Count -ne 1) { throw "预期恰好一个 NSIS 安装包，实际为 $($installers.Count) 个。" }

$releaseDir = Join-Path $repoRoot 'out/release'
if (Test-Path -LiteralPath $releaseDir) {
    throw "发布输出目录已存在，请先移走旧产物：$releaseDir"
}
$null = New-Item -ItemType Directory -Path $releaseDir
$prefix = "OfflinePreOpsTool_${Version}_windows-x64"
$installerPath = Join-Path $releaseDir "$prefix-setup.exe"
Copy-Item -LiteralPath $installers[0].FullName -Destination $installerPath

$portableDir = Join-Path $releaseDir $prefix
$null = New-Item -ItemType Directory -Path $portableDir
Copy-Item -LiteralPath $binary -Destination (Join-Path $portableDir 'OfflinePreOpsTool.exe')
Copy-Item -LiteralPath (Join-Path $repoRoot '.github/RELEASE_NOTES.md') -Destination (Join-Path $portableDir 'README.md')
Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination $portableDir
$zipPath = Join-Path $releaseDir "$prefix-portable.zip"
Compress-Archive -LiteralPath $portableDir -DestinationPath $zipPath -CompressionLevel Optimal

$checksums = @($installerPath, $zipPath) | ForEach-Object {
    $hash = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([System.IO.Path]::GetFileName($_))"
}
# 使用无 BOM 的 UTF-8 和 LF，供 Linux 发布任务的 sha256sum 直接校验。
[System.IO.File]::WriteAllText(
    (Join-Path $releaseDir 'SHA256SUMS.txt'),
    (($checksums -join "`n") + "`n"),
    [System.Text.UTF8Encoding]::new($false)
)
Write-Output "发布产物已生成：$releaseDir"
