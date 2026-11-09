# Imported Telegram Desktop resources

The PNG interface icons and interface metrics were taken from Telegram Desktop
v7.4.1, commit 157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19.

Source: https://github.com/telegramdesktop/tdesktop/tree/v7.4.1

The original icon files are in Telegram/Resources/icons. The back, close and
window caption icons come from Telegram/lib_ui/icons at submodule commit
42c6f1e88db81e6d9e8006f3e7b1c3ba39f9cac4.
The original grayscale icon masks were converted to RGBA by using the mask
intensity as the alpha channel. Glyph dimensions, padding and antialiasing
were preserved. The icons are tinted by Slint at runtime.

The native paths in ui/icons/vector-icons.slint are derived from those alpha
masks with tools/vectorize_icons.py. The paths retain their logical dimensions
and padding and are filled directly by Skia at the current display scale.
They are reconstructed contours, rather than original SVG artwork. The source
PNG masks remain available for comparison and regeneration.

Copyright (c) 2014-2026 The Telegram Desktop Authors.
See Telegram-LEGAL.txt and Telegram-GPL-3.0.txt.

The bundled Open Sans Regular and SemiBold fonts come from Telegram/lib_ui/fonts.
Digitized data copyright 2010-2011, Google Corporation.
Licensed under Apache License 2.0. See OpenSans-Apache-2.0.txt.

The Slint interface is a manual port. It does not contain Telegram's Qt widget
implementation. Appearance metrics are based on dialogs/dialogs.style,
ui/chat/chat.style, window/window_main_menu.style, lib_ui/ui/basic.style and the
bundled night.tdesktop-theme color palette.

Animation equations, durations and transition behavior in ui/motion.slint and
the associated UI components are ported from the same Telegram Desktop version
and desktop-app/lib_ui submodule. See docs/telegram-animations.md for pinned
source links, covered interactions and verification. These algorithms retain
their upstream GPL-3.0 license; see Telegram-GPL-3.0.txt and Telegram-LEGAL.txt.
