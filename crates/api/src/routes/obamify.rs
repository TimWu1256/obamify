use axum::{
    extract::Multipart,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    Json,
};
use obamify_core::{
    calculate::{self, ProgressMsg},
    calculate::util::{Algorithm, GenerationSettings, CropScale},
    preset::{Preset, UnprocessedPreset},
    ports::ProgressSink,
};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use uuid::Uuid;

use crate::dto::{AssignmentsResponse, ErrorResponse};

/// POST /obamify/assignments
/// multipart fields: source (image bytes, 必填), preset (string, 選填), resolution (u32, 選填)
pub async fn assignments(mut multipart: Multipart) -> Response {
    // 1. 解析 multipart 欄位
    let mut source_bytes: Option<Vec<u8>> = None;
    let mut preset_name = "wisetree".to_string();
    let mut resolution: u32 = 128;
    let mut proximity_importance: i64 = 13;
    let mut algorithm = Algorithm::Genetic;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("source") => {
                source_bytes = field.bytes().await.ok().map(|b| b.to_vec());
            }
            Some("preset") => {
                if let Ok(s) = field.text().await { preset_name = s; }
            }
            Some("resolution") => {
                if let Ok(s) = field.text().await {
                    resolution = s.parse().unwrap_or(128);
                }
            }
            Some("proximity_importance") => {
                if let Ok(s) = field.text().await {
                    proximity_importance = s.parse().unwrap_or(13);
                }
            }
            Some("algorithm") => {
                if let Ok(s) = field.text().await {
                    algorithm = if s == "optimal" { Algorithm::Optimal } else { Algorithm::Genetic };
                }
            }
            _ => {}
        }
    }

    // 2. 驗證 source
    let source_bytes = match source_bytes {
        Some(b) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "missing 'source' field"),
    };

    // 3. 解碼圖片並建立 UnprocessedPreset
    let img = match image::load_from_memory(&source_bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("invalid image: {e}")),
    };
    let (w, h) = img.dimensions();
    let unprocessed = UnprocessedPreset {
        name: preset_name.clone(),
        width: w,
        height: h,
        source_img: img.into_raw(),
    };

    // 4. 建立 GenerationSettings
    // 因為 GenerationSettings::custom_target 是 private，所以使用 default 建構子，然後再設定 public 欄位。
    let mut settings = GenerationSettings::default(Uuid::new_v4(), preset_name.clone());
    settings.proximity_importance = proximity_importance;
    settings.algorithm = algorithm;
    settings.sidelen = resolution;
    settings.target_crop_scale = CropScale::identity();
    settings.source_crop_scale = CropScale::identity();

    // 5. 在 blocking thread 跑計算（避免 block tokio runtime）
    let result = tokio::task::spawn_blocking(move || {
        let mut result: Option<Preset> = None;
        let cancel = Arc::new(AtomicBool::new(false));

        let mut sink = |msg: ProgressMsg| {
            if let ProgressMsg::Done(preset) = msg {
                result = Some(preset);
            }
        };

        let _ = calculate::process(unprocessed, settings, &mut sink, cancel);
        result
    }).await;

    // 6. 回傳結果
    match result {
        Ok(Some(preset)) => {
            let resp = AssignmentsResponse {
                width: preset.inner.width,
                height: preset.inner.height,
                assignments: preset.assignments,
                preset: preset_name,
            };
            Json(resp).into_response()
        }
        Ok(None) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "calculation did not produce result"),
        Err(e)   => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// POST /obamify/gif
/// 目前回傳 501 Not Implemented，等 render crate 完成後接入
pub async fn gif(_multipart: Multipart) -> Response {
    error_response(StatusCode::NOT_IMPLEMENTED, "gif endpoint requires GPU render, coming soon")
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    let body = Json(ErrorResponse { error: msg.to_string() });
    (status, body).into_response()
}
