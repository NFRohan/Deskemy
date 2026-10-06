> **Deskemy's patched copy** of `i-slint-renderer-skia` 1.18.1, applied with
> `[patch.crates-io]` in `app/Cargo.toml`. The only change: on Windows, text
> is drawn with subpixel (ClearType-style) antialiasing — see
> `subpixel_text()` in `lib.rs` and its uses in `opengl_surface.rs`,
> `software_surface.rs` and `itemrenderer.rs` — paused at runtime with
`set_subpixel_text_paused` (the app does over video). Re-apply when upgrading Slint
> (the patch must match the `slint` version exactly), or drop it once Slint
> supports subpixel text itself.


**NOTE**: This library is an **internal** crate of the [Slint project](https://slint.dev).
This crate should **not be used directly** by applications using Slint.
You should use the `slint` crate instead.

**WARNING**: This crate does not follow the semver convention for versioning and can
only be used with `version = "=x.y.z"` in Cargo.toml.
