import type { Answer, Sources } from './app.contract.d.ts';
import { CHAPTERS } from './content.ts';

export const grants = '';
const sources: Sources = {
  chapters: async () => CHAPTERS,
};
export const answer: Answer = (source, args, store, storage) => sources[source](args, store, storage);
