/** Create a lowercase canonical UUIDv7 for one semantic browser action. */
export function createUuidV7(now = Date.now()): string {
  const bytes = new Uint8Array(16)
  const cryptoApi = globalThis.crypto
  if (!cryptoApi?.getRandomValues) {
    throw new Error('Secure browser randomness is unavailable.')
  }
  cryptoApi.getRandomValues(bytes)

  let timestamp = Math.max(0, Math.floor(now))
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = timestamp % 256
    timestamp = Math.floor(timestamp / 256)
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x70
  bytes[8] = (bytes[8] & 0x3f) | 0x80

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
