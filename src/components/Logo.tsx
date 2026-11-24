// The CaptionNest mark: three caption lines woven into a nest, holding the gold
// "voice" egg. Same drawing as brand/app-icon.svg, without the tile.

export function LogoMark({ size = 28, title }: { size?: number; title?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="250 280 524 520"
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <defs>
        <linearGradient id="lm-egg" x1="0" y1="0" x2="0.35" y2="1">
          <stop offset="0" stopColor="#FCE29A" />
          <stop offset="0.5" stopColor="#F2BD4B" />
          <stop offset="1" stopColor="#D9952C" />
        </linearGradient>
        <linearGradient id="lm-strand" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#F6EFDF" />
          <stop offset="1" stopColor="#D9CDB5" />
        </linearGradient>
      </defs>
      <path
        d="M512 318 C 566 318, 606 392, 606 462 C 606 522, 564 560, 512 560 C 460 560, 418 522, 418 462 C 418 392, 458 318, 512 318 Z"
        fill="url(#lm-egg)"
      />
      <g fill="none" strokeLinecap="round">
        <path d="M268 498 Q 510 650 760 512" stroke="var(--logo-cut, #14110E)" strokeWidth="76" />
        <path d="M268 498 Q 510 650 760 512" stroke="url(#lm-strand)" strokeWidth="50" />
        <path d="M318 616 Q 506 722 708 600" stroke="url(#lm-strand)" strokeWidth="50" strokeOpacity="0.8" />
        <path d="M396 708 Q 516 776 630 714" stroke="url(#lm-strand)" strokeWidth="50" strokeOpacity="0.6" />
      </g>
    </svg>
  );
}

export function Wordmark() {
  return (
    <span className="wordmark" aria-label="CaptionNest">
      <LogoMark size={22} />
      <span className="wordmark__text display">
        Caption<span className="wordmark__nest">Nest</span>
      </span>
    </span>
  );
}
