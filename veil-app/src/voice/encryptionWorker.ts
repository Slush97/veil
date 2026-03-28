/**
 * Web Worker for E2E encrypting/decrypting WebRTC media frames.
 *
 * Frame format (prepended to each encrypted frame):
 *   [1 byte flags] [8 bytes generation BE] [4 bytes counter BE] [encrypted payload] [16 bytes GCM tag]
 *
 * Uses AES-256-GCM via the Web Crypto API for hardware-accelerated encryption.
 * The key is derived from the group key via GroupKey::derive_channel_key(b"voice").
 */

const HEADER_SIZE = 1 + 8 + 4; // flags + generation + counter
const TAG_SIZE = 16; // AES-GCM authentication tag

let encryptionKey: CryptoKey | null = null;
let keyGeneration = 0;
let frameCounter = 0;
let direction: 'encrypt' | 'decrypt' = 'encrypt';

// Handle messages from the main thread
self.onmessage = async (event: MessageEvent) => {
  const msg = event.data;

  if (msg.type === 'init') {
    direction = msg.direction;
    keyGeneration = msg.generation ?? 0;
  } else if (msg.type === 'setKey') {
    const keyBytes: Uint8Array = msg.keyBytes;
    keyGeneration = msg.generation;
    frameCounter = 0;

    encryptionKey = await crypto.subtle.importKey(
      'raw',
      keyBytes,
      { name: 'AES-GCM' },
      false,
      ['encrypt', 'decrypt'],
    );
  }
};

// RTCRtpScriptTransform entry point
// @ts-expect-error - onrtctransform is the worker-side handler
self.onrtctransform = (event: { transformer: { readable: ReadableStream; writable: WritableStream } }) => {
  const { readable, writable } = event.transformer;

  const transform = new TransformStream({
    async transform(frame: RTCEncodedAudioFrame, controller: TransformStreamDefaultController) {
      if (!encryptionKey) {
        // Pass through unencrypted if no key yet
        controller.enqueue(frame);
        return;
      }

      try {
        if (direction === 'encrypt') {
          await encryptFrame(frame, controller);
        } else {
          await decryptFrame(frame, controller);
        }
      } catch (e) {
        // On error, drop the frame rather than passing garbage
        console.error(`Voice ${direction} error:`, e);
      }
    },
  });

  readable.pipeThrough(transform).pipeTo(writable);
};

async function encryptFrame(
  frame: RTCEncodedAudioFrame,
  controller: TransformStreamDefaultController,
): Promise<void> {
  const data = new Uint8Array(frame.data);

  // Build the 13-byte header
  const header = new Uint8Array(HEADER_SIZE);
  const view = new DataView(header.buffer);

  // Flags byte: 0x01 = encrypted
  header[0] = 0x01;

  // Generation (8 bytes, big-endian)
  // DataView doesn't have setBigUint64 in all environments, write as two u32s
  view.setUint32(1, Math.floor(keyGeneration / 0x100000000), false);
  view.setUint32(5, keyGeneration >>> 0, false);

  // Counter (4 bytes, big-endian)
  view.setUint32(9, frameCounter, false);
  frameCounter = (frameCounter + 1) >>> 0;

  // Use header as IV (padded to 12 bytes for AES-GCM)
  const iv = new Uint8Array(12);
  iv.set(header.subarray(1, 13)); // generation(8) + counter(4) = 12 bytes

  const encrypted = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv, additionalData: header },
    encryptionKey!,
    data,
  );

  // Assemble: header + ciphertext (includes 16-byte GCM tag appended by WebCrypto)
  const encryptedBytes = new Uint8Array(encrypted);
  const output = new Uint8Array(HEADER_SIZE + encryptedBytes.length);
  output.set(header, 0);
  output.set(encryptedBytes, HEADER_SIZE);

  frame.data = output.buffer;
  controller.enqueue(frame);
}

async function decryptFrame(
  frame: RTCEncodedAudioFrame,
  controller: TransformStreamDefaultController,
): Promise<void> {
  const data = new Uint8Array(frame.data);

  if (data.length < HEADER_SIZE + TAG_SIZE) {
    // Too short to be an encrypted frame — pass through (could be unencrypted)
    controller.enqueue(frame);
    return;
  }

  const flags = data[0];
  if ((flags & 0x01) === 0) {
    // Not encrypted, pass through
    controller.enqueue(frame);
    return;
  }

  const header = data.subarray(0, HEADER_SIZE);
  const ciphertext = data.subarray(HEADER_SIZE);

  // Reconstruct IV from header
  const iv = new Uint8Array(12);
  iv.set(header.subarray(1, 13));

  const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv, additionalData: header },
    encryptionKey!,
    ciphertext,
  );

  frame.data = decrypted;
  controller.enqueue(frame);
}

// RTCEncodedAudioFrame type augmentation (if not in lib.dom.d.ts)
type VoiceFrame = RTCEncodedAudioFrame;
