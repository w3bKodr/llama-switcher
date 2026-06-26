; Custom NSIS installer hooks for Llama Switcher.
; Wired in via tauri.conf.json > bundle.windows.nsis.installerHooks.
;
; POSTINSTALL:
;   1. Optionally add an autostart shortcut to the user's Startup folder
;      (equivalent to dropping the exe in shell:startup).
;   2. Optionally install the bundled Hermes Agent skill into an auto-detected
;      Hermes skills folder. The API token is written later by the app's
;      Agent Control page because it is generated on first launch.

!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; ---- Desktop shortcut icon refresh ----
  ; Recreate the desktop shortcut with an explicit installed icon file so
  ; Windows does not keep showing a stale cached icon from an older build.
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\desktop-icon.ico" 0

  ; ---- Autostart (shell:startup) ----
  MessageBox MB_YESNO|MB_ICONQUESTION "Start Llama Switcher automatically when you sign in?" IDNO lbl_skip_autostart
    Delete "$SMSTARTUP\${PRODUCTNAME}.lnk"
    CreateShortcut "$SMSTARTUP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\desktop-icon.ico" 0
  lbl_skip_autostart:

  ; ---- Optional Hermes skill install ----
  MessageBox MB_YESNO|MB_ICONQUESTION "Install the Hermes Agent skill now?$\r$\n$\r$\n(You can also do this later from the app's Agent Control page, which also writes your API token automatically.)" IDNO lbl_skip_hermes

  StrCpy $R0 ""

  ; HERMES_SKILLS_DIR
  ReadEnvStr $0 "HERMES_SKILLS_DIR"
  ${If} $0 != ""
    StrCpy $R0 "$0"
  ${EndIf}

  ; HERMES_HOME\skills
  ${If} $R0 == ""
    ReadEnvStr $0 "HERMES_HOME"
    ${If} $0 != ""
      StrCpy $R0 "$0\skills"
    ${EndIf}
  ${EndIf}

  ; Official Hermes default: ~/.hermes/skills. The skills directory may not
  ; exist yet, so detect the Hermes home and create it below.
  ${If} $R0 == ""
  ${AndIf} ${FileExists} "$PROFILE\.hermes\*.*"
    StrCpy $R0 "$PROFILE\.hermes\skills"
  ${EndIf}

  ${If} $R0 == ""
    MessageBox MB_OK|MB_ICONINFORMATION "Could not auto-detect a Hermes skills folder.$\r$\n$\r$\nAfter launching Llama Switcher, open Agent Control and click 'Install skill to Hermes' to browse to it."
  ${Else}
    CreateDirectory "$R0\llama-switcher"
    CreateDirectory "$R0\llama-switcher\scripts"
    CopyFiles /SILENT "$INSTDIR\hermes-skill\SKILL.md" "$R0\llama-switcher"
    CopyFiles /SILENT "$INSTDIR\hermes-skill\scripts\llama_switcher.py" "$R0\llama-switcher\scripts"
    MessageBox MB_OK|MB_ICONINFORMATION "Hermes skill installed to:$\r$\n$R0\llama-switcher$\r$\n$\r$\nOpen Llama Switcher > Agent Control and click 'Install / update Hermes skill' once to configure the API token. Start a new Hermes session (or use /reset) to see the skill."
  ${EndIf}
  lbl_skip_hermes:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the autostart shortcut if present.
  Delete "$SMSTARTUP\${PRODUCTNAME}.lnk"
!macroend
