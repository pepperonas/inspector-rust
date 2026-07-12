import { useCallback, useEffect, useState } from "react";
import { listSnippets, listSnippetCategories, type SnippetCategory } from "../lib/ipc";
import type { Snippet } from "../lib/types";

export function useSnippets() {
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [categories, setCategories] = useState<SnippetCategory[]>([]);

  const refresh = useCallback(async () => {
    const [rows, cats] = await Promise.all([listSnippets(), listSnippetCategories()]);
    setSnippets(rows);
    setCategories(cats);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { snippets, categories, refresh };
}
