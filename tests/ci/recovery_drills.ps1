<#
.SYNOPSIS
    OWNER-RUN ONLY. Recovery drill checklist R-A .. R-E for Kenali v1 sign-off.

.DESCRIPTION
    OWNER-RUN: this file is DELIVERED ONLY. It was never executed on the
    Linux/WSL2 build host. Each drill below is a labelled section containing
    EXACT MANUAL STEPS that the owner performs, by hand, in the disposable
    Windows 11 x64 VM (runbook section 1). An agent never executes them.

    HARD RULE: this script NEVER auto-registers a Credential Provider, NEVER
    edits the live registry on this host, NEVER installs/uninstalls/rolls back
    a service, NEVER opens a camera, NEVER writes Windows registry state.
    Calling any Drill-RA .. Drill-RE function only PRINTS the manual steps to
    the console and writes a redacted OWNER-RUN checklist token; the owner
    then follows the printed steps by hand and records the result in
    docs/reports/recovery_drills.md.

    Authority: docs/dev_safety_runbook.md section 4 (R-A .. R-E) is the
    authoritative wording; docs/test_plan.md section 12; docs/prd.md section 13.

.NOTES
    Owner use (elevated PowerShell in the VM), for example:
        .\recovery_drills.ps1 -OwnerRunConfirmed -Drill R-A
        .\recovery_drills.ps1 -OwnerRunConfirmed -Drill All
    Every drill must succeed once in the VM before v1 sign-off.
#>
[CmdletBinding()]
param(
    # Owner acknowledgement gate: no steps are printed without it.
    [switch]$OwnerRunConfirmed,

    # Which drill to show. Defaults to all five.
    [ValidateSet('R-A', 'R-B', 'R-C', 'R-D', 'R-E', 'All')]
    [string]$Drill = 'All',

    # Optional redacted checklist output file (owner fills the report separately).
    [string]$ChecklistPath = (Join-Path $env:TEMP 'kenali-recovery-drills.checklist.log')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $OwnerRunConfirmed) {
    throw 'OWNER-RUN: pass -OwnerRunConfirmed. Drill steps are delivered only and are never executed by an agent.'
}

# --- Redacted checklist emitter --------------------------------------------
# Only drill IDs and fixed status tokens are written: no secrets, SIDs,
# hostnames or raw command output. The manual registry commands below are
# DISPLAY TEXT for the owner to run by hand on the VM, never invoked here.
$script:ChecklistPath = $ChecklistPath
function Write-DrillChecklist {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('R-A', 'R-B', 'R-C', 'R-D', 'R-E')]
        [string]$Id,
        [Parameter(Mandatory = $true)]
        [ValidateSet('STEPS_SHOWN', 'OWNER_RUN_PENDING')]
        [string]$Status
    )
    $line = '{0:o} drill={1} status={2}' -f (Get-Date), $Id, $Status
    Add-Content -LiteralPath $script:ChecklistPath -Value $line -Encoding UTF8
}

function Show-DrillSteps {
    param(
        [Parameter(Mandatory = $true)] [string]$Id,
        [Parameter(Mandatory = $true)] [string]$Title,
        [Parameter(Mandatory = $true)] [string[]]$Steps,
        [Parameter(Mandatory = $true)] [string]$PassCondition
    )
    Write-Host ""
    Write-Host "===== OWNER-RUN DRILL $Id : $Title ====="
    Write-Host "Manual steps (execute by hand in the VM; this script does NOT run them):"
    $n = 1
    foreach ($step in $Steps) {
        Write-Host ("  {0}. {1}" -f $n, $step)
        $n++
    }
    Write-Host "PASS CONDITION: $PassCondition"
    Write-Host "Record the result in docs/reports/recovery_drills.md (VERIFY / OWNER-RUN / NOT EXECUTED)."
    Write-DrillChecklist -Id $Id -Status 'STEPS_SHOWN'
    Write-DrillChecklist -Id $Id -Status 'OWNER_RUN_PENDING'
}

# ---------------------------------------------------------------------------
# R-A — Normal password fallback
# Goal: any failure or service-down state leaves password/PIN path working
# instantly (FR-03). Authority: runbook section 4, R-A.
# ---------------------------------------------------------------------------
function Drill-RA {
    Show-DrillSteps -Id 'R-A' -Title 'Normal password fallback' -Steps @(
        'In the VM, sign in normally with the account password.',
        'Kill kenali-svc.exe (Task Manager -> Details -> End task) or stop the service with: sc stop KenaliSvc',
        'Lock the screen (Win+L). The tile must read "Face unlock unavailable" within <= 300 ms.',
        'Type the account password at the normal password tile -> desktop reached.'
    ) -PassCondition 'Password works immediately, no delay, no face tile blocking it.'
}

# ---------------------------------------------------------------------------
# R-B — Kill switch
# Goal: registry kill switch removes the tile at next session creation (FR-10).
# Runbook section 3 gives the exact values; owner runs them by hand on the VM.
# ---------------------------------------------------------------------------
function Drill-RB {
    Show-DrillSteps -Id 'R-B' -Title 'Kill switch' -Steps @(
        'On the VM, in an elevated PowerShell, set BOTH REG_DWORD values to 1 (runbook section 3):',
        '  $clsid = "<KENALI_CP_CLSID from crates/kenali-winapi/src/com.rs>"',
        '  New-Item -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid" -Force',
        '  New-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid" -Name Disabled -PropertyType DWord -Value 1 -Force',
        '  New-Item -Path "HKLM:\SOFTWARE\Kenali" -Force',
        '  New-ItemProperty -Path "HKLM:\SOFTWARE\Kenali" -Name Disabled -PropertyType DWord -Value 1 -Force',
        'Lock / sign out and back in on the VM.'
    ) -PassCondition 'No Kenali tile; password/PIN works; UI shows kill-switch state read-only. Re-enable afterwards (delete both Disabled values or set them to 0) and confirm the tile returns.'
}

# ---------------------------------------------------------------------------
# R-C — Safe Mode
# Goal: Safe Mode boots with only the built-in password provider [R12][R13].
# ---------------------------------------------------------------------------
function Drill-RC {
    Show-DrillSteps -Id 'R-C' -Title 'Safe Mode' -Steps @(
        'On the VM: Settings -> System -> Recovery -> Advanced startup -> Restart now, or hold Shift while choosing Restart.',
        'Troubleshoot -> Advanced options -> Startup Settings -> Restart -> press 4 (or F4) for Safe Mode.',
        'Sign in at the plain built-in username/password provider.'
    ) -PassCondition 'Sign-in succeeds with Kenali CP present-but-inert; the default password provider is the only one offered. Return to normal boot.'
}

# ---------------------------------------------------------------------------
# R-D — WinRE (when Safe Mode is unreachable)
# Goal: disable the CP offline via the Windows Recovery Environment prompt.
# ---------------------------------------------------------------------------
function Drill-RD {
    Show-DrillSteps -Id 'R-D' -Title 'WinRE (offline disable)' -Steps @(
        'Boot the VM to WinRE (Shift+Restart, or Windows installer USB -> Repair your computer).',
        'Choose Troubleshoot -> Advanced options -> Command Prompt.',
        'Identify the Windows partition (often C: in WinRE, but verify): dir C:\Windows / bcdedit /enum',
        'Load the offline hives: reg load HKLM\KENALI_OFFLINE_SYSTEM C:\Windows\System32\config\SYSTEM ; reg load HKLM\KENALI_OFFLINE_SOFTWARE C:\Windows\System32\config\SOFTWARE',
        'Set Disabled=1 on the provider CLSID under the offline SOFTWARE hive, and cover the well-known path:',
        '  reg add "HKLM\KENALI_OFFLINE_SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{CLSID}" /v Disabled /t REG_DWORD /d 1 /f',
        '  reg add "HKLM\KENALI_OFFLINE_SOFTWARE\Kenali" /v Disabled /t REG_DWORD /d 1 /f',
        'Alternative if hive editing is uncertain: rename the DLL (CP fails to load, password provider remains):',
        '  ren "C:\Program Files\Kenali\<version>\kenali-cp.dll" kenali-cp.dll.disabled',
        'Unload the hives and reboot: reg unload HKLM\KENALI_OFFLINE_SOFTWARE ; reg unload HKLM\KENALI_OFFLINE_SYSTEM ; exit'
    ) -PassCondition 'Normal boot presents the password provider; sign-in succeeds; Kenali tile absent. VERIFY the exact offline hive path/ControlSet on the reference build; renaming the DLL is the robust fallback.'
}

# ---------------------------------------------------------------------------
# R-E — Versioned rollback
# Goal: bad upgrade reversible without hand-editing the registry (FR-15).
# ---------------------------------------------------------------------------
function Drill-RE {
    Show-DrillSteps -Id 'R-E' -Title 'Versioned rollback' -Steps @(
        'Confirm installs live at C:\Program Files\Kenali\<version>\ and the previous version directory is kept.',
        'In the VM, as owner, run: cargo xtask rollback (rewrites InprocServer32 default value to the previous absolute DLL path; restores prior registration).',
        'Lock / sign out and back in; confirm the previous version tile loads.'
    ) -PassCondition 'Previous version works; no in-place edit of a registered DLL was performed. If rollback fails, fall back to R-D (rename DLL / offline disable).'
}

# --- Dispatch ---------------------------------------------------------------
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ChecklistPath) | Out-Null
Write-Host 'OWNER-RUN: recovery drills are manual. This script only PRINTS steps and writes a redacted checklist token; it changes no registry, registers no CP and runs no install/rollback command.'
Write-Host "Redacted checklist: $ChecklistPath"

switch ($Drill) {
    'R-A' { Drill-RA }
    'R-B' { Drill-RB }
    'R-C' { Drill-RC }
    'R-D' { Drill-RD }
    'R-E' { Drill-RE }
    'All' {
        Drill-RA
        Drill-RB
        Drill-RC
        Drill-RD
        Drill-RE
    }
}

Write-Host ''
Write-Host 'All shown drills: VERIFY / OWNER-RUN / NOT EXECUTED until the owner fills docs/reports/recovery_drills.md with real results.'
