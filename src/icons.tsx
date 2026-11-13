import type { SVGProps } from "react";

const I = (d: string, fill = false) =>
  function Icon(p: SVGProps<SVGSVGElement>) {
    return (
      <svg viewBox="0 0 24 24" width={18} height={18} aria-hidden {...p}
        fill={fill ? "currentColor" : "none"} stroke={fill ? "none" : "currentColor"}
        strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
        <path d={d} />
      </svg>
    );
  };

export const Play = I("M7 4.5v15a.5.5 0 0 0 .76.43l12.2-7.5a.5.5 0 0 0 0-.86L7.76 4.07A.5.5 0 0 0 7 4.5Z", true);
export const Pause = I("M6 4h4v16H6zM14 4h4v16h-4z", true);
export const Prev = I("M5 4h2v16H5zM19.2 4.6v14.8a.5.5 0 0 1-.78.42L8.6 12.42a.5.5 0 0 1 0-.84l9.82-7.4a.5.5 0 0 1 .78.42Z", true);
export const Next = I("M17 4h2v16h-2zM4.8 4.6v14.8a.5.5 0 0 0 .78.42l9.82-7.4a.5.5 0 0 0 0-.84L5.58 4.18a.5.5 0 0 0-.78.42Z", true);
export const Shuffle = I("M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5");
export const Repeat = I("M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3");
export const Volume = I("M11 5 6 9H2v6h4l5 4V5ZM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14");
export const Mute = I("M11 5 6 9H2v6h4l5 4V5ZM22 9l-6 6M16 9l6 6");
export const Search = I("M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16ZM21 21l-4.3-4.3");
export const Plus = I("M12 5v14M5 12h14");
export const PlayNext = I("M3 6h12M3 12h8M3 18h8M15 12l6 3.5-6 3.5z");
export const X = I("M18 6 6 18M6 6l12 12");
export const Gear = I("M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z");
export const Speaker = I("M6 2h12a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1ZM12 18a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM12 6.5h.01");
export const Back = I("M15 18l-6-6 6-6");
export const Refresh = I("M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6");
export const Music = I("M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z");
export const Map = I("M3 6l6-3 6 3 6-3v15l-6 3-6-3-6 3V6ZM9 3v15M15 6v15");
export const Player = I("M3 5h18v14H3zM9 5v14");
export const Minimize =I("M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7");
export const Expand = I("M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7");
export const Pin = I("M12 17v5M9 10.76V6h6v4.76a2 2 0 0 0 1.11 1.79l1.78.9A2 2 0 0 1 19 15.24V17H5v-1.76a2 2 0 0 1 1.11-1.79l1.78-.9A2 2 0 0 0 9 10.76ZM8 2h8");
export const Heart =I("M19 14c1.5-1.5 3-3.2 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.8 0-3 .5-4.5 2-1.5-1.5-2.7-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4 3 5.5l7 7Z", true);
export const Cast = I("M2 8V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-6M2 12a9 9 0 0 1 8 8M2 16a5 5 0 0 1 4 4M2 20h.01");
export const ThumbUp = I("M7 10v11M15 5.9 14 10h5.8a2 2 0 0 1 1.9 2.6l-2.3 7A2 2 0 0 1 17.5 21H4a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1h2.8a2 2 0 0 0 1.8-1.1L12 2a3.1 3.1 0 0 1 3 3.9Z");
export const ThumbDown = I("M17 14V3M9 18.1 10 14H4.2a2 2 0 0 1-1.9-2.6l2.3-7A2 2 0 0 1 6.5 3H20a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-2.8a2 2 0 0 0-1.8 1.1L12 22a3.1 3.1 0 0 1-3-3.9Z");
export const Locate = I("M12 2v3M12 19v3M2 12h3M19 12h3M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z");
export const Move = I("M5 9l-3 3 3 3M9 5l3-3 3 3M15 19l-3 3-3-3M19 9l3 3-3 3M2 12h20M12 2v20");
export const Crossfade = I("M3 7h3c5 0 7 10 12 10h3M3 17h3c5 0 7-10 12-10h3");
export const Check = I("M20 6 9 17l-5-5");
export const Sparkle = I("M11 3l1.8 5.2L18 10l-5.2 1.8L11 17l-1.8-5.2L4 10l5.2-1.8L11 3ZM18.5 14.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2Z");
export const ZoomIn = I("M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16ZM21 21l-4.3-4.3M11 8v6M8 11h6");
export const ZoomOut = I("M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16ZM21 21l-4.3-4.3M8 11h6");
