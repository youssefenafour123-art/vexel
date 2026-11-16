/* @refresh reload */
import { render } from "solid-js/web";
import "@fontsource-variable/geist";
import "@fontsource-variable/jetbrains-mono";
import "./styles/global.css";
import App from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("#root missing from index.html");

render(() => <App />, root);
