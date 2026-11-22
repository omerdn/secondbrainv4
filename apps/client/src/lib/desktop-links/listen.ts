/** Native builds handle links through Expo Router (app scheme); no desktop listener. */
export function listenForDeepLinks(_onLink: (url: string) => void): () => void {
  return () => undefined;
}
