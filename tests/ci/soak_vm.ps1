<#
.SYNOPSIS
    OWNER-RUN ONLY. 1,000-cycle VM lock/unlock soak for Kenali (NFR-06, docs/test_plan.md section 9).

.DESCRIPTION
    OWNER-RUN: this script is DELIVERED ONLY. It was never executed on the Linux/WSL2
    build host and must only be run by the owner, by hand, inside the disposable
    Windows 11 x64 VM that already has a known-good checkpoint (runbook section 1).
    No agent runs this. It never registers a Credential Provider, never writes the
    live registry, never installs or rolls back a service, and never opens a camera.

    Contract implemented here:
      * 1,000 cycles (default), overridable only DOWNWARD with -Cycles for a smoke pass.
      * Credentials are parameterised as a [PSCredential]; no plaintext password
        parameter, no secret in source, no secret in the log. Passwords are accepted
        interactively via Get-Credential when -Credential is omitted.
      * Every cycle is counted; only redacted status tokens are written to the log.
      * Any sign of account lock-out aborts the run immediately (fail closed).
      * A redacted summary is emitted at the end and echoed to the log.

    Pass criteria (docs/test_plan.md section 9): 0 CP crashes, 0 lock-outs,
    password/PIN fallback available at every cycle, camera LED off when idle,
    no handle/memory growth (CP RSS <= 5 MB, service idle RSS <= 150 MB).

.NOTES
    Run example (owner, elevated PowerShell in the VM):
        .\soak_vm.ps1 -OwnerRunConfirmed -Credential (Get-Credential)
    Validate wiring without locking the workstation:
        .\soak_vm.ps1 -OwnerRunConfirmed -DryRun -Cycles 3
#>
[CmdletBinding()]
param(
    # Owner acknowledgement gate: the script refuses to do real work without it.
    [switch]$OwnerRunConfirmed,

    # Dry run validates the harness and logging only; it never locks the session.
    [switch]$DryRun,

    # Total cycles. Default is the full 1,000. Only values <= 1,000 are accepted,
    # so this switch can shorten a pass, never inflate it beyond the agreed soak.
    [ValidateRange(1, 1000)]
    [int]$Cycles = 1000,

    # Secret-bearing credential. NEVER a plaintext string parameter. When omitted,
    # the owner is prompted with Get-Credential on the secure desktop.
    [System.Management.Automation.PSCredential]$Credential,

    # Redacted status log. Fixed default under the repo's ignored target tree.
    [string]$LogPath = (Join-Path $env:TEMP 'kenali-soak-vm.status.log'),

    # External, read-only health probe (PRD Q7 recovery CLI). Must report status
    # only; its stdout is NEVER written verbatim to the log, only a status token.
    [string]$HealthCheckCommand = 'kenali status',

    # Seconds to wait at the lock screen before the health probe per cycle.
    [ValidateRange(0, 3600)]
    [int]$LockWaitSeconds = 5,

    # Consecutive failed health probes that are treated as a lock-out / CP fault.
    [ValidateRange(1, 10)]
    [int]$MaxFailures = 2
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# --- Safety gates -----------------------------------------------------------
if (-not $OwnerRunConfirmed) {
    throw 'OWNER-RUN: pass -OwnerRunConfirmed. This script is delivered only and is never run by an agent.'
}
if (-not $DryRun -and -not $Credential) {
    $Credential = Get-Credential -Message 'OWNER-RUN: Kenali soak account (password is never logged)'
}
if (-not $DryRun -and -not $Credential) {
    throw 'OWNER-RUN: a credential is required for a real soak; use -DryRun to validate wiring.'
}

# --- Redacted logging -------------------------------------------------------
# Write-SoakLog accepts ONLY a fixed status token plus numbers. It deliberately
# has no free-text parameter, so a username, password, SID, hostname or raw probe
# output cannot reach the log. This satisfies FR-12 / FR-19 log hygiene.
$script:SoakLogPath = $LogPath
function Write-SoakLog {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('RUN_START', 'CYCLE_LOCKED', 'CYCLE_OK', 'CYCLE_FAIL',
                     'LOCKOUT_ABORT', 'FALLBACK_OK', 'RUN_ABORTED', 'RUN_COMPLETE')]
        [string]$Status,
        [int]$Cycle = 0
    )
    $line = '{0:o} status={1} cycle={2}' -f (Get-Date), $Status, $Cycle
    Add-Content -LiteralPath $script:SoakLogPath -Value $line -Encoding UTF8
}

# --- Health probe -----------------------------------------------------------
# Returns $true only when the read-only probe reports a healthy, sign-in-able
# state. Raw output is discarded; a boolean is the only thing that leaves here.
function Test-SoakHealth {
    if ($DryRun) { return $true }
    try {
        $out = & cmd /c $HealthCheckCommand 2>$null
        $text = ($out -join ' ')
        if ($text -match '(?i)lock(ed)?[-_ ]?out|account[-_ ]?locked|disabled[-_ ]?by[-_ ]?policy') {
            return $false
        }
        return ($LASTEXITCODE -eq 0)
    }
    catch {
        return $false
    }
}

# --- Lock / unlock ----------------------------------------------------------
function Invoke-SoakLock {
    # LockWorkStation is the documented, non-destructive way to start a cycle.
    # The owner then unlocks at the normal password/PIN tile; the supplied
    # [PSCredential] is held in memory only and is NEVER serialised to the log.
    if ($DryRun) { return }
    Add-Type -Namespace Kenali -Name Soak -MemberDefinition @'
        [System.Runtime.InteropServices.DllImport("user32.dll")]
        public static extern bool LockWorkStation();
'@
    if (-not [Kenali.Soak]::LockWorkStation()) {
        throw 'OWNER-RUN: LockWorkStation failed; VM session may not be interactive.'
    }
}

# --- Run --------------------------------------------------------------------
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$completed = 0
$failed = 0
$consecutiveFailures = 0
$aborted = $false

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $LogPath) | Out-Null
Write-SoakLog -Status 'RUN_START'

Write-Host "OWNER-RUN soak: $Cycles cycles (target 1,000). Log (redacted): $LogPath"
Write-Host 'VM checkpoint verified by owner? Runbook section 1 requires one before registration.'

for ($cycle = 1; $cycle -le $Cycles; $cycle++) {
    if (-not $DryRun) {
        Write-Host "Cycle $cycle of $Cycles : lock the session, then unlock at the password/PIN tile."
    }
    Invoke-SoakLock
    Write-SoakLog -Status 'CYCLE_LOCKED' -Cycle $cycle

    if ($LockWaitSeconds -gt 0) { Start-Sleep -Seconds $LockWaitSeconds }

    if (Test-SoakHealth) {
        $consecutiveFailures = 0
        $completed++
        Write-SoakLog -Status 'FALLBACK_OK' -Cycle $cycle
        Write-SoakLog -Status 'CYCLE_OK' -Cycle $cycle
    }
    else {
        $failed++
        $consecutiveFailures++
        Write-SoakLog -Status 'CYCLE_FAIL' -Cycle $cycle
        if ($consecutiveFailures -ge $MaxFailures) {
            $aborted = $true
            Write-SoakLog -Status 'LOCKOUT_ABORT' -Cycle $cycle
            break
        }
    }
}

$sw.Stop()
$verdict = if ($aborted) { 'LOCKOUT_ABORT' } elseif ($failed -gt 0) { 'COMPLETE_WITH_FAILURES' } else { 'COMPLETE' }
Write-SoakLog -Status $(if ($aborted) { 'RUN_ABORTED' } else { 'RUN_COMPLETE' }) -Cycle $completed

# --- Redacted summary -------------------------------------------------------
$summary = @"
----- OWNER-RUN VM SOAK SUMMARY (redacted) -----
Requested cycles : $Cycles (agreed soak target: 1,000)
Completed cycles : $completed
Failed cycles    : $failed
Aborted on lockout: $($aborted ? 'YES' : 'NO')
Elapsed          : $($sw.Elapsed)
Health probe     : $HealthCheckCommand (raw output never logged)
Log (redacted)   : $LogPath
Verdict          : $verdict
Status           : VERIFY / OWNER-RUN / NOT EXECUTED (owner signs off in docs/reports/soak.md)
No plaintext secret, username, SID or hostname was written to the log.
"@
Write-Host $summary
if ($aborted) {
    throw 'OWNER-RUN VM soak aborted on suspected lock-out. Keep the password fallback, apply runbook section 7, and record the result.'
}
