; Custom NSIS installer hooks for Deskemy.
;
; Deskemy bundles libmpv-2.dll (mpv's shared library) next to the executable, so
; video playback works out of the box with no separate mpv install. WebView2 is
; checked and installed by Tauri's own bootstrapper. There are therefore no
; runtime dependencies left for us to verify here; the hooks are kept as
; extension points in case that changes.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
