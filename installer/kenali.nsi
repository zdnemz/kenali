; ---------------------------------------------------------------------------
; Kenali Windows installer (NSIS, Unicode, x86_64)
;
; OWNER-RUN WARNING
; ----------------
; This installer registers a Credential Provider (CP) in-process with LogonUI
; and starts a Windows service. A faulty CP can affect logon / lock the owner
; out. Install only from an elevated session, VM first, and read
; docs/dev_safety_runbook.md BEFORE running it:
;   - VM-first ladder (section 1): VM -> secondary host account -> main account
;   - pre-flight checklist (section 2): restore point / VM checkpoint mandatory
;   - kill switch (section 3): Disabled=1 under the provider CLSID key AND
;     HKLM\SOFTWARE\Kenali takes effect at the next session creation
;   - password / PIN fallback always remains available (section 5)
;   - R-C Safe Mode (section 6) and R-D WinRE (section 7) recovery
;   - R-E versioned rollback (section 8)
; DO NOT run this installer on a machine without a verified recovery path.
;
; No signing keys or secrets are embedded here. The four binaries and any
; optional PDB / SBOM / support files must already be signed by the release
; workflow before makensis.exe compiles this script.
;
; Required defines (all supplied by the workflow via /D; no hidden defaults):
;   KENALI_VERSION, KENALI_CLSID, KENALI_STAGE_DIR,
;   KENALI_SVC_EXE, KENALI_CP_DLL, KENALI_UI_EXE, KENALI_CLI_EXE
; ---------------------------------------------------------------------------

!ifndef KENALI_VERSION
  !error "KENALI_VERSION must be defined (e.g. /DKENALI_VERSION=0.1.0)"
!endif
!ifndef KENALI_CLSID
  !error "KENALI_CLSID must be defined as the 36-char Kenali CP GUID (no braces)"
!endif
!ifndef KENALI_STAGE_DIR
  !error "KENALI_STAGE_DIR must be defined (directory holding staged release files)"
!endif
!ifndef KENALI_SVC_EXE
  !error "KENALI_SVC_EXE must be defined (absolute path to staged kenali-svc.exe)"
!endif
!ifndef KENALI_CP_DLL
  !error "KENALI_CP_DLL must be defined (absolute path to staged kenali-cp.dll)"
!endif
!ifndef KENALI_UI_EXE
  !error "KENALI_UI_EXE must be defined (absolute path to staged kenali-ui.exe)"
!endif
!ifndef KENALI_CLI_EXE
  !error "KENALI_CLI_EXE must be defined (absolute path to staged kenali.exe)"
!endif

Name "Kenali ${KENALI_VERSION}"
OutFile "kenali-${KENALI_VERSION}-x86_64-pc-windows-msvc-setup.exe"
InstallDir "C:\Program Files\Kenali\${KENALI_VERSION}"
InstallDirRegKey HKLM "SOFTWARE\Kenali" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "nsExec.nsh"

; ---------------------------------------------------------------------------
; Registry keys (HKLM only; owner recovery state is preserved)
; ---------------------------------------------------------------------------
!define KENALI_REG_ROOT        "SOFTWARE\Kenali"
!define KENALI_CLSID_KEY       "SOFTWARE\Classes\CLSID\{{${KENALI_CLSID}}}"
!define KENALI_INPROC_KEY      "SOFTWARE\Classes\CLSID\{{${KENALI_CLSID}}\InprocServer32"
!define KENALI_CP_KEY          "SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{{${KENALI_CLSID}}}"
!define KENALI_CP_NAME         "Kenali Credential Provider"
!define KENALI_SERVICE         "KenaliSvc"
!define KENALI_SERVICE_DISPLAY "Kenali Service"
!define KENALI_UNINSTALL_KEY   "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Kenali-${KENALI_VERSION}"
!define KENALI_START_MENU      "Kenali"

Var PreviousVersion
Var PreviousDir

; ---------------------------------------------------------------------------
; Helpers
; ---------------------------------------------------------------------------

; Join base and leaf into an absolute path. Uses dedicated global variables so
; no stack manipulation is needed inside the function.
Var JoinBase
Var JoinLeaf
Var JoinResult

!macro KenaliJoinPath out base leaf
  StrCpy $JoinBase "${base}"
  StrCpy $JoinLeaf "${leaf}"
  Call KenaliJoin
  StrCpy "${out}" "$JoinResult"
!macroend

Function KenaliJoin
  StrCpy $JoinResult "$JoinBase\$JoinLeaf"
  ${GetFullPath} $JoinResult "$JoinResult"
FunctionEnd

; Check whether a previous version directory contains valid binaries.
; Invalid / missing binaries -> "0", do not repoint registration at it.
Var PrevValidDir
Var PrevValidResult
Function KenaliPreviousValid
  StrCpy $PrevValidDir "$0"
  StrCpy $PrevValidResult "0"
  ${If} ${FileExists} "$PrevValidDir\kenali-cp.dll"
  ${AndIf} ${FileExists} "$PrevValidDir\kenali-svc.exe"
  ${AndIf} ${FileExists} "$PrevValidDir\kenali-ui.exe"
  ${AndIf} ${FileExists} "$PrevValidDir\kenali.exe"
    StrCpy $PrevValidResult "1"
  ${EndIf}
  StrCpy $0 "$PrevValidResult"
FunctionEnd

; Create a HKLM key only when it does not exist and write Disabled=0 there.
; If the key already exists, its Disabled value is left untouched: an owner
; kill switch Disabled=1 must survive install and uninstall.
Var EnsureKeyPath
Function KenaliEnsureEnabledKey
  StrCpy $EnsureKeyPath "$0"
  IfRegKeyExists HKLM "$EnsureKeyPath" keyExists
  WriteRegDWORD HKLM "$EnsureKeyPath" "Disabled" 0
keyExists:
  ; Fresh key not created -> never overwrite a possible owner Disabled=1.
FunctionEnd

Function .onInit
  ; Read previous installation state BEFORE any write. $INSTDIR defaults to
  ; this version; InstallDirRegKey may already have changed it, so always
  ; resolve the versioned target explicitly from the pinned prefix.
  StrCpy $INSTDIR "C:\Program Files\Kenali\${KENALI_VERSION}"
  ${GetFullPath} $INSTDIR "$INSTDIR"

  StrCpy $PreviousVersion ""
  StrCpy $PreviousDir ""

  ReadRegStr $0 HKLM "${KENALI_REG_ROOT}" "InstallDir"
  ${If} $0 != ""
    ${GetFullPath} $1 "$0"
    ${If} $1 == $INSTDIR
      MessageBox MB_ICONSTOP|MB_TOPMOST "Kenali ${KENALI_VERSION} is already installed at $\n$INSTDIR $\n$\nIn-place edits are never allowed. Uninstall that version (or use R-E rollback from docs/dev_safety_runbook.md) before installing again."
      Abort
    ${EndIf}
    ; Record the leaf (version string) and keep that directory untouched.
    ${GetFileName} $2 "$1"
    StrCpy $PreviousVersion "$2"
    StrCpy $PreviousDir "$1"
  ${EndIf}

  ; One-line owner warning in the UI as well as in the header comments.
  MessageBox MB_ICONEXCLAMATION|MB_TOPMOST|MB_OKCANCEL \
    "OWNER-RUN INSTALL: this registers a Credential Provider in LogonUI and starts the Kenali service.$\nA faulty CP can affect logon. Password/PIN fallback, kill switch, Safe Mode and WinRE recovery are documented in docs/dev_safety_runbook.md; take a VM checkpoint / restore point first.$\n$\nContinue only on the intended VM or test account?" \
    IDOK proceed
  Abort
proceed:
FunctionEnd

; ---------------------------------------------------------------------------
; Installation
; ---------------------------------------------------------------------------
Section "Kenali ${KENALI_VERSION}" SecInstall
  SectionIn RO
  SetOutPath "$INSTDIR"
  CreateDirectory "$INSTDIR"

  ; Four staged release binaries. All names are fixed by the shared contract;
  ; their absolute staged sources are passed as /D defines by the workflow.
  File "/oname=kenali-svc.exe" "${KENALI_SVC_EXE}"
  File "/oname=kenali-cp.dll"  "${KENALI_CP_DLL}"
  File "/oname=kenali-ui.exe"  "${KENALI_UI_EXE}"
  File "/oname=kenali.exe"     "${KENALI_CLI_EXE}"

  ; Optional PDB / SBOM / support files: silently skip any that are absent.
  ; Naming is fixed by xtask package: four PDBs plus
  ; kenali-${KENALI_VERSION}.cdx.xml.
  File /nonfatal /oname=kenali-svc.pdb "${KENALI_STAGE_DIR}\kenali-svc.pdb"
  File /nonfatal /oname=kenali-cp.pdb  "${KENALI_STAGE_DIR}\kenali-cp.pdb"
  File /nonfatal /oname=kenali-ui.pdb  "${KENALI_STAGE_DIR}\kenali-ui.pdb"
  File /nonfatal /oname=kenali.pdb     "${KENALI_STAGE_DIR}\kenali.pdb"
  File /nonfatal "kenali-${KENALI_VERSION}.cdx.xml"

  ; Start Menu: required UI shortcut, optional CLI and Uninstall shortcuts.
  CreateDirectory "$SMPROGRAMS\${KENALI_START_MENU}"
  CreateShortCut "$SMPROGRAMS\${KENALI_START_MENU}\Kenali UI.lnk" "$INSTDIR\kenali-ui.exe"
  CreateShortCut "$SMPROGRAMS\${KENALI_START_MENU}\Kenali CLI.lnk" "$INSTDIR\kenali.exe" "" "$INSTDIR\kenali.exe" 0
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${KENALI_START_MENU}\Uninstall Kenali ${KENALI_VERSION}.lnk" "$INSTDIR\Uninstall.exe"

  ; --- Registration. The user explicitly chose auto-registration. ----------
  ; Credential Provider COM class and InprocServer32 point at this version.
  WriteRegStr HKLM "${KENALI_CLSID_KEY}" "" "${KENALI_CP_NAME}"
  WriteRegStr HKLM "${KENALI_INPROC_KEY}" "" "$INSTDIR\kenali-cp.dll"
  WriteRegStr HKLM "${KENALI_INPROC_KEY}" "ThreadingModel" "Apartment"

  ; Provider key: write Disabled=0 ONLY when creating a NEW key. If an owner
  ; already set Disabled=1 as a kill switch, it is preserved verbatim.
  ; Check existence BEFORE writing the default value (which creates the key).
  StrCpy $0 "${KENALI_CP_KEY}"
  Call KenaliEnsureEnabledKey
  WriteRegStr HKLM "${KENALI_CP_KEY}" "" "${KENALI_CP_NAME}"

  ; Application key: same fresh-key-only rule for the global kill switch.
  ; Check BEFORE WriteRegStr InstallDir creates the key.
  StrCpy $0 "${KENALI_REG_ROOT}"
  Call KenaliEnsureEnabledKey
  WriteRegStr HKLM "${KENALI_REG_ROOT}" "InstallDir" "$INSTDIR"
  ${If} $PreviousVersion != ""
    WriteRegStr HKLM "${KENALI_REG_ROOT}" "PreviousVersion" "$PreviousVersion"
  ${EndIf}

  ; Service registration via sc.exe, as pinned in the contract. The quoted
  ; ImagePath is REG_EXPAND_SZ and uses this version's absolute service binary.
  nsExec::ExecToLog 'sc.exe create "${KENALI_SERVICE}" binPath= "\"$INSTDIR\kenali-svc.exe\"" DisplayName= "${KENALI_SERVICE_DISPLAY}" start= 2 type= 16 error= 1'
  ; Re-assert all four values explicitly (config also covers re-registration
  ; after a previous service was removed by an owner).
  nsExec::ExecToLog 'sc.exe config "${KENALI_SERVICE}" binPath= "\"$INSTDIR\kenali-svc.exe\"" DisplayName= "${KENALI_SERVICE_DISPLAY}" start= 2 type= 16 errorControl= 1'
  nsExec::ExecToLog 'sc.exe start "${KENALI_SERVICE}"'

  WriteRegStr HKLM "${KENALI_UNINSTALL_KEY}" "DisplayName" "Kenali ${KENALI_VERSION} (x86_64)"
  WriteRegStr HKLM "${KENALI_UNINSTALL_KEY}" "DisplayVersion" "${KENALI_VERSION}"
  WriteRegStr HKLM "${KENALI_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${KENALI_UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKLM "${KENALI_UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${KENALI_UNINSTALL_KEY}" "NoRepair" 1

  MessageBox MB_ICONINFORMATION|MB_TOPMOST \
    "Kenali ${KENALI_VERSION} installed to $\n$INSTDIR $\n$\nCredential Provider and ${KENALI_SERVICE} are registered. Lock / sign out and back in to check that password/PIN fallback still works. If logon is affected, use the kill switch or Safe Mode / WinRE recovery in docs/dev_safety_runbook.md."
SectionEnd

; ---------------------------------------------------------------------------
; Uninstall: unregister current version, restore a valid previous version,
; remove only this version's files. Other version directories are never
; touched, and owner kill switches are never silently cleared.
; ---------------------------------------------------------------------------
; Section Uninstall — NSIS special section, runs when the installed copy is
; removed. It unregisters the current version first, then restores a valid
; previous version when present.
Section "Uninstall"
  ; Capture previous-version state BEFORE deleting any registry values.
  ReadRegStr $PreviousVersion HKLM "${KENALI_REG_ROOT}" "PreviousVersion"

  ; Always unregister the current version first so we never leave the CP
  ; pointing at files we are about to remove.
  nsExec::ExecToLog 'sc.exe stop "${KENALI_SERVICE}"'
  nsExec::ExecToLog 'sc.exe delete "${KENALI_SERVICE}"'

  DeleteRegKey HKLM "${KENALI_CLSID_KEY}"
  DeleteRegKey HKLM "${KENALI_CP_KEY}"
  DeleteRegValue HKLM "${KENALI_REG_ROOT}" "InstallDir"
  DeleteRegValue HKLM "${KENALI_REG_ROOT}" "PreviousVersion"

  ; $INSTDIR is always this version's absolute directory; never read it from
  ; a writable registry string at uninstall time.
  StrCpy $INSTDIR "C:\Program Files\Kenali\${KENALI_VERSION}"
  ${GetFullPath} $INSTDIR "$INSTDIR"

  StrCpy $PreviousDir ""
  ${If} $PreviousVersion != ""
    ; The previous directory is derived from the pinned prefix + leaf; never
    ; from an arbitrary registry string.
    ${KenaliJoinPath} $PreviousDir "C:\Program Files\Kenali" "$PreviousVersion"
    StrCpy $0 "$PreviousDir"
    Call KenaliPreviousValid
    StrCpy $0 "$PrevValidResult"
    ${If} $0 == "1"
      ; R-E: repoint the CP and service at the previous version's binaries.
      WriteRegStr HKLM "${KENALI_CLSID_KEY}" "" "${KENALI_CP_NAME}"
      WriteRegStr HKLM "${KENALI_INPROC_KEY}" "" "$PreviousDir\kenali-cp.dll"
      WriteRegStr HKLM "${KENALI_INPROC_KEY}" "ThreadingModel" "Apartment"
      ; Only set Disabled=0 when we had to create the provider key; any
      ; owner-set Disabled=1 remains in place. Check BEFORE writing the
      ; default value (which itself creates the key).
      IfRegKeyExists HKLM "${KENALI_CP_KEY}" cpKeyStillExists
      WriteRegStr HKLM "${KENALI_CP_KEY}" "" "${KENALI_CP_NAME}"
      WriteRegDWORD HKLM "${KENALI_CP_KEY}" "Disabled" 0
      Goto cpKeyDone
cpKeyStillExists:
      WriteRegStr HKLM "${KENALI_CP_KEY}" "" "${KENALI_CP_NAME}"
cpKeyDone:

      WriteRegStr HKLM "${KENALI_REG_ROOT}" "InstallDir" "$PreviousDir"
      WriteRegStr HKLM "${KENALI_REG_ROOT}" "PreviousVersion" "$PreviousVersion"

      nsExec::ExecToLog 'sc.exe create "${KENALI_SERVICE}" binPath= "\"$PreviousDir\kenali-svc.exe\"" DisplayName= "${KENALI_SERVICE_DISPLAY}" start= 2 type= 16 error= 1'
      nsExec::ExecToLog 'sc.exe config "${KENALI_SERVICE}" binPath= "\"$PreviousDir\kenali-svc.exe\"" DisplayName= "${KENALI_SERVICE_DISPLAY}" start= 2 type= 16 errorControl= 1'
      nsExec::ExecToLog 'sc.exe start "${KENALI_SERVICE}"'
      MessageBox MB_ICONINFORMATION|MB_TOPMOST \
        "Kenali ${KENALI_VERSION} was uninstalled. The previous version ($PreviousVersion) was restored from $\n$PreviousDir $\n$\nIts directory was preserved. Owner kill-switch values were not changed."
    ${Else}
      ; No valid previous binaries: leave the machine fully unregistered.
      ; Do NOT clear HKLM\SOFTWARE\Kenali\Disabled if the owner set it.
      MessageBox MB_ICONINFORMATION|MB_TOPMOST \
        "Kenali ${KENALI_VERSION} was uninstalled and unregistered. No valid previous version binary was found, so nothing was re-registered. The kill switch in HKLM\SOFTWARE\Kenali was preserved."
    ${EndIf}
  ${Else}
    MessageBox MB_ICONINFORMATION|MB_TOPMOST \
      "Kenali ${KENALI_VERSION} was uninstalled and unregistered. Recovery instructions remain in docs/dev_safety_runbook.md."
  ${EndIf}

  ; Remove shortcuts and only this version's files / directory. The versioned
  ; directories of all other versions are left exactly as they were.
  Delete "$SMPROGRAMS\${KENALI_START_MENU}\Kenali UI.lnk"
  Delete "$SMPROGRAMS\${KENALI_START_MENU}\Kenali CLI.lnk"
  Delete "$SMPROGRAMS\${KENALI_START_MENU}\Uninstall Kenali ${KENALI_VERSION}.lnk"
  RMDir "$SMPROGRAMS\${KENALI_START_MENU}"

  Delete "$INSTDIR\kenali-svc.exe"
  Delete "$INSTDIR\kenali-cp.dll"
  Delete "$INSTDIR\kenali-ui.exe"
  Delete "$INSTDIR\kenali.exe"
  Delete "$INSTDIR\kenali-svc.pdb"
  Delete "$INSTDIR\kenali-cp.pdb"
  Delete "$INSTDIR\kenali-ui.pdb"
  Delete "$INSTDIR\kenali.pdb"
  Delete "$INSTDIR\kenali-${KENALI_VERSION}.cdx.xml"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${KENALI_UNINSTALL_KEY}"
SectionEnd
