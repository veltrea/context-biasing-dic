import { invoke } from "@tauri-apps/api/core";
import { save, confirm } from "@tauri-apps/plugin-dialog";

// バックエンド diff_pair の戻り（kind タグで判別）。
type Row =
  | { kind: "equal"; surface: string }
  | { kind: "adopted"; reference: string; hypothesis: string; reading: string }
  | {
      kind: "rejected";
      reference: string;
      hypothesis: string;
      reference_reading: string;
      hypothesis_reading: string;
    };
type WordCount = { word: string; reading: string; count: number };
type DiffResult = { rows: Row[]; danger_words: WordCount[] };

// 最小 i18n。ブラウザ言語で日英を選ぶ。
const messages = {
  ja: {
    dangerWords: "危険語",
    strict: "厳密一致",
    collectTitle: "収集 (Collect)",
    referenceLabel: "正解文 (Reference)",
    hypothesisLabel: "認識結果 (ASR)",
    referencePh: "正解の文章を貼り付け（複数行可・行ごとに対応）",
    hypothesisPh: "ASR の認識結果を貼り付け（正解と行対応）",
    diff: "照合",
    clear: "クリア",
    diffResult: "差分検出結果",
    accTitle: "溜まっている危険語",
    exportDict: "辞書",
    exportReject: "除外ログ",
    footerPrivacy: "ローカル完結・外部送信なし ／ 外に出るのは語だけ",
    footerTag: "biasdiff: Linguistic Integrity Guaranteed",
    items: (n: number) => `${n} items`,
    empty: "（まだありません）",
    adopted: "Adopted",
    equal: "Equal",
    rejected: "Rejected",
    saved: (p: string) => `保存しました: ${p}`,
    error: (e: string) => `エラー: ${e}`,
    nothingToSave: "保存する語がありません",
    confirmClear: "蓄積した危険語をすべて消去しますか？",
  },
  en: {
    dangerWords: "Danger",
    strict: "exact",
    collectTitle: "Collect",
    referenceLabel: "Reference",
    hypothesisLabel: "ASR result",
    referencePh: "Paste the correct text (multi-line; paired by line)",
    hypothesisPh: "Paste the ASR output (paired with reference by line)",
    diff: "Compare",
    clear: "Clear",
    diffResult: "Diff result",
    accTitle: "Collected danger words",
    exportDict: "Dict",
    exportReject: "Rejects",
    footerPrivacy: "Local-only · no network — only words leave",
    footerTag: "biasdiff: Linguistic Integrity Guaranteed",
    items: (n: number) => `${n} items`,
    empty: "(none yet)",
    adopted: "Adopted",
    equal: "Equal",
    rejected: "Rejected",
    saved: (p: string) => `Saved: ${p}`,
    error: (e: string) => `Error: ${e}`,
    nothingToSave: "Nothing to save",
    confirmClear: "Clear all collected danger words?",
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

function span(cls: string, text: string): HTMLSpanElement {
  const s = document.createElement("span");
  s.className = cls;
  s.textContent = text;
  return s;
}

function renderRows(rows: Row[]) {
  const ul = el<HTMLUListElement>("rows");
  ul.innerHTML = "";
  el("items-count").textContent = t.items(rows.length);

  if (rows.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = t.empty;
    ul.appendChild(li);
    return;
  }

  for (const row of rows) {
    const li = document.createElement("li");
    li.className = `row row-${row.kind}`;

    const mark = span(
      "mark",
      row.kind === "adopted" ? "+" : row.kind === "rejected" ? "−" : "=",
    );

    const body = document.createElement("span");
    body.className = "row-body";
    if (row.kind === "equal") {
      body.append(span("surf", row.surface), span("op", "＝"), span("surf", row.surface));
    } else if (row.kind === "adopted") {
      body.append(
        span("surf keep", row.reference),
        span("op", "←"),
        span("surf strike", row.hypothesis),
        span("reading", `（${row.reading}）`),
      );
    } else {
      body.append(
        span("surf strike dim", row.reference),
        span("op", "/"),
        span("surf strike dim", row.hypothesis),
        span("reading", `（${row.reference_reading} / ${row.hypothesis_reading}）`),
      );
    }

    const label =
      row.kind === "adopted" ? t.adopted : row.kind === "rejected" ? t.rejected : t.equal;
    const badge = span(`badge badge-${row.kind}`, label);

    li.append(mark, body, badge);
    ul.appendChild(li);
  }
}

function renderChips(words: WordCount[]) {
  const ul = el<HTMLUListElement>("chips");
  ul.innerHTML = "";
  el("danger-count").textContent = String(words.length);

  if (words.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = t.empty;
    ul.appendChild(li);
    return;
  }

  words.forEach((w, i) => {
    const li = document.createElement("li");
    li.className = i === 0 ? "chip chip-top" : "chip";
    const left = document.createElement("span");
    left.className = "chip-left";
    left.append(span("chip-word", w.word), span("chip-reading", w.reading));
    li.append(left, span("chip-count", String(w.count)));
    ul.appendChild(li);
  });
}

async function runDiff() {
  const reference = el<HTMLTextAreaElement>("reference").value;
  const hypothesis = el<HTMLTextAreaElement>("hypothesis").value;
  const strict = el<HTMLInputElement>("strict-toggle").checked;
  try {
    const result = await invoke<DiffResult>("diff_pair", { reference, hypothesis, strict });
    renderRows(result.rows);
    renderChips(result.danger_words);
    const adopted = result.rows.filter((r) => r.kind === "adopted").length;
    const rejected = result.rows.filter((r) => r.kind === "rejected").length;
    setStatus(`+${adopted} / −${rejected}`);
  } catch (e) {
    setStatus(t.error(String(e)));
  }
}

async function clearAll() {
  if (!(await confirm(t.confirmClear))) return;
  await invoke("clear");
  renderRows([]);
  renderChips([]);
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

  // Cmd/Ctrl+Enter で照合。
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      runDiff();
    }
  });

  renderRows([]);
  renderChips([]);
});
