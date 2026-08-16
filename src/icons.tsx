//! The icon system, and the category model both surfaces read.
//
// One drawn set — 24×24, 2px stroke, round caps, shapes after Lucide (ISC; credited in
// THIRD_PARTY_NOTICES.md) — used three ways from this single source:
//
//   * `<Icon>` renders it in the panel (buttons, chips, result rows);
//   * `map.ts` rasterises the category glyphs onto coloured discs for the map's markers;
//   * `categoryOf` turns a place's `kind` ("railway station", "fast food") into the one
//     category id both of those colour by.
//
// The point is that a fuel station is the SAME blue disc with the same pump in the list,
// on the map, and in a suggestion — one vocabulary, learned once. Green stops meaning
// "result" and goes back to meaning "this app": only a place we cannot classify wears it.

import type { JSX } from "react";

/** Stroke paths per icon id, in a 24×24 box. `c` entries are circles [cx, cy, r]. */
export const GLYPHS: Record<string, { d: string[]; c?: [number, number, number][] }> = {
  // ── controls ──
  search: { d: ["m21 21-4.34-4.34"], c: [[11, 11, 8]] },
  x: { d: ["M18 6 6 18", "m6 6 12 12"] },
  "chevron-left": { d: ["m15 18-6-6 6-6"] },
  "chevron-right": { d: ["m9 18 6-6-6-6"] },
  "arrow-right": { d: ["M5 12h14", "m12 5 7 7-7 7"] },
  plus: { d: ["M5 12h14", "M12 5v14"] },
  trash: { d: ["M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2", "M10 11v6", "M14 11v6"] },
  download: { d: ["M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "m7 10 5 5 5-5", "M12 15V3"] },
  clock: { d: ["M12 6v6l4 2"], c: [[12, 12, 10]] },
  "arrow-down-up": { d: ["m3 16 4 4 4-4", "M7 20V4", "m21 8-4-4-4 4", "M17 4v16"] },

  // ── categories ──
  cafe: { d: ["M10 2v2", "M14 2v2", "M17 8h1a3 3 0 1 1 0 6h-1", "M3 8h14v7a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4Z"] },
  restaurant: { d: ["M3 2v7a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2V2", "M7 2v20", "M21 15V2a5 5 0 0 0-5 5v6a2 2 0 0 0 2 2h3Z", "M21 15v7"] },
  bar: { d: ["M8 22h8", "M12 11v11", "m19 3-7 8-7-8Z"] },
  bakery: { d: ["M12 3a9 9 0 0 1 9 9v1a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-1a9 9 0 0 1 9-9Z", "M3 19h18", "M8 8v2", "M12 7v2", "M16 8v2"] },
  hotel: { d: ["M2 4v16", "M2 8h18a2 2 0 0 1 2 2v10", "M2 17h20", "M6 8v9"] },
  shop: { d: ["M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z", "M3 6h18", "M16 10a4 4 0 0 1-8 0"] },
  pharmacy: { d: ["M9 3h6v6h6v6h-6v6H9v-6H3V9h6Z"] },
  hospital: { d: ["M22 12h-2.5a2 2 0 0 0-1.9 1.4l-2.1 6.4a.3.3 0 0 1-.6 0L9.1 4.2a.3.3 0 0 0-.6 0l-2.1 6.4A2 2 0 0 1 4.5 12H2"] },
  fuel: { d: ["M3 22h12", "M4 9h10", "M14 22V4a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v18", "M14 13h2a2 2 0 0 1 2 2v2a2 2 0 1 0 4 0V9.8a2 2 0 0 0-.6-1.4L18 5"] },
  parking: { d: ["M3 5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z", "M9 17V7h4a3 3 0 0 1 0 6H9"] },
  bank: { d: ["M3 22h18", "M6 18v-7", "M10 18v-7", "M14 18v-7", "M18 18v-7", "m12 2 9 5H3Z"] },
  transit: { d: ["M4 4a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v11a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3Z", "M4 11h16", "M12 2v9", "m8 18-2 4", "m16 18 2 4"], c: [[8.5, 15, 0.5], [15.5, 15, 0.5]] },
  park: { d: ["m12 2 5 7h-3l4 6h-4l4 6H6l4-6H6l4-6H7Z", "M12 21v1"] },
  landmark: { d: ["M2 20h20", "M4 20V10", "M20 20V10", "M8 20v-6", "M12 20v-6", "M16 20v-6", "m12 2 8 6H4Z"] },
  place: { d: ["M20 10c0 5-5.5 10.2-7.4 11.8a1 1 0 0 1-1.2 0C9.5 20.2 4 15 4 10a8 8 0 0 1 16 0Z"], c: [[12, 10, 3]] },
  pin: { d: ["M20 10c0 5-5.5 10.2-7.4 11.8a1 1 0 0 1-1.2 0C9.5 20.2 4 15 4 10a8 8 0 0 1 16 0Z"], c: [[12, 10, 3]] },
};

/** One place category: what to draw it with, and in what colour. The colours hold a
 *  white glyph at ≥3:1 and survive both basemaps. */
export const CATEGORIES_META: Record<string, { color: string; icon: string }> = {
  cafe: { color: "#9C5B2B", icon: "cafe" },
  restaurant: { color: "#CE5A24", icon: "restaurant" },
  bar: { color: "#8352B5", icon: "bar" },
  bakery: { color: "#A5771E", icon: "bakery" },
  hotel: { color: "#3F74BC", icon: "hotel" },
  shop: { color: "#BC4E88", icon: "shop" },
  pharmacy: { color: "#1F9563", icon: "pharmacy" },
  hospital: { color: "#CC4A4A", icon: "hospital" },
  fuel: { color: "#5763CE", icon: "fuel" },
  parking: { color: "#3570CE", icon: "parking" },
  bank: { color: "#6E7F35", icon: "bank" },
  transit: { color: "#2E939B", icon: "transit" },
  park: { color: "#4E8A39", icon: "park" },
  landmark: { color: "#71788A", icon: "landmark" },
  place: { color: "#64748B", icon: "place" },
  pin: { color: "#0E9F70", icon: "pin" },
};

/** A place's `kind` phrase → the category that colours it, everywhere at once.
 *
 *  Substring matching on the human phrase the core already produces ("railway station",
 *  "fast food"), most-specific first — so "bus station" is transit, not shop. */
export function categoryOf(kind: string): string {
  const k = ` ${kind.toLowerCase()} `;
  const has = (...words: string[]) => words.some((w) => k.includes(w));
  if (has("cafe", "coffee", "tea")) return "cafe";
  if (has("restaurant", "fast food", "food court", "ice cream")) return "restaurant";
  if (has("bar", "pub", "biergarten", "nightclub")) return "bar";
  if (has("bakery", "pastry", "confection")) return "bakery";
  if (has("hotel", "hostel", "guest", "lodging", "motel", "chalet")) return "hotel";
  if (has("pharmacy", "chemist")) return "pharmacy";
  if (has("hospital", "clinic", "doctor", "dentist")) return "hospital";
  if (has("fuel", "charging")) return "fuel";
  if (has("parking")) return "parking";
  if (has("bank", "atm", "bureau de change")) return "bank";
  if (has("station", "tram", "subway", "railway", "bus stop", "halt", "ferry", "platform"))
    return "transit";
  if (has("park", "garden", "playground", "forest", "wood", "pitch", "grass")) return "park";
  if (
    has("museum", "attraction", "monument", "castle", "memorial", "worship", "mosque",
        "church", "artwork", "viewpoint", "tower", "ruins", "archaeological")
  )
    return "landmark";
  if (has("supermarket", "convenience", "grocery", "mall", "department", "marketplace",
          "shop", "store", "kiosk", "deli", "greengrocer"))
    return "shop";
  if (has("city", "town", "suburb", "village", "quarter", "neighbourhood", "hamlet",
          "locality", "square", "district"))
    return "place";
  return "pin";
}

/** The panel's icon: current-color stroke, so surfaces tint it like text. */
export function Icon({ id, size = 16 }: { id: string; size?: number }): JSX.Element {
  const g = GLYPHS[id] ?? GLYPHS.pin;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {g.d.map((d, i) => (
        <path key={i} d={d} />
      ))}
      {(g.c ?? []).map(([cx, cy, r], i) => (
        <circle key={`c${i}`} cx={cx} cy={cy} r={r} />
      ))}
    </svg>
  );
}

/** A category's coloured disc with its white glyph — the list-row twin of the map marker. */
export function CategoryDisc({ kind, size = 26 }: { kind: string; size?: number }): JSX.Element {
  const cat = CATEGORIES_META[categoryOf(kind)];
  return (
    <span
      className="catdisc"
      style={{ background: cat.color, width: size, height: size }}
      aria-hidden
    >
      <Icon id={cat.icon} size={Math.round(size * 0.58)} />
    </span>
  );
}

/** Rasterise one category marker for the map: coloured disc, white ring, white glyph.
 *  Drawn at 2× and registered with `pixelRatio: 2`, so it is crisp on retina. */
export function markerImage(catId: string): ImageData {
  const cat = CATEGORIES_META[catId] ?? CATEGORIES_META.pin;
  const S = 56; // 2× of a 28px marker
  const canvas = document.createElement("canvas");
  canvas.width = S;
  canvas.height = S;
  const ctx = canvas.getContext("2d")!;
  const cx = S / 2;

  ctx.beginPath();
  ctx.arc(cx, cx, cx - 3, 0, Math.PI * 2);
  ctx.fillStyle = cat.color;
  ctx.fill();
  ctx.lineWidth = 4;
  ctx.strokeStyle = "#ffffff";
  ctx.stroke();

  // The glyph: 24-box paths scaled into the disc.
  const g = GLYPHS[cat.icon] ?? GLYPHS.pin;
  const scale = (S - 26) / 24;
  ctx.save();
  ctx.translate(cx - (24 * scale) / 2, cx - (24 * scale) / 2);
  ctx.scale(scale, scale);
  ctx.lineWidth = 2.4 / scale > 2.4 ? 2.4 : 2.4; // constant visual weight
  ctx.lineWidth = 2.4;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.strokeStyle = "#ffffff";
  for (const d of g.d) ctx.stroke(new Path2D(d));
  for (const [gx, gy, r] of g.c ?? []) {
    ctx.beginPath();
    ctx.arc(gx, gy, r, 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.restore();

  return ctx.getImageData(0, 0, S, S);
}
