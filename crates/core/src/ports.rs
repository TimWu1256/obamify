//! Ports（六角架構介面定義）
use crate::calculate::ProgressMsg;

/// Domain → 外界：進度回報 Port
pub trait ProgressSink: Send {
    fn send(&mut self, msg: ProgressMsg);
}

impl ProgressSink for std::sync::mpsc::SyncSender<ProgressMsg> {
    fn send(&mut self, msg: ProgressMsg) {
        let _ = std::sync::mpsc::SyncSender::send(self, msg);
    }
}

impl<F> ProgressSink for F
where
    F: FnMut(ProgressMsg) + Send,
{
    fn send(&mut self, msg: ProgressMsg) {
        self(msg);
    }
}

/// 外界 → Domain：Preset 存取 Port
pub trait PresetRepository: Send + Sync {
    fn list_names(&self) -> Vec<String>;
    fn load(&self, name: &str) -> Option<crate::preset::Preset>;
}
