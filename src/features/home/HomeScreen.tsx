"use client";
import type { View } from "../../lib/store";
import { useLibrary } from "../../lib/library";
import { useHistory, clearHistory, refreshHistory } from "../../lib/history";
import { Button, Screen, Section, StatusNote } from "../../components/ui";
import TrackShelf from "../discovery/TrackShelf";
import LiveFeed from "../discovery/LiveFeed";
type Props = { onNavigate: (view: View) => void; onOpenPlaylist: (id: string) => void };
export default function HomeScreen(_props: Props) {
  const history = useHistory(s => s.tracks);
  const historyError = useHistory(s => s.error);
  const likes = useLibrary(s => s.tracks);
  return <Screen id="home-title" title="Listen Now" caption="Your music, picked up where you left off.">
    <div className="flex flex-col gap-7">
      {historyError && <StatusNote role="alert">{historyError}<Button onClick={() => void refreshHistory()}>Retry history</Button></StatusNote>}
      {history.length > 0 && <Section id="recently-played" title="Recently played" action={<Button variant="ghost" onClick={() => void clearHistory()}>Clear</Button>}><TrackShelf tracks={history.slice(0, 12)} label="Recently played" /></Section>}
      <LiveFeed kind="home" featured />
      {likes.length > 0 && <Section id="home-liked" title="Your liked songs"><TrackShelf tracks={likes.slice(0, 12)} label="Your liked songs" /></Section>}
    </div>
  </Screen>;
}
