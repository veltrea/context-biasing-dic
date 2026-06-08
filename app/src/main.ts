import { invoke } from "@tauri-apps/api/core";
import { save, confirm } from "@tauri-apps/plugin-dialog";

type Pair = {
  reference_surface: string;
  hypothesis_surface: string;
  reference_reading: string;
  hypothesis_reading: string;
  homophone: boolean;
};
type WordCount = { word: string; count: number };
type DiffResult = { pairs: Pair[]; danger_words: WordCount[] };

// 最小 i18n。ブラウザ言語で日英を選ぶ。
const messages = {
  ja: {
    strict: "完全一致",
    referenceLabel: "正解文",
    hypothesisLabel: "認識結果",
    referencePh: "正解の文章を貼り付け（複数行可・行ごとに対応）",
    hypothesisPh: "ASR の認識結果を貼り付け（正解と行対応）",
    diff: "diff を取る",
    clear: "クリア",
    lastPairs: "今回の置換",
    dangerWords: "危険語",
    exportDict: "辞書を保存",
    exportReject: "除外ログ",
    empty: "（まだありません）",
    summary: (h: number, r: number) => `採用 ${h} ・ 除外 ${r}`,
    confirmClear: "蓄積した危険語をすべて消去しますか？",
    saved: (p: string) => `保存しました: ${p}`,
    error: (e: string) => `エラー: ${e}`,
    nothingToSave: "保存する語がありません",
  },
  en: {
    strict: "exact",
    referenceLabel: "Reference",
    hypothesisLabel: "ASR result",
    referencePh: "Paste the correct text (multi-line; paired by line)",
    hypothesisPh: "Paste the ASR output (paired with reference by line)",
    diff: "Run diff",
    clear: "Clear",
    lastPairs: "Last replacements",
    dangerWords: "Danger words",
    exportDict: "Save dict",
    exportReject: "Reject log",
    empty: "(none yet)",
    summary: (h: number, r: number) => `${h} kept · ${r} rejected`,
    confirmClear: "Clear all collected danger words?",
    saved: (p: string) => `Saved: ${p}`,
    error: (e: string) => `Error: ${e}`,
    nothingToSave: "Nothing to save",
  },
};

const lang: "ja" | "en" = navigator.language.startsWith("ja") ? "ja" : "en";
const t = messages[lang];

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element: ${id}`);
  return node as T;
}

function setStatus(text: string) {
  el("summary").textContent = text;
}

function applyI18n() {
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((node) => {
    const key = node.dataset.i18n as keyof typeof t;
    const value = t[key];
    if (typeof value === "string") node.textContent = value;
  });
  el<HTMLTextAreaElement>("reference").placeholder = t.referencePh;
  el<HTMLTextAreaElement>("hypothesis").placeholder = t.hypothesisPh;
  document.documentElement.lang = lang;
}

function renderPairs(pairs: Pair[]) {
  const ul = el<HTMLUListElement>("pairs");
  ul.innerHTML = "";
  if (pairs.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = t.empty;
    ul.appendChild(li);
    return;
  }
  for (const p of pairs) {
    const li = document.createElement("li");
    li.className = p.homophone ? "plus" : "minus";

    const mark = document.createElement("span");
    mark.className = "mark";
    mark.textContent = p.homophone ? "+" : "−";

    const body = document.createElement("span");
    body.textContent = p.homophone
      ? `${p.reference_surface} ← ${p.hypothesis_surface}`
      : `${p.reference_surface} / ${p.hypothesis_surface}`;

    const reading = document.createElement("span");
    reading.className = "reading";
    reading.textContent = p.homophone
      ? p.reference_reading
      : `${p.reference_reading} / ${p.hypothesis_reading}`;

    li.append(mark, body, reading);
    ul.appendChild(li);
  }
}

function renderDanger(words: WordCount[]) {
  const ol = el<HTMLOListElement>("danger");
  ol.innerHTML = "";
  if (words.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = t.empty;
    ol.appendChild(li);
    return;
  }
  for (const w of words) {
    const li = document.createElement("li");
    const word = document.createElement("span");
    word.textContent = w.word;
    const count = document.createElement("span");
    count.className = "count";
    count.textContent = String(w.count);
    li.append(word, count);
    ol.appendChild(li);
  }
}

async function runDiff() {
  const reference = el<HTMLTextAreaElement>("reference").value;
  const hypothesis = el<HTMLTextAreaElement>("hypothesis").value;
  const strict = el<HTMLInputElement>("strict-toggle").checked;
  try {
    const result = await invoke<DiffResult>("diff_pair", {
      reference,
      hypothesis,
      strict,
    });
    renderPairs(result.pairs);
    renderDanger(result.danger_words);
    const kept = result.pairs.filter((p) => p.homophone).length;
    setStatus(t.summary(kept, result.pairs.length - kept));
  } catch (e) {
    setStatus(t.error(String(e)));
  }
}

async function clearAll() {
  if (!(await confirm(t.confirmClear))) return;
  await invoke("clear");
  renderPairs([]);
  renderDanger([]);
  setStatus("");
}

async function exportDict() {
  try {
    const words = await invoke<WordCount[]>("danger_words");
    if (words.length === 0) {
      setStatus(t.nothingToSave);
      return;
    }
    const path = await save({
      defaultPath: "dict.txt",
      filters: [{ name: "text", extensions: ["txt"] }],
    });
    if (!path) return;
    await invoke("save_dict", { path });
    setStatus(t.saved(path));
  } catch (e) {
    setStatus(t.error(String(e)));
  }
}

async function exportReject() {
  try {
    const path = await save({
      defaultPath: "reject.txt",
      filters: [{ name: "text", extensions: ["txt"] }],
    });
    if (!path) return;
    await invoke("save_reject", { path });
    setStatus(t.saved(path));
  } catch (e) {
    setStatus(t.error(String(e)));
  }
}

window.addEventListener("DOMContentLoaded", () => {
  applyI18n();
  el("diff-btn").addEventListener("click", runDiff);
  el("clear-btn").addEventListener("click", clearAll);
  el("export-dict-btn").addEventListener("click", exportDict);
  el("export-reject-btn").addEventListener("click", exportReject);

  // Cmd/Ctrl+Enter で diff を実行。
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      runDiff();
    }
  });

  renderPairs([]);
  renderDanger([]);
});
