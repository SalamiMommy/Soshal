# Soshal Flutter

Native Flutter client for Soshal, backed by the Rust workspace through a
direct `flutter_rust_bridge` FFI bridge. This is the only client UI.

## Architecture

- **Rust backend**: 27 `*-core` crates, pure logic, no platform deps
- **FFI adapter**: `flutter-bridge` (30 modules) delegates to cores; generated
  bindings in `lib/frb_generated.dart`
- **Flutter frontend**: Material 3 UI, go_router navigation, Provider
  (ChangeNotifier services); screens never call the bridge directly — only
  services do
- **Key material**: nsec enters only at signer init; OS keychain ops exchange
  pubkeys, never key bytes

## Project Structure

```
soshal_flutter/
├── lib/
│   ├── main.dart           # App root, provider registration
│   ├── frb_generated.dart    # Generated FFI bindings (do not hand-edit)
│   ├── services/             # 16 ChangeNotifier services (auth, feed, session, …)
│   ├── screens/              # ~24 screens
│   ├── routes/               # go_router configuration (~30 routes)
│   └── widgets/              # Shared widgets
├── android/app/src/main/jniLibs/  # libsoshal_flutter_bridge.so (3 ABIs)
└── pubspec.yaml
```

## Development

### Prerequisites

- Flutter SDK 3.47+ (fvm: `export PATH="$HOME/fvm/default/bin:$PATH"`)
- Rust toolchain + NDK 27.1.12297006 for Android
- flutter_rust_bridge_codegen 2.12.0 (only when FFI signatures change)

### Building

```bash
./builds/android/build.sh            # 3-ABI bridge + debug APK (add --release)
./builds/linux/build.sh              # host bridge + Linux bundle (add --release)
flutter analyze                      # lint gate: 0 errors / 0 warnings
```

## Status

- FFI surface fully wired: every real Rust function has a UI path; no callable
  stale codegen artifacts (network relay/publish fns exist only as leftover
  wire stubs in `frb_generated.rs` — never call them).
- Backend-gated surfaces (stubs, honest UI): NWC zap invoice fetch, push
  notifications, friend requests/suggestions, minis, WebRTC voice/video.
- Legacy Tauri 2.0 + Dioxus WASM client deleted (2026-08); Flutter is the only
  client.

## UI Feature Guidelines & Reference Specifications

The Flutter client provides the UI/UX implementation of the 11 product domains defined in `../docs/FEATURE_GUIDELINES.md`. All screens and ChangeNotifier services must adhere to the reference platform standards:

- **Feed (`feed_screen.dart`, `composer_screen.dart`, `thread_screen.dart`)**: Complete **Facebook** parity — rich post composition (typography, styled backgrounds, multi-image collages, video autoplay, GIFs, OpenGraph links, polls), 6 animated reactions + Sats zaps, nested multi-level comment threading, reshares with commentary, per-post audience selector (Public, Friends, Friends of Friends, Custom/Stealth), and Top vs Most Recent toggle.
- **Notifications (`notifications_screen.dart`)**: Actionable categorized notification feed with full **Ignore** functionality — one-tap dismissal, ignore/mute specific users, turn off post/thread notifications, mute categories, quiet hours / Do Not Disturb, and an ignored entity management dashboard.
- **Messages (`inbox_screen.dart`, `thread_screen.dart`)**: Complete **Facebook Messenger** parity — 1-on-1 and group chats with admin roles, presence and real-time typing indicators (`...`), delivery/seen avatars, voice notes with interactive waveform scrubbing (1x/1.5x/2x), media gallery tray, in-chat emoji reactions, swipe-to-reply quoting, vanish mode, WebRTC voice/video calling, and isolated Message Requests.
- **Groups (`groups_screen.dart`, `group_tabs.dart`)**: Complete **Discord** parity — guild/server hierarchy, collapsible channel categories, text channels with markdown/spoiler tags/pins/threads, persistent WebRTC voice channels with speaker halos and volume sliders, stage channels with a moderated "Raise Hand" queue, forum channels, multi-tier RBAC roles with color tags and permissions, `@` mentions, and presence sidebar.
- **Dating (`dating_screen.dart`, `dating_profile_screen.dart`)**: Complete **Facebook Dating** parity — 100% segregated dating profile invisible to friends, swipe/scroll discovery deck, contextual likes on specific photos or prompts, mutual match activation, **Secret Crush** (up to 9 friends), shared Events/Groups matching, and an isolated dating inbox.
- **Marketplace (`marketplace_screen.dart`)**: Complete **Facebook Marketplace** parity — category hierarchy, radius search slider, condition tags (New, Like New, Good, Fair), multi-photo carousels, approximate location privacy circles, seller dashboard (Active/Pending/Sold), and in-app negotiation with automated offer prompts.
- **Events (`events_screen.dart`)**: Multi-screen event management architecture:
  - **Screen 1: Calendar View**: Month/week grid displaying distinct visual icons in each day cell for events the user is attending on that day (differentiating Attending/Going vs Interested), plus date tap agenda sheet.
  - **Screen 2: Audience Discovery Screen**: Dedicated screen to find events segmented strictly by **Audience Type** (Public, Friends of Friends, Friends Only), with temporal (Today/Weekend) and proximity filters.
  - **Screens 3 & 4**: Comprehensive Event Detail (cover, RSVP, attendee list, discussion wall) and Event Creation studio.
- **Minis (`minis_screen.dart`)**: Complete **Instagram Reels** parity — 9:16 vertical video player with swipe up/down navigation and background pre-buffering, right action rail (Like, Comment sheet, Share, Remix, Audio disc), creator overlay with Follow toggle and audio marquee, dedicated Sound/Audio page ("Use Audio"), and multi-segment creation camera with speed/timer controls.
- **Live (`live_broadcast_screen.dart`, `moq_viewer_screen.dart`)**: Complete **Twitch** parity — sub-second low-latency streaming via MoQ/WebRTC, multi-quality resolution selector, real-time live chat with badges (Broadcaster, Mod, VIP, Sub) and emotes, mod view tools (timeouts, bans, purges), channel raids/hosts, zap/bits tipping alerts, and VOD archive with synchronized chat replay.
- **Musicloud (`music_screen.dart`)**: Complete **SoundCloud** parity — interactive visual waveform scrubber with touch seeking, timed waveform comments pinned at exact timestamps along the track, creator upload studio (FLAC, WAV, MP3) with square artwork, chronological stream feed of followed artists, sets/playlists, reposts/likes, and persistent background playback with lockscreen controls.
- **ChatRandom (`chatrandom_service.dart`)**: **Chatroulette** parity supporting multimodal inputs and multiple room topologies:
  - **Input Modalities**: Video mode (two-way WebRTC), Audio-only mode, and Text-only mode with dynamic negotiation.
  - **Topologies**: 1-on-1 random pairing and Group random lounges (3 to 8 users pooled together with auto-replenishment).
  - Instant Next/Skip controls, interest tag matching (`#topics`), and real-time automated NSFW blur detection.