# Telegram Desktop animation port

Reference: [Telegram Desktop v7.4.1](https://github.com/telegramdesktop/tdesktop/tree/157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19),
with [desktop-app/lib_ui](https://github.com/desktop-app/lib_ui/tree/42c6f1e88db81e6d9e8006f3e7b1c3ba39f9cac4)
at its pinned submodule revision. This port covers interactions implemented in
Relay. It does not add Telegram's missing screens or animated media assets.

| Interaction | Upstream behavior | Source |
| --- | --- | --- |
| Main menu | 200 ms, `easeOutCirc`, both directions | lib_ui `ui/layers/layer_widget.cpp`, `ui/layers/layers.style` |
| Settings and modal content | Linear opacity, 200 ms | lib_ui `ui/layers/layer_widget.cpp` |
| Shared background dim | 200 ms; `easeOutCirc` opening, linear closing; retained when changing layers | lib_ui `ui/layers/layer_widget.cpp` |
| Settings pages and compact chat navigation | 240 ms, `easeOutCirc`; underlying page shifts 100 logical px; incoming page travels its full width | tdesktop `Telegram/SourceFiles/window/window_slide_animation.cpp`, lib_ui `ui/basic.style` |
| Docked emoji section | Section transition, 240 ms, `easeOutCirc` | tdesktop `Telegram/SourceFiles/chat_helpers/tabbed_section.cpp`, `window/section_widget.cpp` |
| Popup menu reveal | 200 ms; width starts at 50%, height at 30%; circular width/height/alpha curves complete at 60%/90%/30% of the timeline; moving 20% content fade band | lib_ui `ui/effects/panel_animation.cpp`, `ui/widgets/widgets.style` |
| Popup dismissal | Linear opacity, 150 ms; an unfinished opening continues while fading; reopening reverses opacity | lib_ui `ui/widgets/popup_menu.cpp`, `ui/widgets/widgets.style` |
| Intro steps | Linear 200 ms timeline; incoming position/outgoing opacity use `easeOutCirc`, outgoing position/incoming opacity use `easeInCirc` | lib_ui `ui/effects/slide_animation.cpp`, tdesktop `Telegram/SourceFiles/intro/intro_widget.cpp`, `intro/intro.style` |
| Night mode toggle | Linear position/color, 120 ms | lib_ui `ui/widgets/checkbox.cpp`, `ui/basic.style` |
| Press ripples | Pointer origin, furthest-corner radius, 650 ms `easeOutQuint` expansion, linear 200 ms release fade; overlapping presses preserved | lib_ui `ui/effects/ripple_animation.cpp`, `ui/widgets/widgets.style` |
| Auth input stroke | Linear color transition, 150 ms | lib_ui `ui/widgets/widgets.style` |

## Timing and rendering

`ui/motion.slint` uses the original equations from lib_ui
[`ui/effects/animation_value.cpp`](https://github.com/desktop-app/lib_ui/blob/42c6f1e88db81e6d9e8006f3e7b1c3ba39f9cac4/ui/effects/animation_value.cpp):

- `easeOutCirc(t) = sqrt(1 - (t - 1)^2)`
- `easeInCirc(t) = 1 - sqrt(1 - t^2)`
- `easeOutQuint(t) = 1 + (t - 1)^5`
- `sineInOut(t) = (1 - cos(pi * t)) / 2`

The frame clock is a native Slint animation, driven by the rendering event loop.
It replaces the application's fixed 16 ms menu timer. Intermediate values are
computed from elapsed time, without a cubic-bezier approximation. Restarting a
`Motion` samples its current value and uses a full duration to reach the new
target, matching lib_ui `Ui::Animations::Simple::startPrepared`.

Contents remain vector/text items rendered at the display scale by Skia. Popup
opening crops full-size content instead of scaling a low-resolution snapshot.
Ripple masks use native rounded clipping. Dim, slide and ripple colors use the
bundled Telegram night palette and lib_ui's light palette.

This is a behavioral port from Qt to Slint, not a binary reuse of Qt widgets.
Text metrics, dynamic layouts and shadows can still differ between renderers.
In particular, the docked emoji section relayouts the live conversation as it
opens; Telegram can animate cached section snapshots. Source timings and curves
alone do not guarantee identical pixels for every screen.

## Verification

`src/visual_tests.rs` drives a deterministic clock through the actual Skia
renderer. It checks mid-animation golden values, reversing the menu while it is
moving, reopening it, the shared menu-to-settings dim, settings pages, compact
navigation, intro timing, toggle timing, popup geometry/alpha/reopening and
ripple origin/clipping/cleanup.
The Windows debug CI job also exports intermediate animation PNGs alongside the
existing static previews and icon atlas at 100–300% display scaling.
