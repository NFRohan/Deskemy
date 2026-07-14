; Custom NSIS installer hooks for Deskemy.
;
; WebView2 is already checked and installed by Tauri's own bootstrapper, so the
; only runtime dependency we need to flag ourselves is mpv: Deskemy plays video
; through the user's installed mpv (libmpv) rather than bundling a media engine.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Look for mpv on the DLL/exe search path (covers PATH and Scoop shims).
  SearchPath $0 "libmpv-2.dll"
  StrCmp $0 "" 0 deskemy_mpv_ok
  SearchPath $0 "mpv.exe"
  StrCmp $0 "" 0 deskemy_mpv_ok
    MessageBox MB_YESNO|MB_ICONINFORMATION "Deskemy plays video through mpv, which does not appear to be installed yet.$\n$\nInstall mpv from mpv.io (or run 'scoop install mpv'), then start Deskemy. The app will also prompt you if it can't find mpv.$\n$\nOpen the mpv download page now?" IDNO deskemy_mpv_ok
    ExecShell "open" "https://mpv.io/installation/"
  deskemy_mpv_ok:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
