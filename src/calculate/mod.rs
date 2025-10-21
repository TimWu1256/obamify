// Minimal compatibility shim for calculate module used by drawing-mode code.
pub mod drawing_process;
pub mod util;

// Re-export a couple of items used in other modules (stubs kept minimal).
pub use drawing_process::DRAWING_CANVAS_SIZE;

// Progress messages used by the calculation routines (minimal subset required by CLI)
// GUI has a richer ProgressMsg; CLI only needs UpdateAssignments and Cancelled for now.
#[derive(Clone, Debug)]
pub enum ProgressMsg {
	UpdateAssignments(Vec<usize>),
	Cancelled,
}

// Re-export useful items from util so child modules can refer to them as `super::...`
pub use util::GenerationSettings;
pub use util::heuristic;
