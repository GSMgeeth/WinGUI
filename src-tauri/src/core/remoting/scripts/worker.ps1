# WinGUI session worker (Windows PowerShell 5.1).
#
# Runs inside a hidden powershell.exe started by core::remoting::worker. The
# bootstrap dot-sources this file, so it shares the bootstrap's scope and uses
# its two streams:
#
#   $WgIn   UTF-8 StreamReader over the process's stdin   (requests, one JSON per line)
#   $WgOut  UTF-8 StreamWriter over the process's stdout  (replies,  one JSON per line)
#
# Raw std streams are used instead of [Console]::In/Out so the encoding never
# depends on the console code page: PowerShell 5.1 would otherwise mangle
# non-ASCII file names (å, ä, ö) on the way out.
#
# Inputs (environment variables of this process only, never arguments):
#   WG_HOST, WG_USER, WG_PASSWORD   the target and credential
#   WG_LOCAL=1                      run operations on this machine with no
#                                   PSSession (tests only; never set by the app)
#
# Protocol
#   <- {"kind":"ready","data":{...}}                    once, when the session is open
#   <- {"kind":"fatal","code":"...","message":"..."}    once, instead of ready; then exit
#   -> {"id":"7","op":"fs.drives","args":{...}}
#   <- {"id":"7","kind":"result","data":<json>}
#   <- {"id":"7","kind":"error","code":"...","message":"..."}
#
# Rules every operation follows
#   * Operations are the fixed table $Ops below. A request picks one by name; it
#     can never supply script text. Request values reach the remote side only as
#     -ArgumentList data.
#   * The remote script block returns ONE pre-serialised JSON string, not a
#     collection of objects: shipping thousands of objects through PowerShell's
#     remoting serialiser is slow, a single string is not.
#   * Requests are served one at a time. Cancelling an operation means the app
#     kills this process and starts a new one.

$ErrorActionPreference = 'Stop'

function Send-Line([string]$line) { $WgOut.WriteLine($line) }

function ConvertTo-JsonString([string]$s) {
    # A JSON string literal, built without ConvertTo-Json so it is cheap and
    # never wraps or pretty-prints.
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    foreach ($ch in $s.ToCharArray()) {
        $c = [int]$ch
        if     ($c -eq 34) { [void]$sb.Append('\"') }
        elseif ($c -eq 92) { [void]$sb.Append('\\') }
        elseif ($c -eq 10) { [void]$sb.Append('\n') }
        elseif ($c -eq 13) { [void]$sb.Append('\r') }
        elseif ($c -eq 9)  { [void]$sb.Append('\t') }
        elseif ($c -lt 32) { [void]$sb.Append(('\u{0:x4}' -f $c)) }
        else               { [void]$sb.Append($ch) }
    }
    [void]$sb.Append('"')
    $sb.ToString()
}

function Send-Error([string]$id, [string]$code, [string]$message) {
    Send-Line ('{"id":' + (ConvertTo-JsonString $id) + ',"kind":"error","code":' +
        (ConvertTo-JsonString $code) + ',"message":' + (ConvertTo-JsonString $message) + '}')
}

function Send-Fatal([string]$code, [string]$message) {
    Send-Line ('{"kind":"fatal","code":' + (ConvertTo-JsonString $code) + ',"message":' +
        (ConvertTo-JsonString $message) + '}')
}

# Why a session could not be opened. The numeric codes are stable across
# Windows display languages; the phrases are a best-effort backstop.
function Get-ConnectErrorCode([string]$m) {
    if ($m -match '0x803381a6|MaxShellsPerUser|maximum number of concurrent shells|MaxConcurrentUsers|exceeds the maximum') { return 'QuotaExceeded' }
    if ($m -match '0x803380e4|TrustedHosts') { return 'NotTrusted' }
    if ($m -match '0x8009030e|0x8009030c|0x80070005|0x8007052e|Access is denied|user name or password is incorrect|Logon failure|logon session does not exist') { return 'AuthFailed' }
    if ($m -match '0x80338012|0x80338126|0x80072ee2|0x80072efd|0x80090311|cannot find the computer|network path was not found|cannot complete the operation|timed out|could not be resolved|No such host') { return 'Unreachable' }
    return 'ConnectFailed'
}

# Why an operation failed, as a stable code the UI can react to.
function Get-OperationErrorCode($err) {
    $ex = $err.Exception
    $m = [string]$ex.Message
    $fq = [string]$err.FullyQualifiedErrorId
    if ($ex -is [System.Management.Automation.Remoting.PSRemotingTransportException]) { return 'SessionLost' }
    if ($script:Session -and $script:Session.State -ne 'Opened') { return 'SessionLost' }
    if ($m -match 'WG-UNKNOWN-OP') { return 'UnknownOp' }
    if ($fq -match 'UnauthorizedAccess' -or $m -match 'Access to the path .* is denied|Access is denied') { return 'AccessDenied' }
    if ($fq -match 'PathNotFound|ItemNotFound|DirectoryNotFound|FileNotFound' -or $m -match 'Could not find (a part of the path|file)|does not exist|Cannot find path') { return 'NotFound' }
    if ($fq -match 'PathTooLong' -or $m -match 'path is too long|specified path, file name, or both are too long') { return 'PathTooLong' }
    if ($m -match 'being used by another process') { return 'InUse' }
    return 'Error'
}

# Run a script block on the target: through the held session, or directly on
# this machine in test mode.
function Invoke-Target([scriptblock]$Script, [object[]]$Arguments = @()) {
    if ($script:Session) {
        Invoke-Command -Session $script:Session -ScriptBlock $Script -ArgumentList $Arguments
    } else {
        & $Script @Arguments
    }
}

# ---------------------------------------------------------------------------
# Operations. Each takes the request's args object and returns a JSON string.
# ---------------------------------------------------------------------------
$Ops = @{}

$Ops['ping'] = {
    param($a)
    Invoke-Target {
        '{"computerName":"' + $env:COMPUTERNAME + '","utcNow":"' + [DateTime]::UtcNow.ToString('o') + '"}'
    }
}

$Ops['fs.drives'] = {
    param($a)
    Invoke-Target {
        $drives = foreach ($d in [System.IO.DriveInfo]::GetDrives()) {
            $o = [ordered]@{
                name        = $d.Name.TrimEnd('\')
                driveType   = [string]$d.DriveType
                ready       = [bool]$d.IsReady
                label       = ''
                fileSystem  = ''
                totalBytes  = 0
                freeBytes   = 0
            }
            if ($d.IsReady) {
                try {
                    $o.label = [string]$d.VolumeLabel
                    $o.fileSystem = [string]$d.DriveFormat
                    $o.totalBytes = [long]$d.TotalSize
                    $o.freeBytes = [long]$d.AvailableFreeSpace
                } catch { $o.ready = $false }
            }
            [pscustomobject]$o
        }
        ConvertTo-Json -InputObject @($drives) -Compress -Depth 3
    }
}

# ---------------------------------------------------------------------------
# Open the session
# ---------------------------------------------------------------------------
$script:Session = $null
try {
    if ($env:WG_LOCAL -ne '1') {
        $sec = ConvertTo-SecureString $env:WG_PASSWORD -AsPlainText -Force
        $cred = New-Object System.Management.Automation.PSCredential($env:WG_USER, $sec)
        # The password has done its job; don't leave it readable in this
        # process's environment for the life of the session.
        Remove-Item Env:WG_PASSWORD -ErrorAction SilentlyContinue
        $sec = $null
        $opt = New-PSSessionOption -OpenTimeout 20000
        $script:Session = New-PSSession -ComputerName $env:WG_HOST -Credential $cred -Authentication Negotiate -SessionOption $opt
    }
    $info = Invoke-Target {
        $os = ''
        try { $os = [string](Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).Caption } catch { }
        ConvertTo-Json -Compress -InputObject ([ordered]@{
            computerName = [string]$env:COMPUTERNAME
            user         = [Environment]::UserDomainName + '\' + [Environment]::UserName
            os           = $os.Trim()
            psVersion    = $PSVersionTable.PSVersion.ToString()
            utcNow       = [DateTime]::UtcNow.ToString('o')
        })
    }
    Send-Line ('{"kind":"ready","data":' + $info + '}')
} catch {
    $m = [string]$_.Exception.Message
    Send-Fatal (Get-ConnectErrorCode $m) $m
    if ($script:Session) { Remove-PSSession $script:Session -ErrorAction SilentlyContinue }
    exit 2
}

# ---------------------------------------------------------------------------
# Serve requests until stdin closes
# ---------------------------------------------------------------------------
try {
    while ($true) {
        $line = $WgIn.ReadLine()
        if ($null -eq $line) { break }
        if ($line.Length -eq 0) { continue }

        $id = ''
        try {
            $req = $line | ConvertFrom-Json
            $id = [string]$req.id
            $op = $Ops[[string]$req.op]
            if (-not $op) { throw "WG-UNKNOWN-OP: no operation named '$($req.op)'" }
            $json = [string](& $op $req.args)
            if ([string]::IsNullOrEmpty($json)) { $json = 'null' }
            Send-Line ('{"id":' + (ConvertTo-JsonString $id) + ',"kind":"result","data":' + $json + '}')
        } catch {
            $code = Get-OperationErrorCode $_
            Send-Error $id $code ([string]$_.Exception.Message)
            # A dead session cannot serve anything else; exit so the pool
            # starts a fresh worker instead of failing every later request.
            if ($code -eq 'SessionLost') { break }
        }
    }
} finally {
    if ($script:Session) { Remove-PSSession $script:Session -ErrorAction SilentlyContinue }
}
