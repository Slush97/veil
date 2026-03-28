# Veil Roadmap

Current state: E2E encrypted text + voice messaging, Tauri app, embedded relay hosting, Tailscale integration, group/channel CRUD.

---

## P0 — Auth & Onboarding

**Goal:** Replace recovery-phrase-only flow with frictionless sign-up/login.

- [ ] Relay-side auth: Supabase + Google OAuth integration on the relay server
- [ ] Token-based session management (relay issues JWT after OAuth, client stores it)
- [ ] Relay admin panel: approve/deny new users, manage relay access
- [ ] Passphrase-protected identity backup/restore (keep as fallback)
- [ ] Account linking: tie OAuth identity to Ed25519 master key on first login

**Notes:** The crypto identity system stays — OAuth is an onboarding layer, not a replacement. The relay validates OAuth tokens and maps them to peer IDs. Master keys never leave the device.

---

## P1 — Direct Messages & Friends

**Goal:** 1:1 encrypted conversations outside of group context.

- [ ] DM data model: DM channels as a special group (2-member, no relay routing tag broadcast)
- [ ] Friend request flow: send invite via username lookup (relay directory already supports `Register`/`Lookup`)
- [ ] DM list in the Home/DMs view (the left icon in ServerStrip already exists)
- [ ] Online/offline presence tracking via relay heartbeats
- [ ] Notification badges on DM conversations

**Notes:** DMs can reuse the same `SealedMessage` envelope with a 2-member `GroupKeyRing`. The relay directory already supports username registration and lookup.

---

## P2 — Message Editing, Deletion & Reactions

**Goal:** Standard chat UX expectations.

- [ ] `MessageKind::Edit` — already defined in veil-core, needs frontend wiring
- [ ] `MessageKind::Delete` — soft-delete (tombstone), UI removes message body
- [ ] `MessageKind::Reaction` — emoji reactions on messages, already defined in core
- [ ] Edit indicator ("edited" label + timestamp)
- [ ] Confirmation dialog for message deletion
- [ ] Optimistic UI updates for all three operations

**Notes:** The `MessageKind` enum already has `Edit`, `Delete`, and `Reaction` variants. This is mostly frontend + Tauri command wiring.

---

## P3 — File Uploads & Media

**Goal:** Share images, files, and audio clips in channels.

- [ ] File picker integration (Tauri file dialog)
- [ ] Image thumbnails (veil-core already has `generate_thumbnail()`)
- [ ] Audio message recording + waveform display (`MessageKind::Audio` exists)
- [ ] File attachments with download (`MessageKind::File` exists)
- [ ] Inline image/video previews (`MessageKind::Image`, `MessageKind::Video` exist)
- [ ] Blob storage: encrypt + erasure-code files, distribute shards to peers
- [ ] Link previews (`MessageKind::LinkPreview` + `extract_link_preview()` exist in core)

**Notes:** Most of the backend infrastructure exists (blob store, erasure coding, media types). The main work is the Tauri commands to handle file I/O and the React components to render media.

---

## P4 — Notifications & Presence

**Goal:** Make the app feel alive even when you're not looking at it.

- [ ] Typing indicators (wire `PresenceKind::Typing` / `StoppedTyping` to UI — events already flow)
- [ ] Unread counts per channel (not just per group)
- [ ] Desktop notifications via Tauri notification plugin
- [ ] Sound alerts for new messages
- [ ] User status: online/idle/dnd/offline (types exist in `Member`)
- [ ] Idle detection (auto-set status after inactivity)

---

## P5 — Moderation Tools

**Goal:** Give server owners control over their community.

- [ ] Role enforcement: `ControlMessage::RoleChanged` already defined
- [ ] Kick member: `ControlMessage::MemberRemoved` + key rotation (eviction flow exists in crypto)
- [ ] Ban list (persist banned peer IDs, reject on reconnect)
- [ ] Message deletion by moderators
- [ ] Slowmode per channel
- [ ] Audit log (who did what, stored locally on relay)

**Notes:** The crypto layer already handles the hard part — key rotation on member removal with `prepare_eviction()` / `apply_eviction()`. The moderator role and permission flags exist in veil-core.

---

## P6 — Advanced Features

Lower priority, nice-to-have for a polished experience.

- [ ] Threads (reply chains that branch off a message)
- [ ] Message search (backend `search_messages()` exists, needs UI)
- [ ] Custom emoji management (`ControlMessage::AddEmoji` / `RemoveEmoji` exist)
- [ ] User profiles card (bio, status text — types exist in `Member`)
- [ ] Server templates (pre-configured channel layouts)
- [ ] Video/screenshare (extend SFU to handle video tracks)
- [ ] TURN support for voice (currently ICE-lite + STUN only)
- [ ] Mobile app (Tauri mobile or separate React Native build)
- [ ] Message pinning UI (`ControlMessage::PinMessage` exists)

---

## Known Issues

- Speaker detection is stubbed in voice — needs audio level extraction from RTP
- Voice channels are hardcoded to "General" — now fixed with channel CRUD
- `veil://voice-key-rotated` event not emitted from backend yet
- `RelayServer::new()` can panic if DB is locked — should return `Result`
- No TURN support — voice only works when clients can reach the relay directly
