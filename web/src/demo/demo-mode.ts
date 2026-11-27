/** `?demo=1` (or `?demo=true`) renders a sample home with local mock state. */
export function isDemoMode(search: string = window.location.search): boolean {
  const value = new URLSearchParams(search).get('demo')
  return value === '1' || value === 'true'
}
