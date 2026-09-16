/** Marca de Appex Backup: bóveda con flecha de descarga. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <rect x="1" y="1" width="30" height="30" rx="8" fill="var(--app-accent)" />
      <ellipse cx="16" cy="9.5" rx="8" ry="3" fill="none" stroke="var(--app-accent-fg)" strokeWidth="2" />
      <path d="M8 9.5v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6" fill="none" stroke="var(--app-accent-fg)" strokeWidth="2" />
      <path d="M16 17.5v8m-3.2-3.2L16 25.5l3.2-3.2" fill="none" stroke="var(--app-accent-fg)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
