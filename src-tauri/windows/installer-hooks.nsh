; NSIS installer hooks (Tauri `bundle.windows.nsis.installerHooks`), used only by installer
; builds that layer on src-tauri/tauri.bundle.conf.json.
;
; * An MCP client (Claude Desktop, Claude Code, VS Code…) may be running iconarium-cli.exe,
;   and Windows can't replace or delete a running executable, so the hooks end those
;   processes first. Clients restart MCP servers on demand.
; * The install folder is added to the user's PATH so `iconarium-cli` works in terminals,
;   and removed again on uninstall (see iconarium-path.ps1). If that fails (e.g. a
;   PowerShell execution policy set by Group Policy), the install still succeeds and the
;   details log says so; the CLI then works by its full path.
;
; Every macro saves and restores $0 so it doesn't disturb the template's own use of it.

!macro ICONARIUM_STOP_CLI
  Push $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /IM iconarium-cli.exe'
  Pop $0
  ; The image file can stay locked for a moment after the process ends.
  Sleep 500
  Pop $0
!macroend

!macro ICONARIUM_PATH ACTION
  Push $0
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\iconarium-path.ps1" -Action ${ACTION} -Dir "$INSTDIR"'
  Pop $0
  StrCmp $0 "0" +2 0
    DetailPrint "Iconarium: could not update PATH (${ACTION}, result $0). Use the full path: $INSTDIR\iconarium-cli.exe"
  ; Tell running programs (Explorer, new terminals) that the environment changed.
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ICONARIUM_STOP_CLI
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro ICONARIUM_PATH add
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ICONARIUM_STOP_CLI
  !insertmacro ICONARIUM_PATH remove
!macroend
