# Soshal Feature Guidelines & Specifications

This document defines the canonical feature requirements and architectural specifications for the core product domains within **Soshal**. Each domain is specified to provide full parity with its reference platform ("exact" includes all reference features) while seamlessly incorporating Soshal's decentralized, peer-to-peer (P2P), cryptographic, and Web of Trust (WoT) extensions.

---

## Table of Contents

1. [Social Feed (Facebook Parity + Decentralized Enhancements)](#1-social-feed)
2. [Notifications (Comprehensive Ignore & Filtering)](#2-notifications)
3. [Messages (Facebook Messenger Parity + E2EE)](#3-messages)
4. [Groups (Discord Parity)](#4-groups)
5. [Dating (Facebook Dating Parity)](#5-dating)
6. [Marketplace (Facebook Marketplace Parity)](#6-marketplace)
7. [Events (Multi-Screen Architecture: Calendar Day-Cell Icons & Audience Filter)](#7-events)
8. [Minis (Instagram Reels Parity)](#8-minis)
9. [Live (Twitch Parity)](#9-live)
10. [Musicloud (SoundCloud Parity)](#10-musicloud)
11. [ChatRandom (Multimodal Chatroulette + 1-on-1 & Group Matching)](#11-chatrandom)
12. [Architectural Mapping Matrix](#12-architectural-mapping-matrix)

---

## 1. Social Feed

The Soshal feed functions with feature parity to **Facebook**, offering rich multimedia posting, versatile social interactions, algorithmic and chronological curation, robust privacy controls, and granular post-management actions.

### 1.1 Content Creation & Post Formats
- **Multi-Format Publishing**:
  - Plain text posts with dynamic font sizing (larger typography for short status updates) and customizable background color gradients.
  - Multi-image albums and adaptive collages with drag-and-drop reordering, captioning per image, and full-resolution lightbox inspection.
  - High-definition video uploads with automatic thumbnail generation and inline autoplay.
  - Animated GIF integration via searchable repositories and local uploads.
  - Rich link attachments featuring OpenGraph scraping (title, description, domain, and hero preview image).
  - "Feelings & Activities" metadata picker (e.g., "feeling excited", "celebrating a milestone", "listening to...").
  - Geolocation check-ins with venue tagging (geohash indexed).
  - Native polling widgets with single or multi-choice options, real-time vote counters, and expiration timers.
- **Audience & Privacy Selector**:
  - Every post includes an explicit audience selector set at composition:
    - **Public**: Broadcast across relays and discoverable globally.
    - **Friends Only**: Encrypted or restricted to mutual followers / contact lists.
    - **Friends of Friends**: Extended Web of Trust radius (2 hops).
    - **Custom / Stealth**: Whitelist of specific recipient pubkeys using NIP-44 v2 encryption.

### 1.2 Reactions & Social Feedback
- **6 Core Facebook Reactions**:
  - **Like** (👍), **Love** (❤️), **Care** (🥰), **Haha** (😆), **Wow** (😮), **Sad** (😢), **Angry** (😡).
  - Hover or long-press reveals the animated reaction selector with counter summaries.
- **Lightning Zaps & Sats**:
  - Superset feature allowing instant micro-tipping (NIP-57) attached directly to the post, displaying zap amounts alongside reaction badges.

### 1.3 Nested Threading & Comments
- **Hierarchical Discussions**:
  - Multi-level nested replies (parent comment → direct replies → thread continuance).
  - Rich replies supporting text, @mentions, photo attachments, GIFs, and reactions per comment.
  - Sorting options: **Top Comments** (engagement-ranked), **Newest First**, and **All Comments**.

### 1.4 Resharing & Distribution
- **Reshare with Thoughts (Quote Post)**:
  - Repost original content embedded within a new commentary post, preserving original author attribution and verification badges.
- **Instant Share**:
  - Repost directly to user feed (NIP-18).
  - Share externally via link, share internally to direct message threads (Messenger), or share into specific Groups.

### 1.5 Curation, Feeds & Management
- **Feed Views**:
  - **Top / Algorithmic Feed**: Ranks content using engagement metrics, freshness decay, and Web of Trust proximity scores (`feed-core`).
  - **Most Recent / Chronological Feed**: Strict reverse-chronological delivery without algorithmic reordering.
  - **Favorites Feed**: Filtered strictly to prioritized contacts.
- **Post Controls Menu**:
  - **Edit Post**: Edit text content while maintaining a public revision history log.
  - **Pin to Profile**: Pin up to 3 posts to the top of the user's profile timeline.
  - **Hide Post**: Remove item from the current user's feed view.
  - **Snooze**: Temporarily hide posts from an author or group for 30 days without unfriending.
  - **Unfollow**: Cease seeing posts from a user while preserving mutual contact/friend status.
  - **Save Post**: Add to bookmarked collections for later reference.

---

## 2. Notifications

Soshal provides an actionable, organized notification center with dedicated controls to **ignore** content, users, and threads, ensuring users maintain total autonomy over interruptions.

### 2.1 Notification Feeds & Categories
- Segmented inbox tabs: **All**, **Mentions**, **Reactions**, **Comments**, **Friend Requests**, **Group Activity**, **Events**, **Marketplace**, and **Dating**.
- Interactive notification cards with deep links to relevant threads, profiles, or listings.

### 2.2 Comprehensive "Ignore" System
- **Ignore Individual Notification**:
  - Dismiss an individual notification card from the feed without triggering a "read" receipt or alert to the sender.
- **Ignore / Mute User**:
  - Silence all future notifications from a specific user across all surfaces (posts, comments, tags) without unfriending or blocking them.
- **Ignore / Turn Off Notifications for Post/Thread**:
  - Disable further notifications on a specific post or comment thread where the user was previously tagged or participated.
- **Ignore by Notification Category**:
  - Toggle granular mute switches for specific event types (e.g., mute zap alerts, mute group-wide `@everyone` pings, mute event invites).
- **Quiet Mode / Do Not Disturb**:
  - Scheduled or ad-hoc quiet hours that suppress pop-up alerts and badges while queuing items silently in the background.
- **Ignored List Management**:
  - Centralized settings interface (`Settings → Notifications → Ignored`) displaying all currently muted users, threads, and categories, with one-tap unignore/restore options.

---

## 3. Messages

The messaging experience delivers full **Facebook Messenger** capabilities, enhanced with peer-to-peer privacy, post-quantum security, and end-to-end encryption.

### 3.1 Direct & Group Conversations
- **1-on-1 Direct Chats**: Instant encrypted conversation channels between contacts.
- **Group Chats**:
  - Multi-user conversations with customizable group names, custom group avatar photos, and member lists.
  - Role controls: Group creators/admins can add or remove members, appoint co-admins, or modify group permissions.
- **Active Status & Presence**:
  - Real-time online indicators ("Active Now") and last-seen timestamps (with user opt-out privacy switches).
  - Live typing indicators ("...") with debounce synchronization across peers.

### 3.2 Rich Media & Interactions
- **Voice Notes**:
  - Audio recording with dynamic waveform visualization.
  - Interactive playback scrubber, 1.5x/2x speed multipliers, and draft preview before sending.
- **Media & File Attachments**:
  - Full-resolution photo albums, video attachments, documents (PDF, DOCX, ZIP), voice memos, and animated stickers/GIFs.
  - Shared Media Tray: Gallery tab inside chat details indexing all photos, videos, links, and files exchanged in the conversation.
- **Message Reactions & Quoting**:
  - Quick emoji reaction picker on any message.
  - Swipe-to-reply quoting the specific target message.
  - Message forwarding to other contacts or groups.
- **Read Receipts**:
  - Delivery indicators: Sent (check), Delivered (filled check), and Seen (miniature avatar of the reader).

### 3.3 Ephemeral & Security Controls
- **Vanish Mode / Disappearing Messages**:
  - Configurable self-destruct timers (5s, 1m, 1h, 24h) after which messages are purged from local storage and relays.
- **End-to-End Encryption**:
  - Standardized on NIP-44 v2 / Double Ratchet with PQC (ML-KEM-768) key exchange.
- **Message Requests & Spam Filtering**:
  - Messages from unknown users or non-contacts are isolated in a "Message Requests" inbox.
  - Options to **Accept**, **Delete**, or **Ignore / Block** without notifying the sender that the message was read.

### 3.4 Voice & Video Calling
- Native 1-on-1 and group WebRTC voice and video calls with camera flip, mute, speakerphone toggle, and picture-in-picture floating windows.

---

## 4. Groups

Groups in Soshal deliver the full community, channel, and permission architecture of **Discord**, organized into sovereign servers/spaces.

### 4.1 Guild / Space Hierarchy
- **Server Architecture**:
  - Custom server icon, banner art, title, description, and vanity invite links.
  - Server-level welcome/onboarding screens and mandatory community rules acceptance before granting chatting privileges.
- **Categorized Channels**:
  - Hierarchical, collapsible channel categories (e.g., "Information", "Text Channels", "Voice Lounges", "Development").
  - Drag-and-drop category and channel reordering for administrators.

### 4.2 Channel Modalities
- **Text Channels (`#channel-name`)**:
  - Markdown formatting (bold, italics, code blocks, blockquotes, spoiler tags `||spoiler||`).
  - Thread creation branching off messages for side-topic discussions.
  - Pinned messages tray for important announcements.
- **Voice Channels (`🔊 voice-room`)**:
  - Persistent drop-in / drop-out voice rooms powered by decentralized WebRTC mesh / MoQ transport.
  - Green speaker halo indicators around active speakers, individual volume sliders per participant, mute/deafen hotkeys, and screen sharing.
- **Stage Channels (`📢 stage-event`)**:
  - Structured event spaces separating designated stage speakers from audience listeners, with a "Raise Hand" queue managed by stage moderators.
- **Forum Channels**:
  - Dedicated forum layout where every post is an independent discussion card with tags, search, and activity tracking.

### 4.3 Role-Based Access Control (RBAC) & Permissions
- **Hierarchical Roles**:
  - Custom roles with color swatches, distinct role badges, and display hoisting (separating members by role in the sidebar).
  - Fine-grained permission flags:
    - *Server Level*: Administrator, Manage Server, Manage Roles, Manage Channels, Kick Members, Ban Members, View Audit Logs.
    - *Text Level*: Send Messages, Manage Messages, Embed Links, Attach Files, Mention Everyone (`@everyone` / `@here`), Add Reactions.
    - *Voice Level*: Connect, Speak, Video, Priority Speaker, Mute Members, Deafen Members, Move Members.
- **Mentions & Notification Badges**:
  - Granular mention tagging: `@username`, `@role`, `@everyone`, and `@here` with visual unread pill counters.
- **Member List & Presence Sidebar**:
  - Right-hand collapsible member tray grouped by role hierarchy, displaying real-time avatar presence (Online, Idle, Do Not Disturb, Offline) and custom status messages.

---

## 5. Dating

Soshal Dating delivers full **Facebook Dating** parity, designed as a distinct, opt-in persona strictly segregated from the main social profile to safeguard user privacy.

### 5.1 Isolated Dating Persona
- **Separate Dating Profile**:
  - Completely detached from the main feed identity: friends cannot see a user's dating profile or activity unless a mutual match occurs.
  - Dedicated photo gallery (up to 9 photos), bio, height, hometown, occupation, education, and lifestyle tags (drinking, smoking, exercise, pets, zodiac, religion, children/family plans).
  - Icebreaker Prompt Cards: Users answer structured prompts (e.g., "The most spontaneous thing I've done is...", "My ideal Sunday...") displayed attractively between photos.

### 5.2 Discovery & Matching Mechanics
- **Discovery Deck**:
  - Clean card stack view allowing vertical scrolling through a candidate's complete profile, photos, and prompt responses.
  - **Pass (✕)** and **Like (❤️)** actions.
- **Contextual In-Line Liking**:
  - Users can like and send a direct comment attached to a *specific photo* or *specific prompt answer*, initiating high-context conversations.
- **Mutual Match Activation**:
  - When both users express mutual interest, a match is confirmed and unlocks a dedicated 1-on-1 dating conversation.
- **Secret Crush**:
  - Users can select up to **9 existing Soshal friends or followers** to add to their private "Secret Crush" list.
  - If a selected crush also adds the user to their Secret Crush list, an instant match alert is generated.
  - Unless reciprocal, the crush selection remains 100% secret, private, and unrevealed.

### 5.3 Shared Events & Groups Matching
- **Commonalities Matching**:
  - Opt-in toggles allowing singles to discover other candidates who are attending the same Events or members of the same public Groups, providing natural shared interests.

### 5.4 Privacy & Dedicated Dating Inbox
- **Safety by Default**:
  - Friends and family are hidden from ordinary dating discovery by default.
  - Option to also exclude "Friends of Friends".
- **Dedicated Dating Chat**:
  - Dating conversations reside in an isolated dating inbox completely separate from main Messenger.
  - Initial communications restrict video/photo links until mutual trust is established.
  - Instant one-tap "Unmatch", "Block", and "Report" tools.

---

## 6. Marketplace

Soshal Marketplace provides full **Facebook Marketplace** functionality, enabling localized peer-to-peer buying and selling with rich listings, integrated negotiation, and buyer/seller trust metrics.

### 6.1 Browsing & Discovery
- **Structured Categories**:
  - Vehicles, Property Rentals, Electronics, Apparel & Accessories, Home & Garden, Sporting Goods, Toys & Games, Pet Supplies, Free Stuff.
- **Search & Filtering**:
  - Keyword search with auto-suggest and recent query history.
  - Location radius slider (e.g., within 5 km to 100 km).
  - Price filtering (minimum and maximum price boundaries, or "Free").
  - Condition filters (New, Used - Like New, Used - Good, Used - Fair).
  - Sorting criteria: Recommended / Relevant, Distance Nearest, Price Low-to-High, Price High-to-Low, Date Listed Newest.

### 6.2 Listing Details & Seller Profiles
- **Listing Presentation**:
  - Multi-image swipeable carousel with full-screen zoom.
  - Clear item title, price tag (or "Free"), category breadcrumbs, condition badge, and detailed item description.
  - **Approximate Location**: Shows a generalized geographic circle/radius without exposing the seller's exact street address.
- **Seller Trust Card**:
  - Seller profile summary, account age, verification badges, seller rating (1–5 stars), and past customer reviews.

### 6.3 Listing Creation & Management
- **Seller Studio**:
  - Upload up to 15 high-resolution photos.
  - Structured fields: Title, Price, Category, Condition, Description, Tags.
  - **Meet-Up Preferences**:
    - Public Meetup (in a safe, public location).
    - Door Pickup (buyer collects from doorstep).
    - Door Dropoff (seller delivers to buyer).
- **Seller Dashboard**:
  - Tabular management of **Active Listings**, **Pending Sales**, and **Sold Items**.
  - One-tap status updates: "Mark as Pending", "Mark as Sold", "Edit Listing", "Delete Listing", and "Renew / Boost Listing".

### 6.4 Buyer-Seller Chat & Negotiation
- **In-App Transaction Messaging**:
  - Direct inquiry channel initiated from the listing with canned prompt: *"Hi, is this still available?"*.
  - **Make an Offer Flow**:
    - Buyer submits a binding or suggested offer price.
    - Seller can **Accept**, **Decline**, or submit a **Counter-Offer**.
- **Saved Items / Watchlist**:
  - Buyers can bookmark listings to track price drops or availability changes.

---

## 7. Events

Soshal Events provides a comprehensive event management and social gathering platform featuring multiple dedicated screens, calendar views with day-cell attendance badges, and audience-type discovery.

### 7.1 Screen 1: Calendar View Screen
- **Month & Week Grid**:
  - Interactive calendar interface providing a high-level visual overview of upcoming plans.
- **Day-Cell Attendance Icons**:
  - **Every day cell in the calendar grid renders distinct visual badges and icons for events the user is attending on that specific day.**
  - Visual differentiation between RSVP states:
    - Filled colored badge/icon for **Attending / Going**.
    - Outlined badge/icon for **Interested**.
  - Event category icons rendered directly inside or adjacent to the day cell (e.g., musical note for concerts, party popper for celebrations, briefcase for professional meetups).
- **Date Tap & Agenda Sheet**:
  - Tapping any date cell smoothly expands an agenda list at the bottom of the screen showing chronological details of all events scheduled for that day, with quick navigation to full details.

### 7.2 Screen 2: Audience Discovery Screen
- **Dedicated Audience Filtering**:
  - A dedicated discovery screen allowing users to browse events segmented strictly by **Audience Type**:
    - **Public Events**: Open to everyone across the network and relays.
    - **Friends of Friends Events**: Gatherings hosted or attended by second-degree social connections (Web of Trust discovery).
    - **Friends Events**: Intimate, private gatherings hosted by direct mutual friends and contacts.
- **Temporal & Proximity Sub-Filters**:
  - Quick chips: **Today**, **Tomorrow**, **This Weekend**, **Choose Date Range**.
  - Location toggles: **Local / In-Person** (with distance radius) versus **Online / Virtual Events** (with stream/meeting link).

### 7.3 Screen 3: Event Detail Screen
- **Full Event Overview**:
  - Hero banner cover image, event title, host avatar and bio, date and time with recurrence details.
  - One-tap "Add to Calendar" (device calendar sync).
  - Location details: Physical address with embedded map and turn-by-turn directions, or virtual streaming link for online broadcasts.
  - **RSVP Buttons**: **Going**, **Interested**, **Can't Go**.
  - **Attendee List**: Filterable by Going, Interested, and Invited, highlighting mutual friends attending.
  - **Event Discussion Wall**: Dedicated feed for host announcements and attendee questions/media sharing.

### 7.4 Screen 4: Event Creation & Management
- Multi-step event creation wizard:
  - Audience selection (**Public**, **Friends of Friends**, **Friends**, or **Private Invite-Only**).
  - Date, start time, end time, physical venue or online link, co-host designations, ticketing/cost details, and attendee capacity limits.

---

## 8. Minis

Minis are Soshal's short-form vertical video experience, delivering complete feature parity with **Instagram Reels**.

### 8.1 Full-Screen Vertical Video Player
- **Immersive Video Pager**:
  - Edge-to-edge 9:16 vertical video presentation with smooth vertical swipe up/down gesture navigation.
  - Intelligent background pre-buffering of preceding and succeeding clips for zero-latency scrolling.
  - Tap to pause/resume with animated play indicator; double-tap to like with heart burst animation.

### 8.2 Interaction & Action Bar (Right Rail)
- **Heart / Like**: Reaction toggle displaying total likes with real-time counters.
- **Comments**: Opens an interactive slide-up bottom sheet supporting nested comment threads and creator pinned comments.
- **Share / Send**: Instantly forward the Mini to friends via Messenger, share to external apps, or copy link.
- **Audio Disc**: Rotating album art thumbnail in the bottom right corner with animated sound waves; tapping opens the Sound/Audio page.
- **Remix / Duet**: Create a side-by-side or sequential video response using the original clip.
- **Overflow Menu (⋯)**: Not Interested, Save to Collection, Copy Link, Report, Mute Audio.

### 8.3 Creator Overlay (Bottom-Left)
- **Creator Identity**:
  - Avatar, creator handle, and a prominent **Follow / Unfollow** button.
- **Caption & Metadata**:
  - Multi-line caption with collapsible "more" expansion.
  - Clickable hashtags (`#trending`) and tagged accounts (`@creator`).
  - Scrolling marquee displaying audio track title and artist name ("Original Audio - @creator" or licensed track).

### 8.4 Audio & Sound Library Page
- Tapping any audio title or rotating disc navigates to the dedicated **Audio Page**:
  - Shows track title, artist, number of Minis created with this sound, and waveform preview.
  - Grid of all Minis published using this specific audio track.
  - Prominent **"Use Audio"** button launching the creation camera with the audio pre-selected and synced.

### 8.5 Minis Creation Studio
- **Capture Camera**:
  - Multi-segment recording (record, pause, continue next shot).
  - Recording timer and hands-free countdown (3s or 10s).
  - Speed adjustments: 0.3x, 0.5x, 1x, 2x, 3x.
  - Front / rear camera toggle with torch/flash.
  - Video trimmer, clip reordering, text overlay editor with fonts/colors, and sticker placements.

---

## 9. Live

Soshal Live provides an interactive, broadcast-grade streaming experience functioning with complete feature parity to **Twitch**.

### 9.1 Stream Broadcast & Playback
- **Low-Latency Streaming**:
  - Sub-second video playback powered by Media over QUIC (MoQ) and WebRTC / RTMP streaming pipelines (`streaming-core`).
  - Transcoding & Quality Selector: Manual switching between **Source / 1080p60**, **720p60**, **480p**, **360p**, and **Auto**.
- **Player Controls**:
  - Full-screen mode, theater mode (expanding video while keeping chat visible on the side), picture-in-picture (PiP), and volume slider with mute toggle.

### 9.2 Real-Time Live Chat
- **High-Throughput Chat Stream**:
  - Scalable real-time messaging sidebar / overlay accompanying the live broadcast.
- **Badges & Verification**:
  - Chat badges displayed beside usernames: **Broadcaster** (🎥), **Moderator** (⚔️), **VIP** (💎), **Subscriber** (⭐), **Verified**.
- **Emotes & Reactions**:
  - Global system emotes, channel-specific custom emotes, and animated chat reactions.
- **Chat Modes**:
  - **Follower-Only Mode** (requires user to have followed for a set duration).
  - **Subscriber-Only Mode**.
  - **Emote-Only Mode**.
  - **Slow Mode** (rate limits users between messages by *N* seconds).

### 9.3 Stream & Channel Information
- Streamer profile header with avatar, stream title, current playing game / category directory tag (e.g., Just Chatting, Music, Gaming, Tech), live viewer counter, stream uptime clock, and Follow / Subscribe buttons.

### 9.4 Moderation Tools (Mod View)
- Broadcaster and appointed moderators have access to live moderation tools:
  - Timeout user (1m, 10m, 24h).
  - Permanent ban from chat.
  - Delete individual offensive message.
  - Clear entire chat history.

### 9.5 Community Support, Tipping, Raids & VODs
- **Lightning Zaps / "Bits" Tipping**:
  - Viewers can tip the broadcaster with on-screen animated cheer alerts and top tipper leaderboards.
- **Raids & Channel Hosting**:
  - When concluding a broadcast, streamers can initiate a **Raid**, sending all current active viewers over to another live creator's channel.
- **Clips & Past Broadcasts (VODs)**:
  - Viewers can generate 30-to-60 second **Clips** from live streams to share across the feed and Minis.
  - Automatically archived Past Broadcasts (VODs) with full synchronized chat replay.

---

## 10. Musicloud

Musicloud is Soshal's decentralized audio distribution and streaming ecosystem, delivering full feature parity with **SoundCloud**.

### 10.1 Interactive Waveform Player
- **Visual Waveform Canvas**:
  - Full-width rendered audio waveform displaying amplitudes of the entire track.
  - Interactive touch seeking: Tap or drag anywhere along the waveform to instantly scrub playback.
  - Play, pause, skip forward, skip back, loop/repeat, and shuffle controls.

### 10.2 Timed Waveform Comments
- **Timestamped Community Comments**:
  - Listeners can drop comments at exact timestamps along the track (e.g., at 02:14: *"That bass drop!"*).
  - Comments appear as small avatar icons pinned along the waveform timeline.
  - When the playback playhead reaches a comment's timestamp, an animated speech bubble appears briefly above the waveform.

### 10.3 Creator Upload Studio
- **Audio Publishing**:
  - Upload lossless audio formats (FLAC, WAV, AIFF) and compressed formats (MP3, AAC, OGG).
  - High-resolution square artwork uploader.
  - Track metadata: Title, Performing Artist, Featured Artists, Genre (Hip-Hop, Electronic, Rock, Ambient, Podcast, etc.), Tags, Release Date, Description.
  - Privacy options: **Public** release or **Private Link** (secret shareable URL).

### 10.4 Stream Feed & Discovery
- **Chronological Stream**:
  - Continuous chronological feed of new track releases and reposts from artists the user follows.
- **Charts & Recommendations**:
  - Top 50 trending tracks globally and by genre.
  - "Related Tracks" and algorithmic discovery mixes based on listening history.

### 10.5 Sets & Playlists
- Create, curate, and share public or private Playlists / Sets.
- Reorder track queues with drag-and-drop playlist editors.

### 10.6 Social Features & Artist Profiles
- **Social Distribution**:
  - **Repost**: Repost a track directly onto the user's followers' stream feeds.
  - **Like**: Add track to the user's personal "Liked Tracks" library.
  - **Share with Timestamp**: Generate link containing exact playback offset (e.g., `?t=01:45`).
- **Artist Spotlight Profile**:
  - Header banner with featured "Spotlight" track pinned at the top.
  - Segmented discography tabs: **Tracks**, **Albums**, **Playlists**, **Reposts**, **Likes**.

### 10.7 Persistent Background Player
- Minified bottom playback bar across all app screens with play/pause and track title.
- Native OS background audio integration with lockscreen media controls and system notification tray widgets.

---

## 11. ChatRandom

ChatRandom delivers an instant peer discovery and communication experience inspired by **Chatroulette**, augmented to support **all input types** (video, audio, text) and **both 1-on-1 and group** interaction topologies.

### 11.1 Instant Random Matchmaking
- **One-Tap Connect**:
  - Users tap "Start" to be instantly paired with active peers currently in the matchmaking pool.
- **Instant Next / Skip**:
  - Prominent "Next" button immediately disconnects the current session and seamlessly establishes a connection with a new candidate without returning to a menu.

### 11.2 Multimodal Input Modalities (All Media Types)
Users can select and switch between three primary communication modes:
1. **Video Mode (Full AV)**:
   - Live two-way WebRTC camera feed + microphone audio.
   - Self-view picture-in-picture preview, camera toggle (front / back), video mute, and audio mute.
2. **Audio-Only Mode**:
   - Voice chat without video transmission.
   - Displays avatar/visualizer or animated audio wave while transmitting voice, ideal for low-bandwidth environments or privacy-conscious users.
3. **Text-Only Mode**:
   - High-speed anonymous text chat without camera or microphone access required.
   - Clean, lightweight messenger interface with instant delivery and typing state.
- **Dynamic Mode Negotiation**:
  - Users can specify preferred input types, or match with peers who accept any compatible subset (e.g., video user can converse with an audio user if mutually accepted).

### 11.3 Match Topologies: 1-on-1 & Group Matching
- **1-on-1 Random Match**:
  - Classic pairwise connection between two randomized users.
  - Private, direct peer-to-peer WebRTC connection.
- **Group Random Match**:
  - Dynamic multi-party random rooms where **3 to 8 participants** are automatically pooled into a shared video/audio/text lounge.
  - As participants leave or hit "Next", new candidates are routed into the empty slots in the room.
  - Room participants can engage in group video tile displays or collective text chat.

### 11.4 Filters, Interests & Matching Preferences
- **Interest Tags**:
  - Enter topic tags (e.g., `#gaming`, `#music`, `#language-exchange`, `#coding`).
  - Algorithm prioritizes matching users sharing common interest tags before falling back to general pool.
- **Language & Region Filtering**:
  - Filter candidates by spoken language or regional geolocation.

### 11.5 Safety, Privacy & Moderation
- **Real-Time Automated Protection**:
  - On-device lightweight computer vision check to detect and blur inappropriate/NSFW content before unmasking.
  - Option to start video sessions with blurred camera until both users confirm connection.
- **One-Tap Report & Block**:
  - Instant button to report abusive conduct and immediately disconnect. Blocked pubkeys are stored in the local SQLite blacklist to ensure they are never matched again.
- **Identity Privacy**:
  - ChatRandom sessions utilize ephemeral, short-lived Nostr keys or pseudonymous session IDs to prevent unsolicited tracking back to the user's primary social profile.

---

## 12. Architectural Mapping Matrix

| Domain | Primary Reference | Soshal Rust Core Crates | Key Flutter Screens / Services | Protocols & Standards |
|---|---|---|---|---|
| **Feed** | Facebook | `feed-core`, `content-core`, `db-core` | `feed_screen.dart`, `feed_service.dart` | NIP-01, NIP-18, NIP-25, NIP-57 |
| **Notifications** | Facebook (w/ Ignore) | `notification-core`, `db-core` | `notifications_screen.dart`, `notifications_service.dart` | Local SQLite filtering, push tokens |
| **Messages** | Facebook Messenger | `messaging-core`, `crypto-core`, `webrtc-core` | `inbox_screen.dart`, `thread_screen.dart`, `messaging_service.dart` | NIP-44 v2, Double Ratchet, PQC ML-KEM |
| **Groups** | Discord | `groups-core`, `nostr-core`, `db-core` | `groups_screen.dart`, `groups_service.dart`, `group_tabs.dart` | NIP-29 (relay-based groups), WebRTC |
| **Dating** | Facebook Dating | `dating-core`, `social-core`, `db-core` | `dating_screen.dart`, `dating_profile_screen.dart`, `dating_service.dart` | Encrypted match signals, Secret Crush |
| **Marketplace** | Facebook Marketplace | `marketplace-core`, `db-core` | `marketplace_screen.dart`, `marketplace_service.dart` | NIP-15 (listings), NIP-44 chat |
| **Events** | Facebook Events | `events-core`, `spatial-core`, `db-core` | `events_screen.dart`, `events_service.dart` | NIP-52 (calendar events), Geohash |
| **Minis** | Instagram Reels | `minis-core`, `media-core`, `db-core` | `minis_screen.dart`, `minis_service.dart` | NIP-95/96 Blossom, vertical video |
| **Live** | Twitch | `streaming-core`, `webrtc-core` | `live_broadcast_screen.dart`, `moq_viewer_screen.dart` | NIP-30080/30081, MoQ, WebRTC |
| **Musicloud** | SoundCloud | `audio-core`, `storage-core`, `db-core` | `music_screen.dart`, `music_service.dart` | Blossom audio blobs, waveform cache |
| **ChatRandom** | Chatroulette (Multimodal) | `social-core`, `webrtc-core`, `streaming-core` | `chatrandom_service.dart`, WebRTC screens | Ephemeral Nostr signaling, WebRTC P2P |
