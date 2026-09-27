/** Une clases condicionales. Evita anadir `clsx` solo para esto. */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ')
}
