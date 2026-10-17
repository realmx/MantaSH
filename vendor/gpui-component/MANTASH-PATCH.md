# MantaSH component patches

This directory vendors `gpui-component` 0.5.1 from crates.io under its upstream Apache-2.0 license. The upstream repository is https://github.com/longbridge/gpui-component. Versions and dependencies are unchanged.

MantaSH needs explicit formatting to preserve a reversed UTF-8 selection and the scroll offset, and to undo independently from adjacent typing. Upstream exposes neither the selection/scroll setters nor a history checkpoint. The input additions are confined to `src/history.rs`, `src/input/change.rs`, `src/input/state.rs`, and `src/input/state/transaction.rs`:

- `EditSnapshot` and a validated setter capture and restore selection direction and scroll without focusing the input.
- `apply_edit_transaction` rejects disabled, single-line and IME-active inputs, uses the normal replacement/change-event path, clears redo and brackets the replacement with history checkpoints.
- Undo and redo restore the before/after view for this explicit transaction. Ordinary input changes keep the upstream behavior.

The integration is exercised by `scripts/qa_editor_format_macos.py` through an isolated native application and a loopback SFTP fixture. Remove this patch when equivalent supported APIs are available upstream. Keep this note and upstream license with redistributed sources.

`src/button/button.rs` uses the button variant's hover foreground (`hover_style.fg`) instead of a hardcoded red. This keeps neutral and semantic buttons, including dropdown carets, consistent with the application palette. MantaSH's `ui/controls.rs` owns the five action tones and their disabled/selected precedence; no upstream variant or interaction API is removed.
