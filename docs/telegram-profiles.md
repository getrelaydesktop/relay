# Profile source mapping

The supported profile surface is a manual Rust/Slint port, not Telegram's Qt
implementation. Reference: Telegram Desktop **v7.4.1**, commit
[`157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19`](https://github.com/telegramdesktop/tdesktop/tree/157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19).
Upstream-derived algorithms, metrics and resources retain their GPL-3.0 license;
see `licenses/NOTICE.md` and `licenses/Telegram-LEGAL.txt`.

## Geometry implemented

Sources below are relative to `Telegram/SourceFiles/` at that exact commit.

| Relay surface | Upstream reference | Logical pixels / behavior |
| --- | --- | --- |
| Profile width | `info/info.style` | desired 392, minimum 324; horizontal layer margin 48 where space allows |
| Flexible header | `info/profile/info_profile_top_bar.cpp`, `info/info.style` | expanded 236, collapsed 56; progress `clamp((height - 124) / 112, 0, 1)` |
| Centered photo | `TopBar::userpicGeometry()` | size `20 + 60p`, top `-20 + 44p`; clips above header when collapsed |
| Name/status | `TopBar::updateTitlePosition()`, `updateStatusPosition()` | name top 8→113, status 28→134; centered when expanded; 16px collapsed left inset |
| Fonts | `infoTopBarTitle`, `infoProfileStatus` | 14px semibold name, 13px regular status; bundled Open Sans |
| Actions | `info_profile_top_bar_action_button.cpp`, `info_profile_top_bar.style` | 52px tall; 18px side padding, 10px gap, 16px bottom padding; 23px icons at y=6, 11px labels at y=30; two-stage collapse |
| Information | `info_profile_actions.cpp`, `info_profile_actions.style` | phone, bio, username, birthday; 23/9/20/7 padding; value above muted label |
| Gift tab | `infoProfileTabsStrip` in `info/info.style` | 48px strip, 36px outer pill, 4px active inset, semibold label |

The centered flexible header is the current upstream profile. The 108px-high,
left-aligned `infoProfileCover` still present in upstream styles is not the
reference for this screen.

## Wired behavior

- Open a sender's profile, the current chat's information or your own profile.
- Basic text arrives before full-info/photo requests. Telegram's privacy rules
  determine which phone, presence and birthday values are available.
- Photo opens the existing zoom/save viewer; closing it returns to the profile.
- Copy displayed information, copy a public profile link, display its QR code.
- Message action opens the resolved peer even without a public username.
- Notification toggle waits for Telegram's `account.updateNotifySettings`
  acknowledgement; errors do not optimistically change the toggle.
- Public gifts arrive as card metadata before thumbnails. Media downloads are
  limited to two concurrent operations; image processing uses the blocking pool.
  Gift failures do not replace the profile's general loading state.
- Settings can hide the entire gifts section. Photo results cannot overwrite
  the gift-loading status. Results for other profile keys are ignored.

## Explicit parity boundaries

This is **not yet full 1:1 profile parity**. In particular:

- Gift card geometry is still a Relay layout, not the upstream `GiftButton`
  minimal renderer. Pagination is 12 per request with a load-more button, not
  upstream's 50 plus virtualized auto-preload.
- Gift previews are static. No TGS/WebM animation, collectible backdrop/pattern,
  gift collections, transfers/upgrades, pinning or reordering.
- No shared media/member/story tabs, premium/verification/emoji-status badges,
  profile color themes, business details, ratings or music pills.
- Calls are not implemented; their profile button is disabled rather than fake.
  Group/channel management and profile editing are not implemented here.
- Only the current photo is shown, not the user's full photo history.
- The layer uses a 40px vertical margin; upstream's complete adaptive outer-layer
  placement algorithm has not been ported.

## Verification

`src/visual_tests.rs::verify_profiles` renders the actual `Desktop` component,
not a stand-alone mock. It checks header expansion/collapse and the photo's
centered position at 100%, 150%, 200% and 400px window width, and saves dark,
light, collapsed, self-profile and gifts-hidden PNGs. These checks protect the
ported geometry; they are not an upstream screenshot-diff proof of 1:1 parity.
Live Telegram permissions, native dialogs, and DWM/GPU frame pacing require
interactive verification separately.
