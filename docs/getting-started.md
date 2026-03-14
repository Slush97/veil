# Getting Started with Veil

Veil is a private, decentralized messenger. Messages are end-to-end encrypted — no server (not even the relay) can read them. There are no accounts, no phone numbers, no email addresses. Just cryptographic keys that you control.

---

## 1. Create Your Identity

When you first open Veil, you'll see the setup screen.

1. **Set a passphrase** (optional but recommended) — this encrypts your identity on disk so someone with access to your computer can't steal it.
2. Click **Create New**.
3. You'll see a **12-word recovery phrase**. Write it down on paper and store it somewhere safe. This is the only way to recover your identity if you lose your device. Nobody can reset it for you.
4. Click **"I have saved my recovery phrase"** to enter the app.

If you already have an identity saved (from a previous session), just enter your passphrase and click **Load Existing**.

---

## 2. Connect to a Relay

Veil can send messages directly between peers, but if both people aren't online at the same time, messages get lost. A relay server holds encrypted messages until the recipient comes online — like a mailbox.

1. In the sidebar under **Relay**, enter the relay address (e.g. `relay.example.com:4433` or `127.0.0.1:4433` for local testing).
2. Click **Connect Relay**.
3. You should see "Relay connected" in the status area.

The relay is zero-knowledge — it forwards opaque encrypted blobs and cannot read any message content.

---

## 3. Create or Join a Group

All conversations happen inside **groups**. A group is a set of people who share an encryption key.

### Creating an Invite (sender)

1. In the sidebar under **Invite**, type a passphrase. This is a one-time passphrase for the invite — not your identity passphrase. Share it with the person you're inviting through a separate channel (in person, phone call, etc.).
2. Click **Create Invite**.
3. An invite URL will appear. Copy it and send it to the other person.

### Joining a Group (recipient)

1. Enter the same passphrase the sender used.
2. Paste the invite URL into the "Paste invite URL" field.
3. Click **Join**.

You'll see "Invite accepted!" and the new group will appear in the sidebar. You can now exchange messages.

---

## 4. Sending Messages

- Type in the message box at the bottom and press **Enter** or click **Send**.
- Messages show a status indicator: `...` (sending), `ok` (sent).
- If you're offline, messages queue locally and send when you reconnect.

### Replies

Click the **reply** button on any message to reply to it. A preview of the original message appears above your input. Press **Esc** to cancel.

### Reactions

Click an emoji button next to a message to react. Reaction counts appear below the message.

### Edit & Delete

You can **edit** or **delete** your own messages using the buttons that appear on hover. Edits show "(edited)" next to the message. Pressing **Up Arrow** with an empty input edits your last message.

---

## 5. Sending Files

1. Click the **File** button next to the message input.
2. Select a file from the file picker.
3. The file is encrypted and sent to the group.

- **Small files** (< 1 MB) are sent inline with the message.
- **Large files** are split into erasure-coded shards and distributed to peers. Recipients can reconstruct the file from any 4 of 7 shards.

When you receive a file, you'll see a file widget with the filename, size, and a **Save** button. Click Save to decrypt and save it to your computer.

---

## 6. Channels

Each group has channels (like `#general`, `#random`). Click a channel name in the sidebar to switch. Messages are filtered by channel.

---

## 7. Display Names

By default, people show up as cryptographic fingerprints (e.g. `a3f2...c891`). To set a human-readable name:

1. In the sidebar under **Display Name**, type your name.
2. Click **Set**.

Your display name is visible to everyone in the group.

---

## 8. LAN Discovery

If someone on your local network is running Veil, they'll appear automatically under **LAN Peers** in the sidebar. Click their name to connect directly — no relay needed.

---

## 9. Search

Click the search icon or use the search bar at the top of the chat to search through message history. Matching messages are highlighted and non-matching ones are hidden.

---

## 10. Settings

Click **Settings** in the sidebar to access:

- **Theme** — toggle between dark and light mode.
- **Notifications** — enable or disable desktop notifications for incoming messages.
- **Identity info** — view your fingerprint and device name.

---

## Key Concepts

| Concept | What it means |
|---|---|
| **Identity** | Your cryptographic keypair. Created locally, never leaves your device. |
| **Recovery phrase** | 12 words that can regenerate your identity. Write it down, keep it safe. |
| **Group** | A set of people sharing an encryption key. All messages in a group are encrypted to that key. |
| **Relay** | A server that holds encrypted messages for offline recipients. Cannot read message content. |
| **Routing tag** | A derived identifier that the relay uses to route messages. The relay can't determine which group it belongs to. |
| **Fingerprint** | A short hex string derived from your public key. Used to identify you before setting a display name. |

---

## Testing Locally

To run two instances on the same machine for testing:

```bash
# Terminal 1: Start the relay
cargo run --release --bin veil-relay -- 127.0.0.1:4433

# Terminal 2: Client A
VEIL_DATA_DIR=/tmp/veil-a cargo run --release --bin veil-client

# Terminal 3: Client B
VEIL_DATA_DIR=/tmp/veil-b cargo run --release --bin veil-client
```

Create identities in both clients, connect both to the relay at `127.0.0.1:4433`, create an invite in Client A, accept it in Client B, and start chatting.
