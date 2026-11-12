<#
.SYNOPSIS
    Step 1 transport spike: measures what WinGUI's design depends on, against a
    real environment, and writes the numbers to docs/decisions/.

.DESCRIPTION
    Run this from a laptop on the VPN (Windows PowerShell 5.1). It measures:

      1. how long a WinRM session takes to open, and a warm request's latency
         (the reason WinGUI holds sessions instead of opening one per click);
      2. listing folders of 1k / 10k / 50k entries the way the session worker
         does it (one pre-serialised string), against the naive object-returning
         way;
      3. download throughput for one large file and for many small files, three
         ways: WinGUI's chunked transfer, Copy-Item -FromSession, and SMB
         (\\host\X$) when port 445 is reachable;
      4. the server's WinRM quotas, which bound how many sessions a shared
         account can hold.

    The script CREATES TEMPORARY FILES in the remote user's temp folder and
    removes them afterwards. It therefore refuses anything that is not a private
    environment (-SD or -CL) unless you pass -Force.

    Results are printed and written as Markdown to
    docs\decisions\001-transport-spike-<host>.md.

.PARAMETER ComputerName
    The environment to measure, e.g. L-abc123-SD.

.PARAMETER Credential
    The account to connect with. Prompted for when omitted.

.PARAMETER ListCounts
    Folder sizes to list. 50000 takes a while to create; drop it for a quick run.

.PARAMETER LargeFileMB
    Size of the generated large test file.

.PARAMETER SmallFileCount
    Number of 4 KB files in the many-small-files test.

.PARAMETER Local
    Self-test: run every measurement on this machine without WinRM. Useful to
    check the script itself; the numbers say nothing about the network.

.NOTES
    Run it from a PowerShell prompt (or with `powershell -Command "& .\scripts\Spike-Transport.ps1 ..."`),
    not with `powershell -File`: -File does not parse comma-separated lists.

.EXAMPLE
    .\scripts\Spike-Transport.ps1 -ComputerName L-abc123-SD

.EXAMPLE
    .\scripts\Spike-Transport.ps1 -ComputerName L-abc123-SD -ListCounts 1000,10000 -LargeFileMB 32
#>
[CmdletBinding(DefaultParameterSetName = 'Remote')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Remote', Position = 0)]
    [string]$ComputerName,

    [Parameter(ParameterSetName = 'Remote')]
    [pscredential]$Credential,

    [Parameter(Mandatory, ParameterSetName = 'Local')]
    [switch]$Local,

    # Bounded on purpose: these create real files on the target. (Started with
    # `powershell -File`, a list such as 1000,10000 arrives as ONE number -
    # 100010000 - which the range check below rejects instead of obeying.)
    [ValidateRange(1, 200000)]
    [int[]]$ListCounts = @(1000, 10000, 50000),
    [ValidateRange(1, 4096)]
    [int]$LargeFileMB = 64,
    [ValidateRange(1, 20000)]
    [int]$SmallFileCount = 500,
    [string]$OutDir,
    [switch]$SkipSmb,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

if (-not $OutDir) { $OutDir = Join-Path (Split-Path -Parent $PSScriptRoot) 'docs\decisions' }
$hostLabel = if ($Local) { 'localhost-selftest' } else { $ComputerName }

# --- Safety: same rule as the app. Temp files are a change, so private only. ---
if (-not $Local) {
    $short = $ComputerName.Trim().TrimStart('\').TrimEnd('.').Split('.')[0]
    if ($short -notmatch '-(SD|CL|EA)$' -and -not $Force) {
        throw "'$ComputerName' is not a private environment (-SD / -CL). This script writes temporary files to the target, so it only runs against private environments. Pass -Force to override."
    }
    if (-not $Credential) { $Credential = Get-Credential -Message "Account for $ComputerName" }
}

$script:Session = $null
$script:Rows = New-Object System.Collections.Generic.List[object]
$script:Notes = New-Object System.Collections.Generic.List[string]

function Add-Row([string]$Area, [string]$Test, [double]$Seconds, [string]$Result) {
    $script:Rows.Add([pscustomobject]@{ Area = $Area; Test = $Test; Seconds = [math]::Round($Seconds, 3); Result = $Result })
    Write-Host ('  {0,-9} {1,-52} {2,9:N3} s  {3}' -f $Area, $Test, $Seconds, $Result)
}

function Invoke-Target([scriptblock]$Script, [object[]]$Arguments = @()) {
    if ($script:Session) {
        Invoke-Command -Session $script:Session -ScriptBlock $Script -ArgumentList $Arguments
    } else {
        & $Script @Arguments
    }
}

function Measure-Seconds([scriptblock]$Block) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = & $Block
    $sw.Stop()
    $sw.Elapsed.TotalSeconds
}

function Get-Percentile([double[]]$Values, [double]$P) {
    $sorted = $Values | Sort-Object
    $index = [math]::Min($sorted.Count - 1, [math]::Ceiling($P * $sorted.Count) - 1)
    $sorted[[math]::Max(0, $index)]
}

function Format-Rate([long]$Bytes, [double]$Seconds) {
    if ($Seconds -le 0) { return 'n/a' }
    '{0:N1} MB/s' -f (($Bytes / 1MB) / $Seconds)
}

function New-Session {
    $opt = New-PSSessionOption -OpenTimeout 20000
    New-PSSession -ComputerName $ComputerName -Credential $Credential -Authentication Negotiate -SessionOption $opt
}

# WinGUI's transfer: read a chunk at an offset remotely, append it locally.
function Copy-Chunked([string]$RemotePath, [string]$LocalPath, [int]$ChunkBytes) {
    $length = [long](Invoke-Target { param($p) (Get-Item -LiteralPath $p).Length } @($RemotePath))
    $out = [IO.File]::Open($LocalPath, [IO.FileMode]::Create, [IO.FileAccess]::Write)
    try {
        $offset = 0L
        while ($offset -lt $length) {
            $bytes = Invoke-Target {
                param($p, $o, $n)
                $f = [IO.File]::Open($p, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
                try {
                    [void]$f.Seek($o, [IO.SeekOrigin]::Begin)
                    $buf = New-Object byte[] $n
                    $read = $f.Read($buf, 0, $n)
                    if ($read -lt $n) {
                        $part = New-Object byte[] $read
                        [Array]::Copy($buf, $part, $read)
                        , $part
                    } else {
                        , $buf
                    }
                } finally { $f.Dispose() }
            } @($RemotePath, $offset, $ChunkBytes)
            $bytes = [byte[]]$bytes
            if ($bytes.Length -eq 0) { throw "Read 0 bytes at offset $offset of $RemotePath" }
            $out.Write($bytes, 0, $bytes.Length)
            $offset += $bytes.Length
        }
    } finally { $out.Dispose() }
}

function Test-Port([string]$HostName, [int]$Port) {
    $client = New-Object Net.Sockets.TcpClient
    try {
        $async = $client.BeginConnect($HostName, $Port, $null, $null)
        if (-not $async.AsyncWaitHandle.WaitOne(3000)) { return $false }
        $client.EndConnect($async)
        $true
    } catch { $false } finally { $client.Close() }
}

$localTemp = Join-Path ([IO.Path]::GetTempPath()) ("wingui-spike-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $localTemp | Out-Null
$remoteTemp = $null
$smbDrive = $null

Write-Host "WinGUI transport spike -> $hostLabel" -ForegroundColor Cyan
try {
    # ----------------------------------------------------------------------
    # 1. Session open and warm latency
    # ----------------------------------------------------------------------
    Write-Host "`n[1] Session"
    if (-not $Local) {
        $opens = @()
        foreach ($i in 1..3) {
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $s = New-Session
            $opens += $sw.Elapsed.TotalSeconds
            if ($i -lt 3) { Remove-PSSession $s } else { $script:Session = $s }
        }
        Add-Row 'Session' 'Open, first (cold: name lookup + auth)' $opens[0] ''
        Add-Row 'Session' 'Open, best of the next two' (($opens[1..2] | Measure-Object -Minimum).Minimum) ''
    } else {
        $script:Notes.Add('Self-test run on this machine without WinRM: session and network numbers are not meaningful.')
    }

    $warm = @()
    foreach ($i in 1..25) { $warm += Measure-Seconds { Invoke-Target { 1 } } }
    $warm = $warm[5..24]   # discard warm-up
    Add-Row 'Session' 'Warm request, median of 20' (Get-Percentile $warm 0.5) ''
    Add-Row 'Session' 'Warm request, p95 of 20' (Get-Percentile $warm 0.95) ''

    $info = Invoke-Target {
        $os = ''
        try { $os = [string](Get-CimInstance Win32_OperatingSystem).Caption } catch { }
        $gui = Test-Path "$env:SystemRoot\explorer.exe"
        [pscustomobject]@{
            Computer = $env:COMPUTERNAME
            OS       = $os.Trim()
            PS       = $PSVersionTable.PSVersion.ToString()
            Shell    = if ($gui) { 'Desktop Experience (explorer.exe present)' } else { 'Server Core / headless (no explorer.exe)' }
            Language = [string]$ExecutionContext.SessionState.LanguageMode
        }
    }
    $script:Notes.Add("Target: $($info.Computer) - $($info.OS) - PowerShell $($info.PS) - $($info.Shell) - language mode $($info.Language).")

    # ----------------------------------------------------------------------
    # 2. WinRM quotas (bound how many sessions a shared account can hold)
    # ----------------------------------------------------------------------
    Write-Host "`n[2] WinRM quotas"
    $quota = Invoke-Target {
        $names = 'MaxShellsPerUser', 'MaxConcurrentUsers', 'MaxProcessesPerShell', 'MaxMemoryPerShellMB', 'IdleTimeout'
        $out = @()
        foreach ($n in $names) {
            try { $out += ('{0}={1}' -f $n, (Get-Item -Path "WSMan:\localhost\Shell\$n" -ErrorAction Stop).Value) }
            catch { $out += ('{0}=(could not read: {1})' -f $n, $_.Exception.Message.Split("`n")[0].Trim()) }
        }
        $out -join '; '
    }
    Write-Host "  $quota"
    $script:Notes.Add("WinRM shell quotas on the target: $quota. If these could not be read remotely, run ``winrm get winrm/config/winrs`` on the server.")

    # ----------------------------------------------------------------------
    # Remote scratch area
    # ----------------------------------------------------------------------
    $remoteTemp = Invoke-Target {
        $d = Join-Path ([IO.Path]::GetTempPath()) ('wingui-spike-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $d | Out-Null
        $d
    }

    # ----------------------------------------------------------------------
    # 3. Listing
    # ----------------------------------------------------------------------
    Write-Host "`n[3] Listing"
    foreach ($count in $ListCounts) {
        $dir = Join-Path $remoteTemp "list-$count"
        $null = Invoke-Target {
            param($d, $n)
            New-Item -ItemType Directory -Path $d | Out-Null
            $empty = New-Object byte[] 0
            foreach ($i in 1..$n) { [IO.File]::WriteAllBytes((Join-Path $d ("file-{0:D6}.txt" -f $i)), $empty) }
        } @($dir, $count)

        # The worker's way: enumerate with .NET, return ONE string.
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $text = [string](Invoke-Target {
                param($p)
                $sb = New-Object Text.StringBuilder
                foreach ($e in (New-Object IO.DirectoryInfo $p).EnumerateFileSystemInfos()) {
                    $isDir = ($e.Attributes -band [IO.FileAttributes]::Directory) -ne 0
                    $len = if ($isDir) { 0 } else { $e.Length }
                    [void]$sb.Append($e.Name).Append("`t").Append([int]$isDir).Append("`t").Append($len).Append("`t").Append($e.LastWriteTimeUtc.Ticks).Append("`n")
                }
                $sb.ToString()
            } @($dir))
        $t = $sw.Elapsed.TotalSeconds
        $lines = @($text -split "`n" | Where-Object { $_ }).Count
        Add-Row 'Listing' "$count entries, pre-serialised string (WinGUI)" $t ("{0} entries, {1:N0} KB" -f $lines, ($text.Length / 1KB))

        # The naive way, for contrast (objects through the remoting serialiser).
        if ($count -le 10000) {
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $n = @(Invoke-Target {
                    param($p) Get-ChildItem -LiteralPath $p | Select-Object Name, Length, LastWriteTimeUtc, PSIsContainer
                } @($dir)).Count
            $t = $sw.Elapsed.TotalSeconds
            Add-Row 'Listing' "$count entries, Get-ChildItem objects (naive)" $t "$n entries"
        }
    }

    # ----------------------------------------------------------------------
    # 4. Transfers
    # ----------------------------------------------------------------------
    Write-Host "`n[4] Transfer - one large file ($LargeFileMB MB, random data)"
    $bigRemote = Join-Path $remoteTemp 'large.bin'
    $bigHash = Invoke-Target {
        param($p, $mb)
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        $buf = New-Object byte[] (1MB)
        $f = [IO.File]::Open($p, [IO.FileMode]::Create, [IO.FileAccess]::Write)
        try { foreach ($i in 1..$mb) { $rng.GetBytes($buf); $f.Write($buf, 0, $buf.Length) } } finally { $f.Dispose() }
        (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
    } @($bigRemote, $LargeFileMB)
    $bigBytes = [long]$LargeFileMB * 1MB

    foreach ($chunkMB in 1, 4) {
        $dest = Join-Path $localTemp "large-chunk$chunkMB.bin"
        $t = Measure-Seconds { Copy-Chunked $bigRemote $dest ($chunkMB * 1MB) }
        $ok = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash -eq $bigHash
        Add-Row 'Transfer' "Large file, chunked $chunkMB MB (WinGUI)" $t ("{0}, hash {1}" -f (Format-Rate $bigBytes $t), $(if ($ok) { 'OK' } else { 'MISMATCH' }))
        Remove-Item -LiteralPath $dest -Force
    }

    if ($script:Session) {
        $dest = Join-Path $localTemp 'large-copyitem.bin'
        $t = Measure-Seconds { Copy-Item -FromSession $script:Session -Path $bigRemote -Destination $dest }
        $ok = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash -eq $bigHash
        Add-Row 'Transfer' 'Large file, Copy-Item -FromSession' $t ("{0}, hash {1}" -f (Format-Rate $bigBytes $t), $(if ($ok) { 'OK' } else { 'MISMATCH' }))
        Remove-Item -LiteralPath $dest -Force
    }

    Write-Host "`n[5] Transfer - $SmallFileCount small files (4 KB each)"
    $smallRemote = Join-Path $remoteTemp 'small'
    $null = Invoke-Target {
        param($d, $n)
        New-Item -ItemType Directory -Path $d | Out-Null
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        $buf = New-Object byte[] 4096
        foreach ($i in 1..$n) { $rng.GetBytes($buf); [IO.File]::WriteAllBytes((Join-Path $d ("s-{0:D5}.dat" -f $i)), $buf) }
    } @($smallRemote, $SmallFileCount)
    $smallBytes = [long]$SmallFileCount * 4096

    # WinGUI's fast path: zip remotely, move one archive, expand locally.
    $zipRemote = Join-Path $remoteTemp 'small.zip'
    $zipLocal = Join-Path $localTemp 'small.zip'
    $expandTo = Join-Path $localTemp 'small-zip'
    $t = Measure-Seconds {
        $null = Invoke-Target {
            param($src, $zip)
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [IO.Compression.ZipFile]::CreateFromDirectory($src, $zip, [IO.Compression.CompressionLevel]::Fastest, $false)
        } @($smallRemote, $zipRemote)
        Copy-Chunked $zipRemote $zipLocal (4MB)
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::ExtractToDirectory($zipLocal, $expandTo)
    }
    $got = @(Get-ChildItem -LiteralPath $expandTo -File).Count
    Add-Row 'Transfer' 'Small files, remote zip + chunked + expand (WinGUI)' $t ("{0}, {1}/{2} files" -f (Format-Rate $smallBytes $t), $got, $SmallFileCount)

    # File by file over the session, the way a naive folder copy would go.
    $perFile = Join-Path $localTemp 'small-perfile'
    New-Item -ItemType Directory -Path $perFile | Out-Null
    $sample = [math]::Min($SmallFileCount, 100)
    $names = Invoke-Target { param($d, $n) Get-ChildItem -LiteralPath $d -File | Select-Object -First $n -ExpandProperty Name } @($smallRemote, $sample)
    $t = Measure-Seconds { foreach ($n in $names) { Copy-Chunked (Join-Path $smallRemote $n) (Join-Path $perFile $n) (1MB) } }
    Add-Row 'Transfer' "Small files, one request each ($sample files, extrapolated)" ($t * $SmallFileCount / $sample) ("{0}; {1:N0} ms per file" -f (Format-Rate $smallBytes ($t * $SmallFileCount / $sample)), (1000 * $t / $sample))

    if ($script:Session) {
        $dest = Join-Path $localTemp 'small-copyitem'
        $t = Measure-Seconds { Copy-Item -FromSession $script:Session -Path $smallRemote -Destination $dest -Recurse }
        $got = @(Get-ChildItem -LiteralPath $dest -File -Recurse).Count
        Add-Row 'Transfer' 'Small files, Copy-Item -FromSession -Recurse' $t ("{0}, {1}/{2} files" -f (Format-Rate $smallBytes $t), $got, $SmallFileCount)
    }

    # ----------------------------------------------------------------------
    # 5. SMB, if reachable
    # ----------------------------------------------------------------------
    if ($script:Session -and -not $SkipSmb) {
        Write-Host "`n[6] SMB (\\$ComputerName\X`$)"
        if (-not (Test-Port $ComputerName 445)) {
            Write-Host '  Port 445 is not reachable - SMB skipped.'
            $script:Notes.Add('SMB: port 445 was NOT reachable from this laptop, so the SMB fast path is not an option here.')
        } else {
            try {
                $letter = $remoteTemp.Substring(0, 1)
                $smbDrive = New-PSDrive -Name WGSPIKE -PSProvider FileSystem -Root "\\$ComputerName\$letter`$" -Credential $Credential
                $share = 'WGSPIKE:' + $remoteTemp.Substring(2)

                $dest = Join-Path $localTemp 'large-smb.bin'
                $t = Measure-Seconds { Copy-Item -LiteralPath (Join-Path $share 'large.bin') -Destination $dest }
                $ok = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash -eq $bigHash
                Add-Row 'SMB' 'Large file' $t ("{0}, hash {1}" -f (Format-Rate $bigBytes $t), $(if ($ok) { 'OK' } else { 'MISMATCH' }))

                $dest = Join-Path $localTemp 'small-smb'
                $t = Measure-Seconds { Copy-Item -LiteralPath (Join-Path $share 'small') -Destination $dest -Recurse }
                $got = @(Get-ChildItem -LiteralPath $dest -File -Recurse).Count
                Add-Row 'SMB' 'Small files, recursive copy' $t ("{0}, {1}/{2} files" -f (Format-Rate $smallBytes $t), $got, $SmallFileCount)
                $script:Notes.Add('SMB: port 445 reachable and the admin share was usable with this account.')
            } catch {
                Write-Host "  SMB failed: $($_.Exception.Message)"
                $script:Notes.Add("SMB: port 445 reachable but the admin share could not be used: $($_.Exception.Message)")
            }
        }
    }
}
finally {
    Write-Host "`nCleaning up..."
    if ($smbDrive) { Remove-PSDrive -Name WGSPIKE -ErrorAction SilentlyContinue }
    if ($remoteTemp) {
        try { $null = Invoke-Target { param($d) Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue } @($remoteTemp) }
        catch { Write-Warning "Could not remove remote scratch folder $remoteTemp : $($_.Exception.Message)" }
    }
    if ($script:Session) { Remove-PSSession $script:Session -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $localTemp -Recurse -Force -ErrorAction SilentlyContinue
}

# --------------------------------------------------------------------------
# Report
# --------------------------------------------------------------------------
$md = New-Object Text.StringBuilder
[void]$md.AppendLine("# Transport spike - $hostLabel")
[void]$md.AppendLine()
[void]$md.AppendLine("Measured $(Get-Date -Format 'yyyy-MM-dd HH:mm') from $env:COMPUTERNAME by ``scripts/Spike-Transport.ps1``.")
[void]$md.AppendLine()
foreach ($n in $script:Notes) { [void]$md.AppendLine("- $n") }
[void]$md.AppendLine()
[void]$md.AppendLine('| Area | Test | Seconds | Result |')
[void]$md.AppendLine('|---|---|---:|---|')
foreach ($r in $script:Rows) { [void]$md.AppendLine("| $($r.Area) | $($r.Test) | $($r.Seconds) | $($r.Result) |") }
[void]$md.AppendLine()
[void]$md.AppendLine('Compare against the targets in `docs/01-requirements.md` (NFR-P1, P2, P5) and record the outcome in `docs/decisions/001-transport.md`.')

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$file = Join-Path $OutDir ("001-transport-spike-{0}.md" -f ($hostLabel -replace '[^A-Za-z0-9._-]', '_'))
[IO.File]::WriteAllText($file, $md.ToString(), (New-Object Text.UTF8Encoding($false)))
Write-Host "`nReport written to $file" -ForegroundColor Green
