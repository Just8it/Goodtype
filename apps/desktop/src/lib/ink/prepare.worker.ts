import { silhouette } from "./paint";
import { unpackInk, type packInk } from "./workerGeometry";

self.onmessage = ({ data }: MessageEvent<{
  id: number; packed: ReturnType<typeof packInk>; floor: number;
}>) => {
  const paths = unpackInk(data.packed).map(stroke => silhouette(stroke, data.floor));
  self.postMessage({ id: data.id, paths });
};
