import { mount } from "svelte";
import App from "./App.svelte";
import { settings } from "./lib/stores/settings.svelte";
import { applyTheme } from "./lib/themes";
import "./app.css";

applyTheme(settings.effectiveTheme);

const app = mount(App, { target: document.getElementById("app")! });
export default app;
