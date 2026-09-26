; Aural installer hooks (Tauri NSIS). Uninstalling always removes the
; "start with Windows" entry. When the user ticks "Delete the application data",
; Tauri removes the identifier-named data folders; they are listed here too so the
; behaviour does not depend on the template version. The program folder
; ($LOCALAPPDATA\Aural) never holds data.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Aural"
  ${If} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\com.aurathex.aural"
    RMDir /r "$APPDATA\com.aurathex.aural"
  ${EndIf}
!macroend
