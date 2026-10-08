//! Rule-based driving coach (no AI): lap comparison, trail-braking rating, feedback texts and
//! session summaries. Ported from the former Python tools in `tools/`.

pub mod compare;
pub mod feedback;
pub mod lap;
pub mod report;
pub mod trailbrake;
pub mod worker;
