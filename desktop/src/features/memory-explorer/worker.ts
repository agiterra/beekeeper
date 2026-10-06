import { parseMarkdown } from "./parseMarkdown";
self.onmessage = (
  event: MessageEvent<{
    id: number;
    path: string;
    text: string;
    beekeeper: boolean;
  }>,
) => {
  const { id, path, text, beekeeper } = event.data;
  try {
    self.postMessage({ id, document: parseMarkdown(path, text, beekeeper) });
  } catch (error) {
    self.postMessage({ id, error: String(error) });
  }
};
