const MARKS: Record<string, string> = {
  Spotify: "/icons/spotify.svg",
  YouTube: "/icons/youtube.svg",
  "YouTube Music": "/icons/youtube-music.svg",
};

export function ProviderMark({ source }: { source: string }) {
  const src = MARKS[source];
  return src ? <img src={src} width={12} height={12} alt="" aria-hidden="true" className="brightness-0 invert" /> : null;
}
