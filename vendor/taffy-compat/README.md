# Taffy compatibility facade

GPUI 0.2.2 pins the `taffy` package to `=0.9.0`. This small facade keeps that
package identity while re-exporting the upstream `taffy` 0.9.2 API from a
pinned commit. Taffy 0.9.2 changes its optional `grid` dependency to the
fixed 1.x line, resolving GHSA-38c5-483c-4qqp without upgrading GPUI.

Remove this facade when the pinned GPUI release accepts a fixed Taffy version.
