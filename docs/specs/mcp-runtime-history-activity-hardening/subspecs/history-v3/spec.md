# 子规格：History v3 状态投影与完整性

## 范围

修正当前状态投影，显式表达归档完整度，并检测派生文件不一致。

## 需求回链

- FR-2
- FR-3
- FR-6

## 验收标准（EARS）

1. WHEN 当前 session 有多个 checkpoint THEN `open_items` SHALL 只反映最新 revision 的 `remaining_issues + next_actions`。
2. WHEN 旧 session 含未决事项 THEN 系统 SHALL 只把旧 session 作为 reference，不合并进当前 `open_items`。
3. WHEN checkpoint 缺少 `raw_user_input` THEN 响应 SHALL 返回 `fidelity=partial` 且 `persistence_complete=false`。
4. WHEN Markdown 中结构化 JSON block 损坏 THEN validate SHALL 返回定位信息而不是静默忽略。
5. WHEN派生文件缺失或与事实档案 revision 不一致 THEN validate SHALL 返回 incomplete 或 stale，并可 repair。

## 涉及文件

- `src-tauri/src/tools/history/mod.rs`
- `src-tauri/src/tools/history/model.rs`
- `src-tauri/src/tools/history/markdown.rs`
- `src-tauri/src/tools/history/storage.rs`
- `src-tauri/tests/history_session.rs`

## 不做项

- 不改写既有数字 Markdown 档案。
- 不将历史全文重新放入 bootstrap。

## 设计要点

`snapshot.json` 在 index、manifest、state 全部成功写入后最后提交，仅用于检测派生 generation 完整性。
