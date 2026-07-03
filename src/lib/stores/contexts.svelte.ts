import { api, type KubeconfigView } from "../api";

export class ContextsStore {
  view = $state<KubeconfigView>({
    contexts: [],
    currentContext: null,
    files: [],
    defaultTarget: "",
    warnings: [],
  });
  error = $state<string | null>(null);

  async refresh(): Promise<void> {
    try {
      this.view = await api.listContexts();
      this.error = null;
    } catch (e) {
      this.error = String(e);
    }
  }
}

export const contexts = new ContextsStore();
