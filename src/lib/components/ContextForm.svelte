<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ContextSpec } from "../api";
  import { contexts } from "../stores/contexts.svelte";

  let { name = null, onclose }: { name?: string | null; onclose: () => void } = $props();
  const isEdit = name !== null;

  let error = $state<string | null>(null);
  let loaded = $state(!isEdit);

  let ctxName = $state("");
  let namespace = $state("");
  let targetFile = $state("");

  let clusterMode = $state<"existing" | "new">(isEdit ? "new" : "existing");
  let clusterRef = $state("");
  let clusterName = $state("");
  let server = $state("");
  let caMode = $state<"none" | "file" | "inline" | "skip">("none");
  let caFile = $state("");
  let caData = $state("");

  let userMode = $state<"existing" | "new">(isEdit ? "new" : "existing");
  let userRef = $state("");
  let userName = $state("");
  let authMethod = $state<"token" | "cert" | "exec">("token");
  let token = $state("");
  let showToken = $state(false);
  let certFile = $state("");
  let keyFile = $state("");
  let certData = $state("");
  let keyData = $state("");
  let execCommand = $state("");
  let execApiVersion = $state("client.authentication.k8s.io/v1beta1");
  let execArgsText = $state("");
  let execEnvText = $state("");

  const clusterNames = $derived([...new Set(contexts.view.contexts.map((c) => c.cluster))]);
  const userNames = $derived([...new Set(contexts.view.contexts.map((c) => c.user))]);

  onMount(async () => {
    targetFile = contexts.view.defaultTarget;
    if (!isEdit) return;
    try {
      const d = await api.getContext(name!);
      ctxName = d.name;
      namespace = d.namespace ?? "";
      clusterName = d.clusterName;
      server = d.server ?? "";
      if (d.caFile) { caMode = "file"; caFile = d.caFile; }
      else if (d.caData) { caMode = "inline"; caData = d.caData; }
      else if (d.insecureSkipTlsVerify) caMode = "skip";
      userName = d.userName;
      if (d.execCommand) {
        authMethod = "exec";
        execCommand = d.execCommand;
        execArgsText = d.execArgs.join("\n");
        execEnvText = d.execEnv.map(([k, v]) => `${k}=${v}`).join("\n");
        if (d.execApiVersion) execApiVersion = d.execApiVersion;
      } else if (d.token) {
        authMethod = "token";
        token = d.token;
      } else if (d.clientCertificate || d.clientCertificateData) {
        authMethod = "cert";
        certFile = d.clientCertificate ?? "";
        keyFile = d.clientKey ?? "";
        certData = d.clientCertificateData ?? "";
        keyData = d.clientKeyData ?? "";
      }
      loaded = true;
    } catch (e) {
      error = String(e);
    }
  });

  function buildSpec(): ContextSpec {
    const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);
    return {
      name: ctxName.trim(),
      originalName: isEdit ? name! : undefined,
      namespace: namespace.trim() || undefined,
      targetFile: isEdit ? undefined : targetFile || undefined,
      cluster:
        clusterMode === "existing"
          ? { existing: clusterRef }
          : {
              name: clusterName.trim(),
              server: server.trim() || undefined,
              caFile: caMode === "file" ? caFile.trim() || undefined : undefined,
              caData: caMode === "inline" ? caData.trim() || undefined : undefined,
              insecureSkipTlsVerify: caMode === "skip" ? true : undefined,
            },
      user:
        userMode === "existing"
          ? { existing: userRef }
          : {
              name: userName.trim(),
              token: authMethod === "token" ? token || undefined : undefined,
              clientCertificate: authMethod === "cert" ? certFile.trim() || undefined : undefined,
              clientKey: authMethod === "cert" ? keyFile.trim() || undefined : undefined,
              clientCertificateData: authMethod === "cert" ? certData.trim() || undefined : undefined,
              clientKeyData: authMethod === "cert" ? keyData.trim() || undefined : undefined,
              execCommand: authMethod === "exec" ? execCommand.trim() || undefined : undefined,
              execArgs: authMethod === "exec" ? lines(execArgsText) : undefined,
              execEnv:
                authMethod === "exec"
                  ? lines(execEnvText).map((l) => {
                      const i = l.indexOf("=");
                      return [l.slice(0, i), l.slice(i + 1)] as [string, string];
                    })
                  : undefined,
              execApiVersion: authMethod === "exec" ? execApiVersion : undefined,
            },
    };
  }

  async function submit(e: Event) {
    e.preventDefault();
    error = null;
    if (!ctxName.trim()) {
      error = "context name is required";
      return;
    }
    if (clusterMode === "existing" && !clusterRef) {
      error = "pick an existing cluster or define a new one";
      return;
    }
    if (userMode === "existing" && !userRef) {
      error = "pick an existing user or define a new one";
      return;
    }
    try {
      await api.saveContext(buildSpec());
      onclose();
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="launch form">
  <header>
    <h1>{isEdit ? `Edit ${name}` : "New context"}</h1>
    <button onclick={onclose}>Cancel</button>
  </header>

  {#if error}<div class="error">{error}</div>{/if}

  {#if loaded}
    <form onsubmit={submit}>
      <section>
        <h3>Context</h3>
        <label>Name <input bind:value={ctxName} placeholder="prod-us-east-1" /></label>
        <label>Default namespace <input bind:value={namespace} placeholder="(all)" /></label>
        {#if !isEdit}
          <label>Write to file
            <select bind:value={targetFile}>
              {#each contexts.view.files as f}<option value={f}>{f}</option>{/each}
            </select>
          </label>
        {/if}
      </section>

      <section>
        <h3>Cluster</h3>
        {#if !isEdit}
          <label class="radio"><input type="radio" bind:group={clusterMode} value="existing" /> Use existing</label>
          <label class="radio"><input type="radio" bind:group={clusterMode} value="new" /> Define new</label>
        {/if}
        {#if clusterMode === "existing"}
          <label>Cluster
            <select bind:value={clusterRef}>
              <option value="" disabled>pick…</option>
              {#each clusterNames as c}<option value={c}>{c}</option>{/each}
            </select>
          </label>
        {:else}
          <label>Cluster name <input bind:value={clusterName} /></label>
          <label>Server URL <input bind:value={server} placeholder="https://…" class="mono" /></label>
          <label>Certificate authority
            <select bind:value={caMode}>
              <option value="none">none</option>
              <option value="file">CA file path</option>
              <option value="inline">inline PEM (base64)</option>
              <option value="skip">insecure-skip-tls-verify</option>
            </select>
          </label>
          {#if caMode === "file"}<label>CA file <input bind:value={caFile} class="mono" /></label>{/if}
          {#if caMode === "inline"}<label>CA data <textarea bind:value={caData} rows="3" class="mono"></textarea></label>{/if}
        {/if}
      </section>

      <section>
        <h3>User</h3>
        {#if !isEdit}
          <label class="radio"><input type="radio" bind:group={userMode} value="existing" /> Use existing</label>
          <label class="radio"><input type="radio" bind:group={userMode} value="new" /> Define new</label>
        {/if}
        {#if userMode === "existing"}
          <label>User
            <select bind:value={userRef}>
              <option value="" disabled>pick…</option>
              {#each userNames as u}<option value={u}>{u}</option>{/each}
            </select>
          </label>
        {:else}
          <label>User name <input bind:value={userName} /></label>
          <label>Auth method
            <select bind:value={authMethod}>
              <option value="token">bearer token</option>
              <option value="cert">client certificate</option>
              <option value="exec">exec plugin</option>
            </select>
          </label>
          {#if authMethod === "token"}
            <label>Token
              <span class="reveal">
                {#if showToken}<input bind:value={token} class="mono" />{:else}<input type="password" bind:value={token} />{/if}
                <button type="button" onclick={() => (showToken = !showToken)}>{showToken ? "Hide" : "Reveal"}</button>
              </span>
            </label>
          {:else if authMethod === "cert"}
            <label>Client certificate file <input bind:value={certFile} class="mono" /></label>
            <label>Client key file <input bind:value={keyFile} class="mono" /></label>
            <label>Client certificate data (base64, optional)
              <textarea bind:value={certData} rows="2" class="mono"></textarea>
            </label>
            <label>Client key data (base64, optional)
              <textarea bind:value={keyData} rows="2" class="mono"></textarea>
            </label>
          {:else}
            <label>Command <input bind:value={execCommand} placeholder="aws" class="mono" /></label>
            <label>Args (one per line)
              <textarea bind:value={execArgsText} rows="4" class="mono" placeholder={"eks\nget-token\n--cluster-name\nmy-cluster"}></textarea>
            </label>
            <label>Env (KEY=VALUE per line)
              <textarea bind:value={execEnvText} rows="2" class="mono" placeholder="AWS_PROFILE=prod"></textarea>
            </label>
            <label>API version <input bind:value={execApiVersion} class="mono" /></label>
          {/if}
        {/if}
      </section>

      <button class="primary" type="submit">{isEdit ? "Save changes" : "Create context"}</button>
    </form>
  {:else if !error}
    <p class="dim">Loading…</p>
  {/if}
</div>
