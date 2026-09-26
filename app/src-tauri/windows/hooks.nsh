; Aural installer hooks (Tauri NSIS). Uninstalling always removes the
; "start with Windows" entry. When the user ticks "Delete the application data",
; Tauri removes the identifier-named data folders; they are listed here too so the
; behaviour does not depend on the template version. The program folder
; ($LOCALAPPDATA\Aural) never holds data.
;
; Upgrading through the installer UI runs the old uninstaller first (without /UPDATE),
; which removes the Run entry. If settings survive the uninstall, a flag file in the
; settings folder remembers that autostart was on, and the next install restores it.
; Delete Aural removes the settings folder before uninstalling, so it never leaves a flag.

!define AURAL_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define AURAL_CONFIG_DIR "$APPDATA\com.aurathex.aural"
!define AURAL_RESTORE_FLAG "$APPDATA\com.aurathex.aural\restore-autostart"

!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "${AURAL_RESTORE_FLAG}"
    WriteRegStr HKCU "${AURAL_RUN_KEY}" "Aural" '"$INSTDIR\aural.exe" --autostart'
    Delete "${AURAL_RESTORE_FLAG}"
  ${EndIf}
!macroend

; Tauri's template deletes the Run entry before POSTUNINSTALL runs, so read it first.
Var AuralAutostartWasOn

!macro NSIS_HOOK_PREUNINSTALL
  ReadRegStr $AuralAutostartWasOn HKCU "${AURAL_RUN_KEY}" "Aural"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "${AURAL_RUN_KEY}" "Aural"
  ${If} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\com.aurathex.aural"
    RMDir /r "${AURAL_CONFIG_DIR}"
  ${ElseIf} $AuralAutostartWasOn != ""
  ${AndIf} ${FileExists} "${AURAL_CONFIG_DIR}\.aural-root"
    FileOpen $R8 "${AURAL_RESTORE_FLAG}" w
    FileClose $R8
  ${EndIf}
!macroend
