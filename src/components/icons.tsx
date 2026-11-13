import type { SVGProps } from "react";

const base = (props: SVGProps<SVGSVGElement>) => ({
  width: 16,
  height: 16,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
  ...props,
});

export const SearchIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.5 10.5 3.5 3.5" />
  </svg>
);

export const FolderIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M1.5 4.5v7.5a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1H8L6.5 3.5h-4a1 1 0 0 0-1 1Z" />
  </svg>
);

export const PlusIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M8 3v10M3 8h10" />
  </svg>
);

export const CloseIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="m4 4 8 8M12 4l-8 8" />
  </svg>
);

export const StopIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)} fill="currentColor" stroke="none">
    <rect x="3.5" y="3.5" width="9" height="9" rx="1.5" />
  </svg>
);

export const PauseIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)} fill="currentColor" stroke="none">
    <rect x="3.5" y="3" width="3.2" height="10" rx="1" />
    <rect x="9.3" y="3" width="3.2" height="10" rx="1" />
  </svg>
);

export const RefreshIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
    <path d="M13.5 2.5v3h-3" />
  </svg>
);

export const LoopIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3 7V6a2 2 0 0 1 2-2h7.5M10.5 2l2 2-2 2" />
    <path d="M13 9v1a2 2 0 0 1-2 2H3.500M5.500 14l-2-2 2-2" />
  </svg>
);

export const WarningIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M8 2 1.500 13.500h13L8 2Z" />
    <path d="M8 6.500v3.500M8 12v.01" />
  </svg>
);

export const VolumeIcon = ({ muted, ...p }: SVGProps<SVGSVGElement> & { muted?: boolean }) => (
  <svg {...base(p)}>
    <path d="M2 6v4h2.500L8 13V3L4.500 6H2Z" />
    {muted ? <path d="m10.500 6 4 4M14.500 6l-4 4" /> : <path d="M10.500 5.500a3.500 3.500 0 0 1 0 5M12.500 3.500a6 6 0 0 1 0 9" />}
  </svg>
);

export const PlayIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)} fill="currentColor" stroke="none">
    <path d="M4.5 2.8v10.4a.6.6 0 0 0 .92.5l8-5.2a.6.6 0 0 0 0-1l-8-5.2a.6.6 0 0 0-.92.5Z" />
  </svg>
);

export const NextIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3.5 3.5 10 8l-6.5 4.5v-9Z" fill="currentColor" />
    <path d="M12.5 3.5v9" />
  </svg>
);

export const SplitIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <path d="M8 3v10" />
  </svg>
);

export const CheckIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="m3.5 8.5 3 3 6-7" />
  </svg>
);

export const ChevronIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="m4 6 4 4 4-4" />
  </svg>
);

export const LibraryIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3 2.5v11M6 2.5v11M9.2 3l3.3 10" />
  </svg>
);

export const SceneIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <path d="m2.5 11 3.5-3.5 2.5 2.5 2-2 3 3" />
  </svg>
);

export const GridIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" />
    <rect x="9" y="2.5" width="4.5" height="4.5" rx="1" />
    <rect x="2.5" y="9" width="4.5" height="4.5" rx="1" />
    <rect x="9" y="9" width="4.5" height="4.5" rx="1" />
  </svg>
);

export const GearIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="8" cy="8" r="2.2" />
    <path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M12.4 3.6l-1.2 1.2M4.8 11.2l-1.2 1.2" />
  </svg>
);

export const PencilIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M11 2.5 13.5 5 5.5 13H3v-2.5l8-8Z" />
  </svg>
);

export const TrashIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5" />
  </svg>
);

export const ArrowIcon = ({ up, ...p }: SVGProps<SVGSVGElement> & { up?: boolean }) => (
  <svg {...base(p)}>
    <path d={up ? "m4 9.5 4-4 4 4" : "m4 6.5 4 4 4-4"} />
  </svg>
);

export const WaveIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M2 8h1.5M5 4.5v7M8 2.5v11M11 5.5v5M13.5 8H14" />
  </svg>
);

export const ListIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" />
  </svg>
);

export const BoltIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M9 1.5 3.5 9H8l-1 5.5L12.5 7H8l1-5.5Z" />
  </svg>
);
