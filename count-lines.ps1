<#
.SYNOPSIS
    Recursively counts lines of code per file type in a directory tree.

.DESCRIPTION
    Walks the target directory (and all sub-directories), skipping any
    folders listed in $IgnoreFolders, and counts lines in every file whose
    extension is listed in $FileExtensions. Blank lines and lines whose
    trimmed content starts with one of $CommentPrefixes are not counted
    as code.

    Always prints per-extension totals and a grand total. When Detailed
    mode is on (see below), it also prints a per-file line count.

    Detailed mode can be turned on two ways:
      1. In-script: set $DefaultDetailed = $true in the CONFIG section.
      2. Command line: pass -Detailed when invoking the script (overrides
         the in-script default).

.PARAMETER Path
    Directory to scan. Defaults to current working directory.

.PARAMETER Detailed
    Switch. If specified, prints per-file line counts in addition to the
    totals. If omitted, the script falls back to $DefaultDetailed below.

.EXAMPLE
    .\Count-Lines.ps1
    Scans the current directory using whatever $DefaultDetailed is set to.

.EXAMPLE
    .\Count-Lines.ps1 -Path C:\src\myproject -Detailed
    Scans C:\src\myproject and prints per-file counts as well as totals.
#>

param(
    [string]$Path,
    [switch]$Detailed
)

# ======================= CONFIG (edit as needed) =======================

# Directory to scan if -Path is not supplied. Defaults to cwd if left empty.
$DefaultPath = ""

# Whether to print per-file line counts when -Detailed is not passed on
# the command line.
$DefaultDetailed = $false

# File extensions to include (with or without leading dot).
$FileExtensions = @(".py", ".rs", ".js", ".ts", ".cs", ".java", ".cpp", ".h", ".sql")

# Folder names to ignore anywhere in the tree (case-insensitive).
$IgnoreFolders = @(".git", ".vs", ".vscode", "bin", "obj", "node_modules", "dist", "build", "packages", "venv", "__pycache__", ".venv", "test", "docs", "doc", "examples", "example")

# Line prefixes (after trimming whitespace) that mark a line as a comment
# and should therefore NOT be counted, e.g. "#", "//", "REM".
$CommentPrefixes = @("#", "//", "REM", "--", "/*", "*", "*/")

# =========================================================================

# Resolve effective settings: CLI args win, otherwise fall back to config.
$TargetPath = if ([string]::IsNullOrWhiteSpace($Path)) { $DefaultPath } else { $Path }
if ([string]::IsNullOrWhiteSpace($TargetPath)) {
    $TargetPath = (Get-Location).Path
}

$IsDetailed = $Detailed.IsPresent -or $DefaultDetailed

if (-not (Test-Path -LiteralPath $TargetPath)) {
    Write-Error "Path not found: $TargetPath"
    return
}
$TargetPath = (Resolve-Path -LiteralPath $TargetPath).Path

$NormalizedExtensions = $FileExtensions | ForEach-Object {
    if ($_.StartsWith(".")) { $_.ToLowerInvariant() } else { ".$($_.ToLowerInvariant())" }
}

Write-Host "Scanning '$TargetPath' ..." -ForegroundColor Cyan
if ($IsDetailed) { Write-Host "Mode: Detailed (per-file counts will be shown)" -ForegroundColor DarkCyan }

# Get all files, then filter out anything sitting under an ignored folder
$allFiles = Get-ChildItem -LiteralPath $TargetPath -Recurse -File -Force -ErrorAction SilentlyContinue

$files = $allFiles | Where-Object {
    $ext = $_.Extension.ToLowerInvariant()
    if ($NormalizedExtensions -notcontains $ext) { return $false }

    $relative = $_.FullName.Substring($TargetPath.Length).TrimStart('\', '/')
    $segments = $relative -split '[\\/]'
    $inIgnoredFolder = $false
    foreach ($seg in $segments) {
        if ($IgnoreFolders -contains $seg) { $inIgnoredFolder = $true; break }
    }
    -not $inIgnoredFolder
}

$perFileResults = [System.Collections.Generic.List[object]]::new()

foreach ($file in $files) {
    $lineCount = 0

    $content = Get-Content -LiteralPath $file.FullName -ErrorAction SilentlyContinue
    foreach ($line in $content) {
        $trimmed = $line.Trim()
        if ($trimmed.Length -eq 0) { continue }   # skip blank lines

        $isComment = $false
        foreach ($prefix in $CommentPrefixes) {
            if ($trimmed.StartsWith($prefix)) { $isComment = $true; break }
        }
        if ($isComment) { continue }

        $lineCount++
    }

    $perFileResults.Add([PSCustomObject]@{
        Path      = $file.FullName.Substring($TargetPath.Length).TrimStart('\', '/')
        Extension = $file.Extension.ToLowerInvariant()
        Lines     = $lineCount
    })
}

# ============================== OUTPUT ==================================

if ($IsDetailed) {
    Write-Host ""
    Write-Host "Per-file line counts:" -ForegroundColor Cyan
    $perFileResults | Sort-Object Lines -Descending | Format-Table Path, Extension, Lines -AutoSize
}

Write-Host ""
Write-Host "Per-extension totals:" -ForegroundColor Cyan
$byExtension = $perFileResults | Group-Object Extension | ForEach-Object {
    [PSCustomObject]@{
        Extension = $_.Name
        Files     = $_.Count
        Lines     = ($_.Group | Measure-Object -Property Lines -Sum).Sum
    }
}
$byExtension | Sort-Object Lines -Descending | Format-Table -AutoSize

$totalFiles = $perFileResults.Count
$totalLines = ($perFileResults | Measure-Object -Property Lines -Sum).Sum

Write-Host ""
Write-Host "TOTAL: $totalFiles files, $totalLines lines" -ForegroundColor Green
