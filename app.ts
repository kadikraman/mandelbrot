import type { Answer, Sources } from './app.contract.d.ts';
import { LEVELS, THEMES } from './content.ts';

export const grants = '';
const sources: Sources = {
  levels: async () => LEVELS,
  themes: async () => THEMES,
};
export const answer: Answer = (source, args, store, storage) => sources[source](args, store, storage);
