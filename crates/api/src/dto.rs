use serde::Serialize;

/// POST /obamify/assignments 回傳格式
#[derive(Serialize)]
pub struct AssignmentsResponse {
    pub assignments: Vec<usize>,
    pub width: u32,
    pub height: u32,
    pub preset: String,
}

/// 通用錯誤回傳
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
