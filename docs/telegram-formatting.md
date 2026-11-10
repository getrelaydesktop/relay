# Telegram Windows formatting and composer

Reference: Telegram Desktop v7.4.1, commit
`157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19`, and its lib_ui revision
`42c6f1e88db81e6d9e8006f3e7b1c3ba39f9cac4` (the same reference as the
existing icon, palette and animation ports).

The dialog date now uses `dialogsDateFont` (13px), unread counters use
`dialogsUnreadHeight` (19px) and 5px horizontal padding, and dialog titles
reserve the actual date and receipt width instead of a fixed text column.
Existing 62px dialog rows, 46px avatars, 430px message width and 11/8px message
padding remain based on upstream `dialogs.style` and `chat.style`.
Reply text uses the 13px message font and a 2px by 36px quote bar.

Message entities survive history, updates and send acknowledgements. Telegram's
UTF-16 offsets are mapped to UTF-8 byte spans without interpreting the message
as Markdown or HTML. Bold, italic, underline, strike, inline/preformatted
monospace and links use native Slint styled spans; nested ranges, Unicode,
explicit line breaks and literal markup are retained. Media-label prefixes are
accounted for separately. Italic uses the bundled upstream font.

Links use native hit testing. Because Slint 1.16.1's StyledText consumes right
clicks, the Winit window event filter selects the context-menu surface before
dispatching a right click. Photo and reply hit areas stay independent. URL
opening passes an approved scheme directly to the Windows shell API without
constructing a shell command.

The composer wraps and grows from 46px up to 166px, with scrolling for longer
drafts. Enter sends; Shift+Enter inserts a newline. History height and the
sending animation's clipping band follow the actual composer height. Existing
draft text, IME, cursor, selection and undo use the native TextInput.
Attachment, emoji and send buttons stay aligned with the bottom of the field.

## Verification

`src/rich_text.rs` covers UTF-16 surrogate boundaries, nested entities, literal
markup, invalid ranges and media-prefix offsets. The existing Skia visual suite
adds formatted messages in light/dark themes, at 150% scale and in a 430px
window, link activation, context menus, multiline input and bounded long drafts.
The existing `Build debug and release` workflow runs these checks and publishes
the corresponding `animation-formatted-*.png` and
`animation-multiline-composer.png` artifacts.

This patch does not establish pixel identity with Telegram. In particular,
spoiler effects, custom emoji, dedicated blockquote/code-block containers,
rich-text editing shortcuts and Telegram's other missing screens still need
separate implementation and comparison. The reference is pinned rather than
an assertion about the latest Telegram release.
