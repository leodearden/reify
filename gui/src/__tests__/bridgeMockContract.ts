/**
 * Pure set helpers for the bridge-mock coverage guard: which bridge.ts runtime
 * exports the shared App factory neither mocks nor allowlists, and which
 * allowlist entries have rotted. The invariant they enforce, and why a gap in
 * it fails silently, are documented once in the consumer —
 * `bridgeMockCoverage.test.ts`. Not restated here.
 *
 * This file has no `.test.` segment, so vitest does not collect it as a suite
 * (same convention as `debugBridgeTestHelpers.ts`).
 */

/**
 * Runtime exports covered by neither the factory nor the allowlist — i.e. the
 * gaps. Sorted, so a failure message reads as a stable list.
 *
 * Factory keys that are not runtime exports are ignored here; catching those is
 * the consumer's sanity check, which fails with a clearer message.
 */
export function missingFactoryKeys(
  runtimeExports: string[],
  factoryKeys: string[],
  omissions: Record<string, string>,
): string[] {
  const covered = new Set(factoryKeys);
  const allowed = new Set(Object.keys(omissions));
  return [...new Set(runtimeExports.filter((n) => !covered.has(n) && !allowed.has(n)))].sort();
}

/**
 * Allowlist entries that have rotted, either because `bridge.ts` no longer
 * exports the name, or because the factory in fact mocks it. Sorted union of
 * the two conditions, without duplicates.
 *
 * Without this the allowlist decays into a rubber stamp: entries whose stated
 * reason stopped being true keep suppressing checks nobody re-reads.
 */
export function staleOmissions(
  runtimeExports: string[],
  factoryKeys: string[],
  omissions: Record<string, string>,
): string[] {
  const exported = new Set(runtimeExports);
  const covered = new Set(factoryKeys);
  return Object.keys(omissions)
    .filter((n) => !exported.has(n) || covered.has(n))
    .sort();
}
