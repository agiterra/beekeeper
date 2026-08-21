/**
 * Domain-separated, UTF-8 byte-length-prefixed key serialization.
 *
 * Every identity string in the coding-session consumer goes through this so a
 * field value that happens to contain a delimiter can never make two distinct
 * tuples serialize identically. The same encoding is used by the provider and
 * the SDK, so keys minted on either side compare byte for byte.
 */
export function encodeStructuredKey(
  domain: string,
  ...fields: readonly string[]
): string {
  const encoder = new TextEncoder();
  const encodedFields = fields
    .map((field) => `${encoder.encode(field).byteLength}:${field}`)
    .join("");
  return `${domain}|${encodedFields}`;
}
