import type { Answer, Sources } from './app.contract.d.ts';
import { CHAPTERS, THEMES } from './content.ts';

export const grants = '';
const sources: Sources = {
  chapters: async () => CHAPTERS,
  themes: async () => THEMES,
};
export const answer: Answer = (source, args, store, storage) => sources[source](args, store, storage);
