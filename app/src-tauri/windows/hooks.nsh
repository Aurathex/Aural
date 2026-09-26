; Aural installer hooks (Tauri NSIS). Uninstalling always removes the
; "start with Windows" entry; when the user ticks "Delete the application data" it
; also removes downloaded models, logs and settings, which live in Aural's own
; folders rather than the identifier-named ones Tauri knows about.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Aural"
  ${If} $DeleteAppDataCheckboxState = 1
    RMDir /r "$LOCALAPPDATA\Aural"
    RMDir /r "$APPDATA\Aural"
  ${EndIf}
!macroend
