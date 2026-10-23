"use client";
import { useEffect, useState } from "react";
import { useUi } from "../../lib/store";
import { useLibrary } from "../../lib/library";
import { libraryAlbum, libraryArtist, type AlbumProfile, type ArtistProfile } from "../../services/tauri/library";
import type { ArtistRef } from "../../services/tauri/search";
import { Button, Screen, Section, StatusNote, cx } from "../../components/ui";
import { Art } from "../../components/Art";
import { useArtistPhoto } from "../../lib/artistPhotos";
import type { Tone } from "../../lib/media";
import { ResultRow, providerName } from "../search/SourceGroup";
import { AddToPlaylist } from "../search/AddToPlaylist";
import { playTrack, usePlayback } from "../../lib/playback";
import { Play, Shuffle } from "lucide-react";


const TONES: Tone[] = ["slate", "violet", "amber", "rose", "moss", "ink"];
const toneFor = (seed: string): Tone => {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0;
  return TONES[h % TONES.length];
};

function ArtistCard({ artist, songs }: { artist: ArtistRef; songs: number }) {
  const photo = useArtistPhoto(artist);
  const [broken, setBroken] = useState(false);
  const showPhoto = photo && !broken;
  return (
    <button type="button" onClick={() => useUi.getState().openArtist(artist)} aria-label={`Open artist ${artist.name}`}
      className="glass-card group flex h-full w-full flex-col items-center gap-4 rounded-[16px] px-4 pt-6 pb-5 text-center transition-colors duration-150 hover:bg-frost-200">
      <span className="relative block size-28 shrink-0 overflow-hidden rounded-full shadow-[0_6px_20px_rgba(0,0,0,0.5)] transition-transform duration-200 group-hover:scale-[1.03]">
        {showPhoto
          ? <img src={photo} alt="" loading="lazy" onError={() => setBroken(true)} className="size-full object-cover" />
          : <Art seed={artist.name} tone={toneFor(artist.name)} className="size-full" rounded="rounded-full" />}
      </span>
      <span className="flex min-w-0 max-w-full flex-col gap-0.5">
        <span className={cx("line-clamp-2 text-[16px] leading-[22px] font-semibold text-primary-text")}>{artist.name}</span>
        <span className="truncate text-[12px] leading-4 text-secondary-text">Artist · {providerName(artist.provider)} · {songs} liked {songs === 1 ? "song" : "songs"}</span>
      </span>
    </button>
  );
}

function LibraryNav() {
  return <nav aria-label="Library views" className="flex gap-2">{(["songs", "artists", "playlists"] as const).map(tab => <Button key={tab} onClick={() => useUi.getState().openLibrary(tab)}>{tab === "songs" ? "Liked Songs" : tab[0].toUpperCase() + tab.slice(1)}</Button>)}</nav>;
}
export function SavedLibraryScreen({ artists = false }: { artists?: boolean }) {
  const tracks = useLibrary(s => s.tracks);
  const ready = useLibrary(s => s.ready);
  const error = useLibrary(s => s.error);
  const [limit, setLimit] = useState(50);
  const refs = new Map<string, ArtistRef>();
  const counts = new Map<string, number>();
  for (const track of tracks) for (const name of track.artists) {
    const artist = track.artistRefs?.find(a => a.provider === track.provider && a.name === name) ?? { provider: track.provider, id: null, name };
    const key = `${artist.provider}:${artist.id ?? artist.name}`;
    refs.set(key, artist);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return <Screen id="saved-library" title={artists ? "Artists" : "Liked Songs"} caption={artists ? "Artists from your liked songs, grouped by their provider identity." : "Saved on this device. Provider libraries are unchanged."}>
    <LibraryNav />
    {!ready && !error && <p role="status">Loading liked songs…</p>}
    {ready && (artists ? <ul className="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-4">{Array.from(refs.entries()).map(([key, artist]) => <li key={key}><ArtistCard artist={artist} songs={counts.get(key) ?? 0} /></li>)}</ul> : <><ul>{tracks.slice(0, limit).map((hit, i, shown) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={shown.slice(i + 1)} />)}</ul>{tracks.length > limit && <Button onClick={() => setLimit(n => n + 50)}>Show more</Button>}</>)}
    {ready && (artists ? refs.size === 0 : tracks.length === 0) && <StatusNote>Like songs from Search to build your library.<Button onClick={() => useUi.getState().navigate("search")}>Search music</Button></StatusNote>}
  </Screen>;
}
export default function ArtistScreen() {
  const artist = useUi(s => s.selectedArtist);
  const saved = useLibrary(s => s.tracks);
  const [profile, setProfile] = useState<ArtistProfile | null>(null);
  const [status, setStatus] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setProfile(null);
    if (!artist?.id) { setStatus("This track did not include an artist profile ID. Showing your saved songs for this provider."); return; }
    setStatus("Loading artist…");
    libraryArtist(artist.provider, artist.id).then(p => { if (active) { setProfile(p); setStatus(""); } }).catch(error => {
      if (active) setStatus(error === "unsupportedProvider" ? "Artist profiles are not supported by this provider yet." : "Could not load artist. Check your connection and retry.");
    });
    return () => { active = false; };
  }, [artist, retry]);
  if (!artist) return <Screen id="artist" title="Artist"><p>Select an artist from a song.</p></Screen>;
  const tracks = profile?.tracks ?? saved.filter(t => t.provider === artist.provider && (artist.id ? t.artistRefs?.some(a => a.id === artist.id) : t.artists.includes(artist.name)));
  return <Screen id="artist" title={profile?.name ?? artist.name} caption={providerName(artist.provider)}>
    {status && <StatusNote role="status">{status}{artist.id && status !== "Loading artist…" && <Button onClick={() => setRetry(n => n + 1)}>Retry</Button>}</StatusNote>}
    <div className="flex flex-col gap-3 empty:hidden">
    {profile?.artworkUrl && <img src={profile.artworkUrl} alt={`${profile.name} artist artwork`} className="max-h-64 w-full rounded-xl object-cover" />}
    {profile?.subscribers != null && <p className="text-secondary-text">{profile.subscribers.toLocaleString()} subscribers · {providerName(profile.provider)}</p>}
    {profile?.description && <p className="whitespace-pre-line text-secondary-text">{profile.description}</p>}
    </div>
    {tracks.length > 0 && <AlbumActions tracks={tracks} />}
    <Section id="artist-songs" title={profile ? "Songs" : "Liked songs"}><ul>{tracks.map((hit, i) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={tracks.slice(i + 1)} />)}</ul>{tracks.length === 0 && <p>No songs available.</p>}</Section>
    {profile && profile.albums.length > 0 && <Section id="artist-albums" title="Albums"><ul className="grid grid-cols-2 gap-4 md:grid-cols-4">{profile.albums.map(album => <li key={album.id}><button type="button" aria-label={`Open album ${album.name}`} onClick={() => useUi.getState().openAlbum({ provider: profile.provider, ...album })} className="group block w-full text-left">{album.artworkUrl && <img src={album.artworkUrl} alt="" loading="lazy" className="mb-2 aspect-square w-full rounded-lg object-cover transition-transform duration-200 group-hover:scale-[1.02]" />}<p className="text-primary-text">{album.name}</p><p className="text-[12px] text-secondary-text">{album.year} · {providerName(profile.provider)}</p></button></li>)}</ul></Section>}
  </Screen>;
}

export function AlbumScreen() {
  const album = useUi(s => s.selectedAlbum);
  const [profile, setProfile] = useState<AlbumProfile | null>(null);
  const [status, setStatus] = useState("Loading album…");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!album) return;
    let active = true;
    setProfile(null);
    setStatus("Loading album…");
    libraryAlbum(album.provider, album.id).then(p => { if (active) { setProfile(p); setStatus(""); } }).catch(error => {
      if (active) setStatus(error === "unsupportedProvider" ? "Album pages are not supported by this provider yet." : "Could not load album. Check your connection and retry.");
    });
    return () => { active = false; };
  }, [album, retry]);
  if (!album) return <Screen id="album" title="Album"><p>Select an album from an artist.</p></Screen>;
  const art = profile?.artworkUrl ?? album.artworkUrl;
  const year = profile?.year ?? album.year;
  return <Screen id="album" title={profile?.name ?? album.name} caption={[profile?.kind, year, providerName(album.provider)].filter(Boolean).join(" · ")}>
    {status && <StatusNote role="status">{status}{status !== "Loading album…" && <Button onClick={() => setRetry(n => n + 1)}>Retry</Button>}</StatusNote>}
    <div className="flex items-end gap-5">
      {art && <img src={art} alt={`${album.name} cover`} className="size-40 shrink-0 rounded-xl object-cover shadow-[0_8px_28px_rgba(0,0,0,0.5)]" />}
      {profile && profile.artists.length > 0 && <p className="pb-1 text-[15px] leading-5 text-primary-text">{profile.artists.map((a, i) => <span key={`${a.id}:${a.name}`}>{i > 0 && ", "}{a.id ? <button type="button" className="underline-offset-2 hover:text-primary-text hover:underline" onClick={() => useUi.getState().openArtist(a)}>{a.name}</button> : a.name}</span>)}</p>}
    </div>
    {profile && profile.tracks.length > 0 && <AlbumActions tracks={profile.tracks} />}
    {profile && <Section id="album-songs" title="Songs"><ul>{profile.tracks.map((hit, i) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={profile.tracks.slice(i + 1)} />)}</ul>{profile.tracks.length === 0 && <p>No songs available.</p>}</Section>}
  </Screen>;
}

const shuffled = <T,>(items: T[]): T[] => {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [out[i], out[j]] = [out[j], out[i]]; }
  return out;
};
const pill = "inline-flex h-10 items-center gap-2 rounded-full px-5 text-[14px] font-semibold transition-colors duration-150 disabled:opacity-40";

/** Play starts track 1 and queues the rest in order (Core plays them as Up next); Shuffle does the same in random order. */
function AlbumActions({ tracks }: { tracks: ArtistProfile["tracks"] }) {
  const busy = usePlayback((s) => s.busy);
  const start = (list: ArtistProfile["tracks"]) => void playTrack(list[0], false, list.slice(1));
  return <div className="flex flex-wrap items-center gap-3">
    <button type="button" disabled={busy} onClick={() => start(tracks)} className={cx(pill, "bg-lumen-lavender text-black hover:opacity-90")}><Play size={16} fill="currentColor" aria-hidden="true" />Play</button>
    <button type="button" disabled={busy} onClick={() => start(shuffled(tracks))} className={cx(pill, "bg-frost-200 text-primary-text hover:bg-frost-300")}><Shuffle size={16} aria-hidden="true" />Shuffle</button>
    <AddToPlaylist hit={tracks[0]} all={tracks} label="Save to playlist" />
  </div>;
}
