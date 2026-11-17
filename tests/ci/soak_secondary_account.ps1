<#
.SYNOPSIS
    OWNER-RUN ONLY. 2-week secondary-account soak for Kenali (docs/test_plan.md section 9).

.DESCRIPTION
    OWNER-RUN: this script is DELIVERED ONLY. It was never executed on the
    Linux/WSL2 build host and must only be run by the owner, by hand, on the
    second local administrator account of the reference laptop (VM-first ladder
    rung 2, runbook section 1). The MAIN account is never touched. An agent
    never runs this script. It never registers a CP, never writes the live
    registry, never installs or rolls back a service, never opens a camera.

    Contract implemented here:
      * 2-week / 14-day window, counted from -StartDate (default: first run).
      * Credentials are a [PSCredential]; no plaintext secret parameter, no
        secret in source or in the log. Omitted credentials prompt securely.
      * Each daily check-in is counted; only redacted status tokens are logged.
      * Any lock-out or unexpected disable aborts the soak immediately.
      * A redacted summary is emitted at the end.

    Pass criteria (docs/test_plan.md section 9): 0 lock-outs; allowed-SID guard
    holds (main account never sees the tile); no unexpected disables;
    refresh-credential works after a password change; hardware matrix events
    (test_plan section 10) handled. Deliberate interleaved failures: service
    kill mid-attempt, camera unplug/replug, password change, kill switch flip.

.NOTES
    Owner examples (elevated PowerShell on the SECONDARY admin account only):
        .\soak_secondary_account.ps1 -OwnerRunConfirmed -Credential (Get-Credential)
        .\soak_secondary_account.ps1 -OwnerRunConfirmed -DryRun
#>
[CmdletBinding()]
param(
    # Owner acknowledgement gate: no real work without it.
    [switch]$OwnerRunConfirmed,

    # Dry run validates the harness/logging only; never touches the session.
    [switch]$DryRun,

    # 2-week / 14-day soak window.
    [ValidateRange(1, 14)]
    [int]$Days = 14,

    # Start of the window; default is the moment of the first run.
    [datetime]$StartDate = (Get-Date),

    # Secret-bearing credential. NEVER a plaintext string parameter.
    [System.Management.Automation.PSCredential]$Credential,

    # Redacted status log. Default under the ignored temp tree.
    [string]$LogPath = (Join-Path $env:TEMP 'kenali-soak-secondary.status.log'),

    # Read-only health probe (PRD Q7 recovery CLI). Output is never logged.
    [string]$HealthCheckCommand = 'kenali status',

    # Seconds to wait between daily check-in polls (owner-run, foreground).
    [ValidateRange(1, 86400)]
    [int]$PollIntervalSeconds = 3600
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# --- Safety gates -----------------------------------------------------------
if (-not $OwnerRunConfirmed) {
    throw 'OWNER-RUN: pass -OwnerRunConfirmed. This script is delivered only and is never run by an agent.'
}
if (-not $DryRun -and -not $Credential) {
    $Credential = Get-Credential -Message 'OWNER-RUN: SECONDARY admin account (main account untouched; password is never logged)'
}
if (-not $DryRun -and -not $Credential) {
    throw 'OWNER-RUN: a credential is required for a real soak; use -DryRun to validate wiring.'
}

# --- Redacted logging -------------------------------------------------------
# Fixed status tokens only: no free text, so a username, password, SID or
# hostname cannot reach the log (FR-12 / FR-19).
$script:SoakLogPath = $LogPath
function Write-SoakLog {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('RUN_START', 'DAY_CHECKIN', 'DAY_OK', 'DAY_FAIL',
                     'LOCKOUT_ABORT', 'FALLBACK_OK', 'RUN_ABORTED', 'RUN_COMPLETE')]
        [string]$Status,
        [int]$Day = 0
    )
    $line = '{0:o} status={1} day={2}' -f (Get-Date), $Status, $Day
    Add-Content -LiteralPath $script:SoakLogPath -Value $line -Encoding UTF8
}

# --- Read-only health probe -------------------------------------------------
function Test-SoakHealth {
    if ($DryRun) { return $true }
    try {
        $out = & cmd /c $HealthCheckCommand 2>$null
        $text = ($out -join ' ')
        if ($text -match '(?i)lock(ed)?[-_ ]?out|account[-_ ]?locked|unexpected[-_ ]?disabl') {
            return $false
        }
        return ($LASTEXITCODE -eq 0)
    }
    catch {
        return $false
    }
}

# --- Run --------------------------------------------------------------------
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$checkins = 0
$failed = 0
$aborted = $false
$lastDay = -1

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $LogPath) | Out-Null
Write-SoakLog -Status 'RUN_START'

Write-Host "OWNER-RUN 2-week (14-day) secondary-account soak: $Days day(s) from $($StartDate.ToString('yyyy-MM-dd'))."
Write-Host 'Confirm with the owner: checkpoint taken, secondary local admin account active, MAIN account untouched.'
Write-Host 'Interleave deliberately (owner, manually): service kill mid-attempt, camera unplug/replug, password change + refresh-credential, kill switch flip.'
Write-Host "Redacted log: $LogPath"

while ($true) {
    $elapsed = (Get-Date) - $StartDate
    $day = [int][math]::Floor($elapsed.TotalDays) + 1
    if ($day -gt $Days) { break }

    if ($day -ne $lastDay) {
        $checkins++
        $lastDay = $day
        Write-SoakLog -Status 'DAY_CHECKIN' -Day $day
        Write-Host "Day $day of $Days : check password fallback, allowed-SID guard (main account sees no tile), and no unexpected disable."
    }

    if (Test-SoakHealth) {
        Write-SoakLog -Status 'FALLBACK_OK' -Day $day
        Write-SoakLog -Status 'DAY_OK' -Day $day
    }
    else {
        $failed++
        Write-SoakLog -Status 'DAY_FAIL' -Day $day
        $aborted = $true
        Write-SoakLog -Status 'LOCKOUT_ABORT' -Day $day
        break
    }

    if ($DryRun) { break }  # validate one check-in only
    Start-Sleep -Seconds $PollIntervalSeconds
}

$sw.Stop()
$verdict = if ($aborted) { 'LOCKOUT_ABORT' } elseif ($failed -gt 0) { 'COMPLETE_WITH_FAILURES' } else { 'COMPLETE' }
Write-SoakLog -Status $(if ($aborted) { 'RUN_ABORTED' } else { 'RUN_COMPLETE' }) -Day $checkins

# --- Redacted summary -------------------------------------------------------
$summary = @"
----- OWNER-RUN SECONDARY-ACCOUNT SOAK SUMMARY (redacted) -----
Soak duration     : 2-week / 14-day window (configured: $Days day(s))
Daily check-ins   : $checkins
Failed check-ins  : $failed
Aborted on lockout: $($aborted ? 'YES' : 'NO')
Elapsed           : $($sw.Elapsed)
Health probe      : $HealthCheckCommand (raw output never logged)
Log (redacted)    : $LogPath
Verdict           : $verdict
Status            : VERIFY / OWNER-RUN / NOT EXECUTED (owner signs off in docs/reports/soak.md)
Main account      : untouched by this script (allowed-SID guard is owner-verified)
No plaintext secret, username, SID or hostname was written to the log.
"@
Write-Host $summary
if ($aborted) {
    throw 'OWNER-RUN secondary-account soak aborted: suspected lock-out or unexpected disable. Apply runbook section 7; do not advance to the main account.'
}
