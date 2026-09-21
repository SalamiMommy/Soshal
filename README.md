# Soshal

A decentralized social media application combining the best of Facebook and MySpace, built on Freenet, Nostr, I2P, and Reticulum Mesh. Features dating, feed, events, messenger, marketplace, and customizable profiles — all running on a peer-to-peer foundation.

## Features & Product Guidelines

Soshal provides complete feature parity with leading social media references ("exact" encompasses every feature of the reference platform plus decentralized and cryptographic superset enhancements). For exhaustive architectural and functional specifications, see [FEATURE_GUIDELINES.md](docs/FEATURE_GUIDELINES.md).

- **Social Feed (Facebook Parity)**:
  - **Rich Multi-Format Publishing**: Plain text with dynamic typography and styled color backgrounds, multi-image collages/albums, high-definition videos with autoplay, searchable animated GIFs, OpenGraph rich link previews, feeling/activity status badges, geohash-indexed location check-ins, and interactive polls.
  - **Reactions & Micro-Tipping**: 6 core Facebook reactions (Like 👍, Love ❤️, Care 🥰, Haha 😆, Wow 😮, Sad 😢, Angry 😡) with animated selection trays and count summaries, plus Lightning Sats zaps (NIP-57) for instant creator tipping.
  - **Nested Threading & Comments**: Hierarchical multi-level comment replies, media attachments, comment reactions, and sorting (Top Comments, Newest, All Comments).
  - **Reshares & Distribution**: Quote reshare with author attribution, instant feed repost (NIP-18), direct share to Messenger, and share to Groups.
  - **Feed Views & Curation**: Toggle between Top/Algorithmic feed (ranking based on engagement, freshness decay, and Web of Trust proximity) and Most Recent/Chronological feed.
  - **Post Controls & Management**: Post revision history editing, pin up to 3 posts to profile timeline, hide post, 30-day snooze for authors/groups, unfollow without unfriending, and save/bookmark to collections.

- **Notifications (with Comprehensive Ignore)**:
  - **Actionable Notification Feed**: Categorized filter tabs (All, Mentions, Reactions, Comments, Friend Requests, Group Activity, Events, Marketplace, Dating).
  - **Granular Ignore Functionality**:
    - **Ignore Notification**: Dismiss an individual notification without triggering read indicators.
    - **Ignore / Mute User**: Silence all future notifications from an individual user across posts, comments, and mentions without unfriending or blocking.
    - **Ignore / Turn Off Thread Notifications**: Unsubscribe from activity on specific posts or comment threads where previously tagged or involved.
    - **Ignore by Category**: Toggle mute switches for specific event types (e.g. zaps, group `@everyone` pings, event invites).
    - **Quiet Mode / Do Not Disturb**: Set scheduled or immediate quiet hours to silence push and in-app alerts.
    - **Ignored Entity Management**: Dedicated dashboard under Settings to review, manage, and unmute ignored users, threads, and categories.

- **Messages (Facebook Messenger Parity)**:
  - **Conversations & Groups**: 1-on-1 direct messaging and multi-participant group chats with custom group names, group avatar images, and admin role management.
  - **Real-Time Presence & State**: "Active Now" online status indicators and last-active timestamps (with privacy toggle), real-time typing indicators (`...`), and delivery/seen indicators featuring recipient avatar badges.
  - **Rich Multimedia**: Voice notes with interactive waveform scrubbing and speed toggles (1x, 1.5x, 2x), high-res photo/video albums, documents, animated stickers, and a searchable Shared Media Gallery tray.
  - **Message Interactions**: Quick emoji reactions on individual bubbles, swipe-to-reply quoting, message forwarding, and pinning vital messages to chat headers.
  - **Security & Ephemeral Modes**: Vanish Mode with configurable self-destruct timers, NIP-44 v2 / Double Ratchet end-to-end encryption with post-quantum ML-KEM-768, and isolated Message Requests inbox with Accept, Delete, and Ignore options.
  - **Calling**: Integrated 1-on-1 and group WebRTC voice and video calls with camera flip, mute, and picture-in-picture.

- **Groups (Discord Parity)**:
  - **Guild / Server Structure**: Sovereign community spaces with custom server icons, banners, vanity invite links, welcome onboarding screens, and mandatory rule-acceptance gates.
  - **Hierarchical Categories & Channels**: Drag-and-drop category organization containing specialized channel types:
    - **Text Channels (`#channel`)**: Markdown formatting, spoiler tags, threaded sub-conversations, pins, and rich file attachments.
    - **Voice Channels (`🔊 voice`)**: Drop-in/drop-out WebRTC voice rooms, active speaker green circles, individual volume sliders, mute/deafen, and screen sharing.
    - **Stage Channels (`📢 stage`)**: Keynote broadcast rooms separating designated stage speakers from audience listeners with a moderated "Raise Hand" queue.
    - **Forum Channels**: Card-based topical discussion boards with tag filtering and search.
  - **Role-Based Access Control (RBAC)**: Multi-tier role hierarchies with customizable color tags, display hoisting, and granular permission matrices (Administrator, Manage Server/Channels/Roles, Kick/Ban, Send Messages, Mention Everyone `@everyone`/`@here`, Voice Connect/Speak/Priority).
  - **Presence & Member Sidebar**: Right-hand member drawer categorized by role hierarchy displaying real-time statuses (Online, Idle, Do Not Disturb, Offline) and activity notes.

- **Dating (Facebook Dating Parity)**:
  - **Segregated Dating Persona**: 100% isolated dating profile detached from the main feed identity (invisible to friends and contacts by default). Up to 9 photos, bio, height, occupation, education, and lifestyle attributes (drinking, smoking, exercise, pets, zodiac, religion, family plans).
  - **Discovery Deck & Contextual Liking**: Vertical scroll-and-swipe profile cards with interactive icebreaker prompt answers. Ability to like and comment directly on a specific photo or prompt to start conversation.
  - **Mutual Match Activation**: Conversations unlock only upon mutual like confirmation.
  - **Secret Crush**: Select up to 9 existing friends or followers; if a crush also adds you to their secret list, an instant match alert triggers. Otherwise, remains completely confidential.
  - **Shared Events & Groups Matching**: Opt-in discovery to match with singles attending the same Events or participating in the same public Groups.
  - **Isolated Dating Inbox**: Strict 1-on-1 messaging quarantined from main Messenger with safety-first media restrictions and immediate unmatch/block/report tools.

- **Marketplace (Facebook Marketplace Parity)**:
  - **Categorized Local Discovery**: Browse Vehicles, Rentals, Electronics, Apparel, Home & Garden, Hobbies, and Free Stuff with keyword auto-suggest and distance radius sliders.
  - **Rich Listings & Seller Transparency**: Multi-image photo carousels, condition tags (New, Like New, Good, Fair), detailed descriptions, generalized location radius bubbles (preserving exact street address privacy), and seller profiles with ratings/reviews.
  - **Seller Dashboard**: Create listings with up to 15 photos, structured attributes, and specified meet-up preferences (Public Meetup, Door Pickup, Door Dropoff); manage Active, Pending, and Sold item states with one-tap status toggles and listing renewal.
  - **In-App Negotiation & Offers**: Direct listing chat with automated prompts ("Is this available?"), formal make-an-offer / counter-offer negotiation engine, and saved item watchlists.

- **Events (Multi-Screen Architecture)**:
  - **Screen 1: Calendar View Screen**:
    - Full-month grid and week views.
    - **Day-Cell Attendance Icons**: Every day cell displays distinct visual icons/badges indicating events scheduled on that day for which the user is attending (with visual distinction between "Attending/Going" vs "Interested", plus event category iconography).
    - Tapping any date cell expands an interactive agenda sheet listing the day's event schedule, venues, and timings.
  - **Screen 2: Audience Discovery Screen**:
    - Dedicated screen to discover and filter events based on **Audience Type**:
      - **Public Events**: Broad network events open to all relays and users.
      - **Friends of Friends Events**: Events hosted or attended by second-degree connections via Web of Trust.
      - **Friends Only Events**: Private gatherings hosted by direct mutual contacts.
    - Quick temporal filters (Today, Tomorrow, This Weekend, Custom Date Range) and proximity radius / virtual event toggles.
  - **Screen 3: Event Detail Screen**: Cover image, host info, date/time with calendar sync, physical address with map directions or virtual stream link, RSVP buttons (Going, Interested, Can't Go), filterable guest list, and discussion wall.
  - **Screen 4: Event Creation Studio**: Audience visibility configuration (Public, Friends of Friends, Friends, Private invite-only), co-hosts, recurrence rules, and ticketing details.

- **Minis (Instagram Reels Parity)**:
  - **Immersive 9:16 Vertical Video**: Edge-to-edge short-form video player with vertical swipe up/down gesture navigation and seamless background pre-buffering.
  - **Action Rail & Interactions**: Heart/Like with counters, slide-up comment tray with nested replies, share/forward to Messenger, remix/duet button, and rotating audio disc.
  - **Creator Overlay**: Creator avatar with instant Follow toggle, expandable multi-line caption, clickable hashtags/mentions, and scrolling audio marquee.
  - **Dedicated Audio / Sound Page**: Tap any audio track to view track details, total Minis created with that audio, a showcase grid, and a "Use Audio" launch button.
  - **Creation Camera Studio**: Multi-segment recording, countdown timer, hands-free recording, speed controls (0.3x to 3x), camera flip, audio overlay picker, video trimming, and text/sticker tools.

- **Live (Twitch Parity)**:
  - **Low-Latency Streaming**: Sub-second video playback via Media over QUIC (MoQ) and WebRTC/RTMP pipelines (`streaming-core`), with multi-quality transcoding selectors (Source/1080p60, 720p60, 480p, 360p, Auto), theater mode, and picture-in-picture.
  - **Interactive Live Chat**: Real-time high-volume chat with user badges (Broadcaster, Mod, VIP, Sub, Verified), custom emotes, emote-only mode, follower/subscriber-only modes, and slow mode.
  - **Stream & Channel Info**: Stream title, category/game directory tagging, live viewer counter, uptime clock, and follow/subscribe tiers.
  - **Moderation View**: Host/mod tools for user timeouts, permanent bans, message deletion, and chat purges.
  - **Community Features**: Lightning zap / "Bits" tipping with on-screen animated cheer alerts, channel raids/hosting upon ending stream, 30–60s clip creator, and archived VODs with full synchronized chat replay.

- **Musicloud (SoundCloud Parity)**:
  - **Interactive Waveform Player**: Visual audio waveform canvas showing track amplitude peaks, with full-width touch scrubbing directly on the waveform, skip, loop, and shuffle.
  - **Timed Waveform Comments**: Listeners can drop comments pinned to exact timestamps along the track, rendering avatar pins on the waveform and animated popup speech bubbles as the playhead passes.
  - **Creator Studio**: Lossless (FLAC, WAV) and compressed (MP3, AAC) audio upload, square album art upload, metadata (title, artist, genre, release date, tags, description), and Public vs Private link privacy.
  - **Stream Feed & Discovery**: Chronological feed of new releases and reposts from followed artists, top charts by genre, and personalized recommendation mixes.
  - **Sets & Playlists**: Create, reorder, and share public/private playlists and track sets.
  - **Social Engagement & Profiles**: Repost tracks to followers' streams, like tracks into personal library, share tracks with timestamp offsets (`?t=01:23`), and artist spotlight profiles with discography tabs.
  - **Persistent Audio Player**: Mini-player bar persisting across app navigation, supported by native OS background audio and lockscreen/notification controls.

- **ChatRandom (Multimodal Chatroulette + 1-on-1 & Groups)**:
  - **Instant Random Matchmaking**: One-tap "Start" pairing with active participants in the matchmaking pool; instant "Next / Skip" button to immediately drop and rotate to a new session.
  - **Multimodal Input Modalities**:
    - **Video Mode**: Live two-way WebRTC camera video + audio with camera flip and mute.
    - **Audio-Only Mode**: Voice chat without video transmission (ideal for low bandwidth or privacy).
    - **Text-Only Mode**: Fast anonymous text chat without requiring camera or microphone.
    - Dynamic capability negotiation between peers.
  - **Match Topologies**:
    - **1-on-1 Matching**: Classic pairwise random connection between two users.
    - **Group Matching**: Dynamic multi-party random lounges where 3 to 8 users are pooled into a shared video/audio/text room, with real-time seat replenishment as members skip or leave.
  - **Interest & Regional Filters**: Topic tag matching (e.g. `#gaming`, `#music`, `#tech`), language filter, and regional preferences.
  - **Safety & Ephemeral Privacy**: Real-time on-device NSFW blur detection, camera blur until confirmed, one-tap report/block with local SQLite blacklisting, and ephemeral cryptographic keys to isolate session identity from main profile.

- **Custom Profiles (MySoshal)** — Drag-and-drop profile builder with modular widgets: text, media gallery, music player, friend grid, contact card, Q&A, and theme selection.
- **Web of Trust & Privacy Controls** — Public, Friends-Only, and Stealth (encrypted to whitelisted pubkeys) modes with friend-of-friend distance calculation.
- **Reticulum Mesh & P2P** — Off-grid peer-to-peer mesh networking with identity discovery (`ANNOUNCE`), 128-bit destination addressing, and packet routing over UDP, Multicast, and RNode serial interfaces.
- **Multi-Account & Biometric Security** — Seamless switching between Nostr identities, protected by Face ID, fingerprint, or PIN lock.
- **I2P & Freenet Support** — Automatic I2P proxy detection, relay routing, and Freenet decentralized storage.


## Tech Stack

| Layer | Technology |
|-------|-----------|
| UI Framework | Flutter (Dart) — the only client |
| App Backend | Rust via `flutter-bridge` FFI adapter (auto-generated `frb_generated.dart` bindings) |
| Core Logic & Domain | Rust Native Workspace (32 `*-core` crates + `flutter-bridge`; 34 workspace members) |
| Database | SQLite (Turso `libsql`) with FTS5 (`db-core`) |
| Nostr & Cryptography | `nostr-sdk`, `ring`, `pqc-core` post-quantum crypto (ML-KEM-768 / ML-DSA-65) |
| Mesh Networking | Native Reticulum Mesh Stack + exotic/mesh transports (`mesh-core`, re-exported via `soshal-network-core::…`) |
| Mobile Support | Android (multi-ABI) + iOS |

## Supported Nostr NIPs

- NIP-01 — Basic protocol
- NIP-02 — Contact List
- NIP-05 — DNS Verification
- NIP-06 — Basic key derivation
- NIP-07 — window.nostr extension API
- NIP-09 — Event Deletion
- NIP-15 — Marketplace (NIP-15 listings)
- NIP-18 — Reposts
- NIP-19 — Bech32 encoding
- NIP-25 — Reactions
- NIP-29 — Group messaging & channels
- NIP-44 — Encrypted DMs
- NIP-52 — Calendar Events
- NIP-57 — Lightning Zaps
- NIP-65 — Relay List Metadata
- NIP-78 — Custom Application Data
- NIP-95/96 — Blossom File Storage (upload server)
- NIP-30080 — Live streaming (video/audio)
- NIP-30081 — Live streaming chat

## Getting Started

### Prerequisites

- Rust (latest stable toolchain)
- Flutter SDK (3.x) with Android toolchain (SDK + NDK 27.1.12297006 for bridge `.so` builds)
- **Linux**: `libmpv.so.2` for media playback (media_kit backend; `builds/linux/build.sh`
  bundles a host copy into the AppImage when present):
  - Debian/Ubuntu: `sudo apt install libmpv2`
  - Fedora: `sudo dnf install mpv-libs`
  - Arch: `sudo pacman -S mpv`

### Optional Dependencies

- **Networking daemons** are bundled into the Android APK at build time
  (`builds/android/build.sh`): i2pd per-ABI, rnsd in-process via Chaquopy
  (`rnspure`). Freenet has no official Android binary — an error stub ships
  instead (no APK binary). See `DAEMON_BUNDLING.md`. Desktop users can run
  their own i2pd/freenet/rnsd; status surfaces in Settings → Network.

### Build & Run Commands

```bash
# Type-check all workspace Rust crates (32 cores + flutter-bridge)
cargo check --workspace

# Run all Rust unit tests
cargo test --workspace

# Flutter app
cd soshal_flutter && flutter pub get && flutter build apk
cd ..

# Build Android bridge .so per ABI (recipe in AGENTS.md), then bundle via
# flutter build apk (jniLibs already wired)
```

## Architecture

```
Soshal/
├── soshal_flutter/         # Flutter UI + services + routes (the only client; 41 providers)
├── flutter-bridge/         # FFI adapter: 55 modules calling into *-core
├── crypto-core/            # ring sha256/hmac/hkdf, NIP-44 & PQC
├── db-core/                # SQLite (libsql) migrations & repositories
├── nostr-core/             # Nostr keys, events, relay engine (nostr-sdk)
├── sync-core/              # Background sync engine, outbox replay, watermark
├── media-core/             # Blossom media client & session management
├── content-core/           # Hashtag/mention/URL parsing & compression
├── identity-core/          # Web of Trust, NIP-05, key & seed management
├── network-core/           # Relay pool, P2P (TCP HMAC + QUIC), battle-tested mesh re-exports
├── mesh-core/              # Exotic/mesh transports: freenet, I2P SAM, Reticulum, BLE, Wi-Fi Direct, PQC link
├── storage-core/           # Audio waveform & cache eviction
├── feed-core/              # Post ranking & feed algorithms
├── groups-core/            # NIP-29 membership & channels
├── messaging-core/         # NIP-44 encrypted messaging
├── social-core/            # Compatibility & social scoring
├── events-core/            # Check-in & NIP-52 calendar logic
├── marketplace-core/       # NIP-15 listing & escrow handling
├── dating-core/            # Compatibility scoring & filtering
├── search-core/            # FTS5 search & event→result mapping
├── notification-core/      # Notification filtering
├── moderation-core/        # Content moderation
├── analytics-core/         # Analytics & audit helpers
├── webrtc-core/            # ICE/WebRTC helpers
├── spatial-core/           # Spatial helpers
├── zap-core/               # LNURL/NWC + BOLT-11 (NIP-47)
├── streaming-core/         # Live video/chat (MoQ groups, WebRTC/RTMP)
├── minis-core/             # Short-form video (kind 31020) + WASM filters
├── audio-core/             # Voice notes, waveform (AAC capture/decode)
├── telemetry-core/         # Local telemetry store
├── layout-core/            # Profile/mesh canvas layout
├── relay-core/             # Local relay node
├── pqc-core/               # Post-quantum crypto (KEM, DSA) & Freenet identity
├── scripts/                # Tooling (check-core-compliance.sh, build-reticulum.sh, trim-ffi-glue.sh)
└── Cargo.toml              # Cargo workspace definition (34 members: 32 cores + bridge + test-util)
```

## Privacy

Soshal is designed with privacy by default:

- **Public** — All content broadcast to relays normally
- **Friends** — Content tagged with friend pubkeys; relays filter by contact list
- **Stealth** — Only visible to explicitly whitelisted pubkeys; uses separate relay sets

Users control their own keys, data, and which relays they connect to.

## License

MIT

