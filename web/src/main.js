import { basicSetup } from "codemirror";
import { emacsStyleKeymap } from "@codemirror/commands";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment, Prec } from "@codemirror/state";
import { tags } from "@lezer/highlight";
import { EditorView, keymap } from "@codemirror/view";
import { Vim, vim } from "@replit/codemirror-vim";
import { lr35902Language } from "./lr35902.js";

const syntaxTheme = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.keyword, color: "#93c5fd", fontWeight: "700" },
    { tag: tags.meta, color: "#c4b5fd" },
    { tag: tags.definition(tags.variableName), color: "#67e8f9" },
    { tag: tags.variableName, color: "#fbbf24" },
    { tag: tags.typeName, color: "#f9a8d4" },
    { tag: tags.atom, color: "#f0abfc" },
    { tag: tags.number, color: "#fdba74" },
    { tag: tags.string, color: "#86efac" },
    { tag: tags.comment, color: "#7890aa", fontStyle: "italic" },
    { tag: tags.operator, color: "#cbd5e1" },
  ]),
);

const editorTheme = EditorView.theme({
  "&": {
    height: "440px",
    backgroundColor: "#111827",
    color: "#dbeafe",
    border: "1px solid #365a91",
    borderRadius: "8px",
    fontSize: "15px",
  },
  ".cm-scroller": {
    overflow: "auto",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    lineHeight: "1.6",
  },
  ".cm-content": { padding: "16px", caretColor: "#eaf2ff" },
  ".cm-gutters": { backgroundColor: "#111827", color: "#7890aa", border: "none" },
  ".cm-activeLine": { backgroundColor: "#19233b" },
  ".cm-activeLineGutter": { backgroundColor: "#19233b", color: "#dbeafe" },
  "&.cm-focused": { outline: "none", borderColor: "#60a5fa" },
});

const editorElement = document.querySelector("#editor");
const result = document.querySelector("#result");
const registers = ["A", "B", "C", "D", "E", "H", "L", "BC", "DE", "HL", "SP"];
const flags = ["Z", "N", "H", "C"];
const storageKey = "lr35902-explorer:v1";
const savedPreferences = loadPreferences();
const registerValues = validRegisterValues(savedPreferences.registers);
const flagValues = validFlagValues(savedPreferences.flags);
const keymapCompartment = new Compartment();
let analysisTimer;
let analysisSerial = 0;
const keymapExtensions = {
  regular: [],
  vim: vim(),
  emacs: Prec.high(
    keymap.of([
      ...emacsStyleKeymap,
      {
        key: "Ctrl-x Ctrl-s",
        run: () => {
          analyze();
          return true;
        },
        preventDefault: true,
      },
    ]),
  ),
};
const initialProgram = `LD A, $12
LD B, $34
ADD A, B
ADD A, $05
HALT`;
const keybindings = document.querySelector("#keybindings");
keybindings.value = ["regular", "vim", "emacs"].includes(savedPreferences.keybindings)
  ? savedPreferences.keybindings
  : "regular";

function loadPreferences() {
  try {
    return JSON.parse(localStorage.getItem(storageKey) ?? "{}") ?? {};
  } catch {
    return {};
  }
}

function validRegisterValues(saved) {
  const values = {};
  if (!saved || typeof saved !== "object") return values;
  for (const name of registers) {
    const value = saved[name];
    const max = ["A", "B", "C", "D", "E", "H", "L"].includes(name) ? 0xff : 0xffff;
    if (Number.isInteger(value) && value >= 0 && value <= max) values[name] = value;
  }
  return values;
}

function validFlagValues(saved) {
  const values = {};
  if (!saved || typeof saved !== "object") return values;
  for (const name of flags) {
    if (typeof saved[name] === "boolean") values[name] = saved[name];
  }
  return values;
}

function savePreferences() {
  try {
    localStorage.setItem(
      storageKey,
      JSON.stringify({ keybindings: keybindings.value, registers: registerValues, flags: flagValues }),
    );
  } catch {}
}

function programFromHash() {
  const hash = window.location.hash;
  if (!hash.startsWith("#asm=")) return initialProgram;
  try {
    return decodeURIComponent(hash.slice(5));
  } catch {
    return initialProgram;
  }
}

const editor = new EditorView({
  doc: programFromHash(),
  extensions: [
    keymapCompartment.of(keymapExtensions[keybindings.value]),
    basicSetup,
    EditorView.contentAttributes.of({ "aria-label": "LR35902 assembly source" }),
    lr35902Language,
    syntaxTheme,
    editorTheme,
    EditorView.updateListener.of((update) => {
      if (update.docChanged && keybindings.value === "regular") scheduleAnalyze();
    }),
    keymap.of([
      {
        key: "Mod-Enter",
        run: () => {
          analyze();
          return true;
        },
      },
    ]),
  ],
  parent: editorElement,
});

function fillSelect(select, values) {
  for (const value of values) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = value;
    select.append(option);
  }
}

const registerName = document.querySelector("#register-name");
const registerValue = document.querySelector("#register-value");
const registerSummary = document.querySelector("#register-values");
const flagName = document.querySelector("#flag-name");
const flagValue = document.querySelector("#flag-value");
const flagSummary = document.querySelector("#flag-values");
const settingError = document.querySelector("#setting-error");
fillSelect(registerName, registers);
fillSelect(flagName, flags);
registerName.value = registers.find((name) => Object.hasOwn(registerValues, name)) ?? "A";
flagName.value = flags.find((name) => Object.hasOwn(flagValues, name)) ?? "Z";

function formatRegister(value, name) {
  const digits = ["A", "B", "C", "D", "E", "H", "L"].includes(name) ? 2 : 4;
  return `0x${value.toString(16).toUpperCase().padStart(digits, "0")}`;
}

function showRegisterValue() {
  const name = registerName.value;
  registerValue.value = Object.hasOwn(registerValues, name)
    ? formatRegister(registerValues[name], name)
    : "";
}

function showFlagValue() {
  flagValue.value = Object.hasOwn(flagValues, flagName.value)
    ? String(flagValues[flagName.value])
    : "false";
}

function updateSummaries() {
  const registerText = registers
    .filter((name) => Object.hasOwn(registerValues, name))
    .map((name) => `${name}=${formatRegister(registerValues[name], name)}`)
    .join(" · ");
  const flagText = flags
    .filter((name) => Object.hasOwn(flagValues, name))
    .map((name) => `${name}=${flagValues[name]}`)
    .join(" · ");
  registerSummary.textContent = registerText || "Registers default to 0";
  flagSummary.textContent = flagText || "Flags default to false";
}

registerName.addEventListener("change", showRegisterValue);
flagName.addEventListener("change", showFlagValue);
document.querySelector("#register-set").addEventListener("click", () => {
  const name = registerName.value;
  const text = registerValue.value.trim();
  const value = text.startsWith("$")
    ? Number.parseInt(text.slice(1), 16)
    : Number(text);
  const max = ["A", "B", "C", "D", "E", "H", "L"].includes(name) ? 0xff : 0xffff;
  if (!Number.isInteger(value) || value < 0 || value > max) {
    settingError.textContent = `Enter an integer from 0 to ${formatRegister(max, name)} for ${name}.`;
    return;
  }
  registerValues[name] = value;
  settingError.textContent = "";
  showRegisterValue();
  updateSummaries();
  savePreferences();
});

document.querySelector("#flag-set").addEventListener("click", () => {
  flagValues[flagName.value] = flagValue.value === "true";
  settingError.textContent = "";
  updateSummaries();
  savePreferences();
});

keybindings.addEventListener("change", () => {
  const mode = keybindings.value;
  clearTimeout(analysisTimer);
  analysisTimer = undefined;
  editor.dispatch({
    effects: keymapCompartment.reconfigure(keymapExtensions[mode]),
  });
  savePreferences();
  if (mode === "regular") scheduleAnalyze();
  editor.focus();
});

document.querySelector("#share").addEventListener("click", async (event) => {
  const url = new URL(window.location.href);
  url.hash = `asm=${encodeURIComponent(editor.state.doc.toString())}`;
  const button = event.currentTarget;
  try {
    await copyToClipboard(url.href);
    button.textContent = "Copied";
    window.setTimeout(() => (button.textContent = "Share"), 1500);
  } catch {
    settingError.textContent = "Could not copy the share link. Check clipboard permissions.";
  }
});

async function copyToClipboard(text) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {}
  }
  const input = document.createElement("textarea");
  input.value = text;
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  const copied = document.execCommand("copy");
  input.remove();
  if (!copied) throw new Error("Clipboard copy failed");
}

document.querySelector("#register-value").addEventListener("keydown", (event) => {
  if (event.key === "Enter") document.querySelector("#register-set").click();
});
document.querySelector("#flag-value").addEventListener("keydown", (event) => {
  if (event.key === "Enter") document.querySelector("#flag-set").click();
});
updateSummaries();
showRegisterValue();
showFlagValue();

function scheduleAnalyze() {
  clearTimeout(analysisTimer);
  analysisSerial += 1;
  analysisTimer = window.setTimeout(analyze, 400);
}

async function analyze() {
  clearTimeout(analysisTimer);
  analysisTimer = undefined;
  const request = ++analysisSerial;
  try {
    const { default: init, analyze_wasm } = await import(
      new URL("./pkg/lr35902_interp.js", import.meta.url)
    );
    await init();

    const values = {
      registers: registerValues,
      flags: flagValues,
    };

    const report = JSON.parse(analyze_wasm(editor.state.doc.toString(), JSON.stringify(values)));
    if (request !== analysisSerial) return;
    result.textContent = `${report.bytes} bytes · ${report.cycles} cycles\n\n${report.instructions
      .map(
        (instruction) =>
          `${String(instruction.line).padStart(3)}  ${String(instruction.bytes).padStart(2)} bytes  ${String(instruction.cycles).padStart(3)} cycles  ${instruction.instruction}`,
      )
      .join("\n")}`;
  } catch (error) {
    if (request !== analysisSerial) return;
    result.textContent = error.toString();
  }
}

Vim.defineEx("write", "w", () => analyze());
document.querySelector("#run").addEventListener("click", analyze);
document.body.classList.remove("loading");
analyze();
