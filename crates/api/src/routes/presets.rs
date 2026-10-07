use axum::Json;

/// 回傳所有內建 preset 名稱
pub async fn list() -> Json<Vec<&'static str>> {
    Json(vec!["wisetree", "blackhole", "cat", "cat2", "colorful"])
}
