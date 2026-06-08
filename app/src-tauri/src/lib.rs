//! Tauri バックエンド。biasdiff のコア（diff＋読みフィルタ）を呼び出し、
//! GUI からのコマンドに応える。形態素解析器は起動時に1度だけ構築する。

use biasdiff::collect::Collector;
use biasdiff::morph::{DictKind, LinderaTokenizer};
use biasdiff::pipeline::process;
use biasdiff::reading::NormalizeOptions;
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

/// アプリの共有状態。危険語は collector に蓄積していく。
struct AppState {
    tokenizer: LinderaTokenizer,
    collector: Mutex<Collector>,
}

/// フロントへ返す置換ペア。採用（同音）か除外（読み違い）かを `homophone` で示す。
#[derive(Serialize)]
struct PairDto {
    reference_surface: String,
    hypothesis_surface: String,
    reference_reading: String,
    hypothesis_reading: String,
    homophone: bool,
}

/// 危険語と出現回数。
#[derive(Serialize)]
struct WordCountDto {
    word: String,
    count: usize,
}

/// `diff_pair` の結果。今回の置換ペアと、蓄積後の危険語リスト。
#[derive(Serialize)]
struct DiffResultDto {
    pairs: Vec<PairDto>,
    danger_words: Vec<WordCountDto>,
}

fn to_word_counts(pairs: Vec<(String, usize)>) -> Vec<WordCountDto> {
    pairs
        .into_iter()
        .map(|(word, count)| WordCountDto { word, count })
        .collect()
}

fn pair_dto(c: &biasdiff::pipeline::Candidate, homophone: bool) -> PairDto {
    PairDto {
        reference_surface: c.reference_surface.clone(),
        hypothesis_surface: c.hypothesis_surface.clone(),
        reference_reading: c.reference_reading.clone(),
        hypothesis_reading: c.hypothesis_reading.clone(),
        homophone,
    }
}

/// 正解文と認識結果を行対応で diff し、同音衝突を蓄積して結果を返す。
#[tauri::command]
fn diff_pair(
    reference: String,
    hypothesis: String,
    strict: bool,
    state: State<AppState>,
) -> Result<DiffResultDto, String> {
    let opts = if strict {
        NormalizeOptions::strict()
    } else {
        NormalizeOptions::loose()
    };

    let ref_lines: Vec<&str> = reference.lines().collect();
    let hyp_lines: Vec<&str> = hypothesis.lines().collect();

    let mut pairs = Vec::new();
    let mut collector = state.collector.lock().map_err(|e| e.to_string())?;

    for (r, h) in ref_lines.iter().zip(hyp_lines.iter()) {
        if r.trim().is_empty() && h.trim().is_empty() {
            continue;
        }
        let outs = process(&state.tokenizer, r, h, &opts).map_err(|e| e.to_string())?;
        for o in &outs {
            pairs.push(pair_dto(o.candidate(), o.is_homophone()));
        }
        collector.add_all(outs);
    }

    Ok(DiffResultDto {
        pairs,
        danger_words: to_word_counts(collector.danger_words_sorted()),
    })
}

/// 現在の危険語リスト（頻度順）。
#[tauri::command]
fn danger_words(state: State<AppState>) -> Result<Vec<WordCountDto>, String> {
    let collector = state.collector.lock().map_err(|e| e.to_string())?;
    Ok(to_word_counts(collector.danger_words_sorted()))
}

/// 除外（読み不一致）ペア一覧。
#[tauri::command]
fn reject_pairs(state: State<AppState>) -> Result<Vec<PairDto>, String> {
    let collector = state.collector.lock().map_err(|e| e.to_string())?;
    let pairs = collector
        .reject_pairs()
        .iter()
        .map(|c| pair_dto(c, false))
        .collect();
    Ok(pairs)
}

/// 危険語リスト（1行1語）を指定パスへ保存する。
#[tauri::command]
fn save_dict(path: String, state: State<AppState>) -> Result<(), String> {
    let collector = state.collector.lock().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    collector.write_dict(&mut buf).map_err(|e| e.to_string())?;
    std::fs::write(&path, buf).map_err(|e| e.to_string())
}

/// 除外ログを指定パスへ保存する。
#[tauri::command]
fn save_reject(path: String, state: State<AppState>) -> Result<(), String> {
    let collector = state.collector.lock().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    collector.write_reject(&mut buf).map_err(|e| e.to_string())?;
    std::fs::write(&path, buf).map_err(|e| e.to_string())
}

/// 蓄積をリセットする。
#[tauri::command]
fn clear(state: State<AppState>) -> Result<(), String> {
    let mut collector = state.collector.lock().map_err(|e| e.to_string())?;
    *collector = Collector::new();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let tokenizer =
        LinderaTokenizer::new(DictKind::Ipadic).expect("failed to load embedded IPADIC dictionary");
    let state = AppState {
        tokenizer,
        collector: Mutex::new(Collector::new()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            diff_pair,
            danger_words,
            reject_pairs,
            save_dict,
            save_reject,
            clear
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
