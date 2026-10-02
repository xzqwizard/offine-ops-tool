/** A digest reference is already complete; a registry port is never mistaken for a tag. */
export function withTag(image: string, tag: string): string {
  if (image.includes('@')) return image
  const colon = image.lastIndexOf(':')
  const base = colon > image.lastIndexOf('/') ? image.slice(0, colon) : image
  return `${base}:${tag || 'latest'}`
}
