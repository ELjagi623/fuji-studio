/**
 * Official FUJIFILM product photo for the connected camera model.
 * Images live in src/assets/cameras/<key>.<ext> and are fetched by
 * `npm run fetch-cameras` (© FUJIFILM, not committed). Missing → generic icon.
 */
const files = import.meta.glob("../assets/cameras/*.{jpg,png,webp}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const byKey: Record<string, string> = {};
for (const [path, url] of Object.entries(files)) {
  const key = path.split("/").pop()!.replace(/\.[^.]+$/, "");
  byKey[key] = url;
}

/** "X-Pro3" → "xpro3", "GFX100S II" → "gfx100sii" (matches the file names). */
const modelKey = (model: string) => model.toLowerCase().replace(/[\s_-]/g, "");

export function cameraImage(model: string): string | null {
  return byKey[modelKey(model)] ?? null;
}

export function CameraImage({ model }: { model: string }) {
  const src = cameraImage(model);
  return (
    <div className="camimg">
      {src ? (
        <img src={src} alt={model} draggable={false} />
      ) : (
        <svg viewBox="0 0 64 40" className="camgeneric" aria-hidden>
          <rect x="4" y="8" width="56" height="28" rx="4" fill="none" stroke="currentColor" strokeWidth="2.5" />
          <path d="M22 8 l4-5 h12 l4 5" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinejoin="round" />
          <circle cx="32" cy="22" r="9" fill="none" stroke="currentColor" strokeWidth="2.5" />
          <circle cx="51" cy="14" r="2" fill="currentColor" />
        </svg>
      )}
    </div>
  );
}
