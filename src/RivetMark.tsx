interface RivetMarkProps {
  size?: number;
}

/** 겹친 두 금속판을 리벳 하나가 고정하는 브랜드 마크 */
export function RivetMark({ size = 64 }: RivetMarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <rect x="5" y="13" width="34" height="26" rx="6" fill="var(--steel)" />
      <rect x="25" y="25" width="34" height="26" rx="6" fill="var(--signal)" />
      <circle cx="32" cy="32" r="8.5" fill="var(--graphite)" />
      <circle cx="32" cy="32" r="6" fill="#E9E4DA" />
    </svg>
  );
}
